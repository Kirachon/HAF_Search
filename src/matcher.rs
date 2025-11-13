use crate::database::{Database, FileRecord};
use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use rayon::prelude::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
pub struct MatchResult {
    pub hh_id: String,
    pub file_id: i64,
    pub file_name: String,
    pub file_path: String,
    pub similarity: f64,
}

#[derive(Clone)]
struct FileMatchContext {
    record: FileRecord,
    candidates: Vec<String>,
}

impl FileMatchContext {
    fn from_record(record: &FileRecord) -> Self {
        let mut candidates = Vec::with_capacity(3);
        candidates.push(record.file_name.to_lowercase());
        if let Some(stem) = Matcher::strip_tiff_suffix(&record.file_name) {
            candidates.push(stem.to_lowercase());
        }
        let extracted = Matcher::extract_id_from_filename(&record.file_name);
        if !extracted.is_empty() {
            candidates.push(extracted.to_lowercase());
        }

        FileMatchContext {
            record: record.clone(),
            candidates,
        }
    }
}

pub struct Matcher {
    progress_callback: Option<Arc<Mutex<dyn FnMut(usize, usize) + Send>>>,
}

impl Matcher {
    pub fn new() -> Self {
        Matcher {
            progress_callback: None,
        }
    }

    pub fn set_progress_callback<F>(&mut self, callback: F)
    where
        F: FnMut(usize, usize) + Send + 'static,
    {
        self.progress_callback = Some(Arc::new(Mutex::new(callback)));
    }

    /// Extract potential ID from filename by removing common prefixes/suffixes and extensions
    fn extract_id_from_filename(filename: &str) -> String {
        // Remove extension
        let name = filename
            .trim_end_matches(".tif")
            .trim_end_matches(".tiff")
            .trim_end_matches(".TIF")
            .trim_end_matches(".TIFF");

        // Remove common separators and extract alphanumeric parts
        name.replace(['_', '-', ' ', '.'], "")
    }

    /// Match household IDs against TIFF files
    pub fn match_ids(
        &self,
        hh_ids: &[String],
        files: &[FileRecord],
        min_similarity: f64,
    ) -> Vec<MatchResult> {
        let total = hh_ids.len();
        let processed = Arc::new(AtomicUsize::new(0));
        let progress_callback = self.progress_callback.clone();

        let file_contexts: Vec<FileMatchContext> = files
            .par_iter()
            .map(FileMatchContext::from_record)
            .collect();

        if file_contexts.is_empty() {
            return Vec::new();
        }

        // Perform matching in parallel
        let results: Vec<MatchResult> = hh_ids
            .par_chunks(32)
            .flat_map_iter(|chunk| {
                let matcher = SkimMatcherV2::default();
                let mut chunk_results = Vec::new();

                for hh_id in chunk {
                    let matches_for_id =
                        Self::match_single_id(&matcher, hh_id, &file_contexts, min_similarity);
                    chunk_results.extend(matches_for_id);
                }

                if let Some(ref callback) = progress_callback {
                    let completed =
                        processed.fetch_add(chunk.len(), Ordering::Relaxed) + chunk.len();
                    if let Ok(mut cb) = callback.lock() {
                        cb(completed.min(total), total);
                    }
                }

                chunk_results
            })
            .collect();

        results
    }

    /// Match IDs and store results in database
    pub fn match_and_store(
        &self,
        hh_ids: &[String],
        db: &mut Database,
        min_similarity: f64,
    ) -> Result<usize, String> {
        // Get all files from database
        let files = db
            .get_all_files()
            .map_err(|e| format!("Failed to get files from database: {}", e))?;

        if files.is_empty() {
            return Err("No files found in database. Please scan a directory first.".to_string());
        }

        // Perform matching
        let matches = self.match_ids(hh_ids, &files, min_similarity);
        let count = matches.len();

        let mut session = db
            .start_match_import()
            .map_err(|e| format!("Failed to start match transaction: {}", e))?;

        session
            .clear_all()
            .map_err(|e| format!("Failed to clear previous matches: {}", e))?;

        for match_result in matches {
            session
                .insert_match(
                    &match_result.hh_id,
                    match_result.file_id,
                    match_result.similarity,
                )
                .map_err(|e| format!("Failed to store match: {}", e))?;
        }

        session
            .commit()
            .map_err(|e| format!("Failed to commit matches: {}", e))?;

        Ok(count)
    }
}

impl Matcher {
    fn perfect_score(matcher: &SkimMatcherV2, query: &str) -> i64 {
        matcher
            .fuzzy_match(query, query)
            .unwrap_or((query.len().max(1) as i64) * 10)
            .max(1)
    }

    fn normalize_score(score: i64, candidate: &str, query: &str, perfect_score: i64) -> f64 {
        if score <= 0 || perfect_score <= 0 {
            return 0.0;
        }

        let base = (score as f64 / perfect_score as f64).min(1.0);
        let candidate_len = candidate.chars().count();
        let query_len = query.chars().count();
        if candidate_len == 0 || query_len == 0 {
            return 0.0;
        }
        let len_ratio =
            (candidate_len.min(query_len) as f64) / (candidate_len.max(query_len) as f64);
        (base * len_ratio).min(1.0)
    }

    fn match_single_id(
        matcher: &SkimMatcherV2,
        hh_id: &str,
        files: &[FileMatchContext],
        min_similarity: f64,
    ) -> Vec<MatchResult> {
        let mut results = Vec::new();
        let trimmed = hh_id.trim();
        if trimmed.is_empty() {
            return results;
        }

        let needle = trimmed.to_lowercase();
        let perfect_score = Self::perfect_score(matcher, &needle);

        for context in files {
            let mut best = 0.0;
            for candidate in &context.candidates {
                let score_forward = matcher.fuzzy_match(candidate, &needle).unwrap_or(0);
                let score_reverse = matcher.fuzzy_match(&needle, candidate).unwrap_or(0);
                let raw_score = score_forward.max(score_reverse);
                let normalized =
                    Self::normalize_score(raw_score, candidate, &needle, perfect_score);
                if normalized > best {
                    best = normalized;
                }
                if best >= min_similarity {
                    break;
                }
            }

            if best >= min_similarity {
                results.push(MatchResult {
                    hh_id: hh_id.to_string(),
                    file_id: context.record.id,
                    file_name: context.record.file_name.clone(),
                    file_path: context.record.file_path.clone(),
                    similarity: best,
                });
            }
        }

        results
    }

    fn strip_tiff_suffix(name: &str) -> Option<&str> {
        name.strip_suffix(".tif")
            .or_else(|| name.strip_suffix(".tiff"))
            .or_else(|| name.strip_suffix(".TIF"))
            .or_else(|| name.strip_suffix(".TIFF"))
    }
}

impl Default for Matcher {
    fn default() -> Self {
        Self::new()
    }
}
