use crate::database::Database;
use crate::gpu::{GpuTileHandle, SimilarityComputer};
use crate::matcher::{MatchResult, Matcher, ProgressCallback as MatcherProgressCallback};
use crate::vectorizer::{Vectorizer, VECTOR_SIZE};
use log::info;
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};
use wgpu::Buffer;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchEngineKind {
    Cpu,
    Gpu,
}

pub type MatchProgressCallback = MatcherProgressCallback;

pub trait MatchEngine: Send {
    fn kind(&self) -> MatchEngineKind;

    fn match_and_store(
        &mut self,
        hh_ids: &[String],
        db: &mut Database,
        min_similarity: f64,
        progress_callback: Option<MatchProgressCallback>,
    ) -> Result<usize, String>;
}

pub fn create_engine(kind: MatchEngineKind) -> Result<Box<dyn MatchEngine>, String> {
    match kind {
        MatchEngineKind::Cpu => Ok(Box::new(CpuMatchEngine::default())),
        MatchEngineKind::Gpu => Ok(Box::new(GpuMatchEngine::new()?)),
    }
}

fn make_logging_progress_callback(
    activity: &'static str,
    unit_label: &'static str,
    total_hint: usize,
) -> MatchProgressCallback {
    let mut last_percent: Option<usize> = None;
    Arc::new(Mutex::new(move |completed: usize, total: usize| {
        let total_units = if total == 0 { total_hint.max(1) } else { total };
        let display_total = if total == 0 { total_hint } else { total };
        let done_units = if display_total == 0 {
            completed
        } else {
            completed.min(display_total)
        };

        let percent = if total_units == 0 {
            100
        } else {
            ((done_units.min(total_units) as f64 / total_units as f64) * 100.0)
                .round()
                .clamp(0.0, 100.0) as usize
        };

        let should_log = match last_percent {
            Some(prev) => percent >= prev.saturating_add(5) || (percent == 100 && percent != prev),
            None => true,
        };

        if should_log {
            let display_total_value = if display_total == 0 {
                total_hint.max(1)
            } else {
                display_total
            };
            info!(
                "{} progress: {}% ({} / {} {})",
                activity, percent, done_units, display_total_value, unit_label
            );
            last_percent = Some(percent);
        }
    }))
}

fn env_chunk(key: &str, default: usize) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
}

#[derive(Default)]
struct CpuMatchEngine {
    matcher: Matcher,
}

impl MatchEngine for CpuMatchEngine {
    fn kind(&self) -> MatchEngineKind {
        MatchEngineKind::Cpu
    }

    fn match_and_store(
        &mut self,
        hh_ids: &[String],
        db: &mut Database,
        min_similarity: f64,
        progress_callback: Option<MatchProgressCallback>,
    ) -> Result<usize, String> {
        let total_ids = hh_ids.len();
        let mut progress = progress_callback;

        if total_ids > 0 && progress.is_none() {
            progress = Some(make_logging_progress_callback(
                "CPU matching",
                "IDs",
                total_ids,
            ));
        }

        if let Some(ref callback) = progress {
            if let Ok(mut cb) = callback.lock() {
                cb(0, total_ids);
            }
            self.matcher.set_progress_handle(callback.clone());
        } else {
            self.matcher.clear_progress_callback();
        }

        if total_ids == 0 {
            info!("CPU matching completed immediately: no household IDs provided");
            return Ok(0);
        }

        info!(
            "CPU matching started: processing {} household IDs",
            total_ids
        );

        let result = self.matcher.match_and_store(hh_ids, db, min_similarity);

        if let Ok(matches) = result {
            info!(
                "CPU matching finished: stored {} matches for {} household IDs",
                matches, total_ids
            );
        }

        result
    }
}

struct GpuMatchEngine {
    vectorizer: Vectorizer,
    computer: SimilarityComputer,
    chunk_size: usize,
    file_chunk_size: usize,
    inflight_limit: usize,
    file_vectors: HashMap<i64, Vec<f32>>,
    file_gpu_buffer: Option<(Arc<Buffer>, usize, u64)>,
}

impl GpuMatchEngine {
    fn new() -> Result<Self, String> {
        let chunk_size = env_chunk("TIFF_GPU_QUERY_CHUNK", 64);
        let file_chunk_size = env_chunk("TIFF_GPU_FILE_CHUNK", 256);
        let inflight_limit = env_chunk("TIFF_GPU_INFLIGHT", 2);
        Ok(Self {
            vectorizer: Vectorizer::new(),
            computer: SimilarityComputer::new()?,
            chunk_size,
            file_chunk_size,
            inflight_limit: inflight_limit.max(1),
            file_vectors: HashMap::new(),
            file_gpu_buffer: None,
        })
    }

    fn encode_ids(&self, ids: &[String]) -> Vec<f32> {
        let mut data = Vec::with_capacity(ids.len() * VECTOR_SIZE);
        for id in ids {
            data.extend(self.vectorizer.encode(id));
        }
        data
    }

    fn collect_matches(
        &self,
        hh_ids: &[String],
        files: &[(i64, String)],
        scores: &[f32],
        min_similarity: f64,
    ) -> Vec<MatchResult> {
        let mut results = Vec::new();
        let file_len = files.len();
        for (qi, hh_id) in hh_ids.iter().enumerate() {
            for (fi, file) in files.iter().enumerate() {
                let score = scores[qi * file_len + fi] as f64;
                if score >= min_similarity {
                    results.push(MatchResult {
                        hh_id: hh_id.clone(),
                        file_id: file.0,
                        similarity: score,
                    });
                }
            }
        }
        results
    }

    fn prepare_cache(&mut self, files: &[(i64, String)], db: &Database) -> Result<(), String> {
        let valid_ids: HashSet<i64> = files.iter().map(|(id, _)| *id).collect();
        self.file_vectors.retain(|id, _| valid_ids.contains(id));

        for (id, name) in files {
            if self.file_vectors.contains_key(id) {
                continue;
            }
            let fingerprint = fingerprint_entry(*id, name);
            if let Some(cached) = db
                .get_file_vector(*id, fingerprint)
                .map_err(|e| format!("Failed to read cached vector: {}", e))?
            {
                self.file_vectors.insert(*id, cached);
                continue;
            }
            let encoded = self.vectorizer.encode(name);
            db.upsert_file_vector(*id, fingerprint, &encoded)
                .map_err(|e| format!("Failed to persist vector: {}", e))?;
            self.file_vectors.insert(*id, encoded);
        }

        Ok(())
    }

    fn gather_cached_vectors(&mut self, files: &[(i64, String)]) -> Vec<f32> {
        let mut data = Vec::with_capacity(files.len() * VECTOR_SIZE);
        for (id, name) in files {
            if let Some(entry) = self.file_vectors.get(id) {
                data.extend_from_slice(entry);
            } else {
                log::warn!(
                    "Vector cache missing entry for file {} ({}). Recomputing on the fly.",
                    id,
                    name
                );
                let encoded = self.vectorizer.encode(name);
                data.extend_from_slice(&encoded);
                // Store the recomputed vector in cache to avoid recomputation
                self.file_vectors.insert(*id, encoded);
            }
        }
        data
    }

    fn ensure_gpu_buffer(
        &mut self,
        files: &[(i64, String)],
    ) -> Result<(Arc<Buffer>, usize), String> {
        // Create order-independent fingerprint by sorting files by ID
        let mut sorted_ids: Vec<(i64, &String)> =
            files.iter().map(|(id, name)| (*id, name)).collect();
        sorted_ids.sort_by_key(|(id, _)| *id);

        let mut hasher = DefaultHasher::new();
        files.len().hash(&mut hasher);
        for (id, name) in sorted_ids {
            id.hash(&mut hasher);
            name.hash(&mut hasher);
        }
        let fingerprint = hasher.finish();

        if let Some((buffer, count, hash)) = &self.file_gpu_buffer {
            if *count == files.len() && *hash == fingerprint {
                return Ok((Arc::clone(buffer), *count));
            }
        }

        let vectors = self.gather_cached_vectors(files);
        let buffer = self.computer.create_file_buffer(&vectors);
        self.file_gpu_buffer = Some((Arc::clone(&buffer), files.len(), fingerprint));
        Ok((buffer, files.len()))
    }

    fn file_chunk_size_for(&self, query_count: usize) -> usize {
        let base = self.file_chunk_size.max(1);
        if query_count == 0 {
            return base;
        }

        let dim = VECTOR_SIZE;
        let bytes_per_vector = (dim * std::mem::size_of::<f32>()) as u64;
        let max_storage = self.computer.max_storage_bytes().max(bytes_per_vector);

        let file_limit = max_storage / bytes_per_vector;
        let output_limit = if query_count == 0 {
            max_storage
        } else {
            max_storage / (query_count as u64 * std::mem::size_of::<f32>() as u64)
        };

        let adaptive = file_limit.min(output_limit).max(1);
        base.min(adaptive as usize).max(1)
    }

    fn finish_next_tile(
        &self,
        pending: &mut VecDeque<PendingTile<'_>>,
        all_matches: &mut Vec<MatchResult>,
        min_similarity: f64,
        tracker: &mut ProgressTracker,
        progress: Option<&MatchProgressCallback>,
    ) -> Result<(), String> {
        if let Some(tile) = pending.pop_front() {
            let scores = tile.handle.wait()?;
            let matches =
                self.collect_matches(tile.hh_slice, tile.file_slice, &scores, min_similarity);
            all_matches.extend(matches);
            tracker.tile_complete(tile.hh_slice.len(), tile.file_slice.len(), progress);
        }
        Ok(())
    }
}

struct PendingTile<'a> {
    hh_slice: &'a [String],
    file_slice: &'a [(i64, String)],
    handle: GpuTileHandle,
}

struct ProgressTracker {
    total_queries: usize,
    total_work: usize,
    completed_work: usize,
    total_tiles: usize,
    completed_tiles: usize,
}

impl ProgressTracker {
    fn new(total_queries: usize, total_files: usize) -> Self {
        Self {
            total_queries,
            total_work: total_queries.saturating_mul(total_files),
            completed_work: 0,
            total_tiles: 0,
            completed_tiles: 0,
        }
    }

    fn register_tile(&mut self, _query_count: usize, _file_count: usize) {
        self.total_tiles = self.total_tiles.saturating_add(1);
    }

    fn tile_complete(
        &mut self,
        query_count: usize,
        file_count: usize,
        progress: Option<&MatchProgressCallback>,
    ) {
        self.completed_tiles = self.completed_tiles.saturating_add(1);
        self.completed_work = self
            .completed_work
            .saturating_add(query_count.saturating_mul(file_count));
        self.emit(progress);
    }

    fn finish(&mut self, progress: Option<&MatchProgressCallback>) {
        self.completed_work = self.total_work;
        self.emit(progress);
    }

    fn emit(&self, progress: Option<&MatchProgressCallback>) {
        if let Some(callback) = progress {
            if let Ok(mut cb) = callback.lock() {
                let ratio = if self.total_work == 0 {
                    1.0
                } else {
                    (self.completed_work as f64 / self.total_work as f64).clamp(0.0, 1.0)
                };
                let mapped = (ratio * self.total_queries as f64).ceil() as usize;
                cb(mapped.min(self.total_queries), self.total_queries);
            }
        }
    }
}

fn fingerprint_entry(id: i64, name: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    id.hash(&mut hasher);
    name.hash(&mut hasher);
    hasher.finish()
}

impl MatchEngine for GpuMatchEngine {
    fn kind(&self) -> MatchEngineKind {
        MatchEngineKind::Gpu
    }

    fn match_and_store(
        &mut self,
        hh_ids: &[String],
        db: &mut Database,
        min_similarity: f64,
        progress_callback: Option<MatchProgressCallback>,
    ) -> Result<usize, String> {
        let files = db
            .get_all_files()
            .map_err(|e| format!("Failed to load files for GPU matcher: {}", e))?;

        if files.is_empty() {
            return Err("No files found in database. Please scan a directory first.".to_string());
        }

        let total_queries = hh_ids.len();
        let mut progress = progress_callback;

        if total_queries == 0 {
            if let Some(callback) = progress.as_ref() {
                if let Ok(mut cb) = callback.lock() {
                    cb(0, 0);
                }
            } else {
                info!("GPU matching completed immediately: no household IDs provided");
            }
            return Ok(0);
        }

        if progress.is_none() {
            progress = Some(make_logging_progress_callback(
                "GPU matching",
                "IDs",
                total_queries,
            ));
        }

        if let Some(ref callback) = progress {
            if let Ok(mut cb) = callback.lock() {
                cb(0, total_queries);
            }
        }

        let file_pairs: Vec<(i64, String)> = files
            .iter()
            .map(|record| (record.id, record.file_name.clone()))
            .collect();

        db.cleanup_orphan_vectors()
            .map_err(|e| format!("Failed to clean vector cache: {}", e))?;

        self.prepare_cache(&file_pairs, db)?;
        let total_files = file_pairs.len().max(1);
        let (file_buffer, _) = self.ensure_gpu_buffer(&file_pairs)?;

        let mut all_matches = Vec::new();
        let mut tracker = ProgressTracker::new(hh_ids.len(), total_files);
        let mut pending: VecDeque<PendingTile<'_>> = VecDeque::new();

        info!(
            "GPU matching started: processing {} household IDs across {} files",
            total_queries,
            file_pairs.len()
        );

        for chunk in hh_ids.chunks(self.chunk_size.max(1)) {
            if chunk.is_empty() {
                continue;
            }
            let chunk_vectors = self.encode_ids(chunk);
            let chunk_file_size = self.file_chunk_size_for(chunk.len());

            for (tile_index, file_chunk) in file_pairs.chunks(chunk_file_size).enumerate() {
                if file_chunk.is_empty() {
                    continue;
                }
                let file_offset = tile_index * chunk_file_size;
                let handle = self.computer.dispatch_tile(
                    &chunk_vectors,
                    chunk.len(),
                    &file_buffer,
                    file_offset,
                    file_chunk.len(),
                    VECTOR_SIZE,
                )?;

                tracker.register_tile(chunk.len(), file_chunk.len());
                pending.push_back(PendingTile {
                    hh_slice: chunk,
                    file_slice: file_chunk,
                    handle,
                });

                if pending.len() >= self.inflight_limit {
                    self.finish_next_tile(
                        &mut pending,
                        &mut all_matches,
                        min_similarity,
                        &mut tracker,
                        progress.as_ref(),
                    )?;
                }
            }
        }

        while !pending.is_empty() {
            self.finish_next_tile(
                &mut pending,
                &mut all_matches,
                min_similarity,
                &mut tracker,
                progress.as_ref(),
            )?;
        }

        tracker.finish(progress.as_ref());

        let mut session = db
            .start_match_import()
            .map_err(|e| format!("Failed to start GPU match transaction: {}", e))?;

        // Clear only matches for the hh_ids being processed (incremental update)
        session
            .clear_for_ids(hh_ids)
            .map_err(|e| format!("Failed to clear previous matches: {}", e))?;

        for result in &all_matches {
            session
                .insert_match(&result.hh_id, result.file_id, result.similarity)
                .map_err(|e| format!("Failed to store GPU match: {}", e))?;
        }

        session
            .commit()
            .map_err(|e| format!("Failed to commit GPU matches: {}", e))?;

        let total_matches = all_matches.len();
        info!(
            "GPU matching finished: stored {} matches for {} household IDs",
            total_matches, total_queries
        );

        Ok(total_matches)
    }
}
