use crate::database::{Database, FileRecord};
use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use log::debug;
use rayon::prelude::*;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
pub struct MatchResult {
    pub hh_id: String,
    pub file_id: i64,
    pub file_name: String,
    pub file_path: String,
    pub similarity: f64,
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

    /// Calculate similarity between two strings using fuzzy matching
    fn calculate_similarity(matcher: &SkimMatcherV2, hh_id: &str, filename: &str) -> f64 {
        let trimmed = hh_id.trim();
        if trimmed.is_empty() {
            return 0.0;
        }

        let needle = trimmed.to_lowercase();
        let perfect_score = Self::perfect_score(matcher, &needle);

        let mut best = 0.0;
        let mut candidates = Vec::with_capacity(2);
        candidates.push(filename.to_lowercase());
        let extracted_id = Self::extract_id_from_filename(filename);
        if !extracted_id.is_empty() {
            candidates.push(extracted_id.to_lowercase());
        }

        for candidate in candidates.into_iter().filter(|c| !c.is_empty()) {
            let score_forward = matcher.fuzzy_match(&candidate, &needle).unwrap_or(0);
            let score_reverse = matcher.fuzzy_match(&needle, &candidate).unwrap_or(0);
            let raw_score = score_forward.max(score_reverse);
            let normalized = Self::normalize_score(raw_score, &candidate, &needle, perfect_score);

            debug!(
                "Matcher score for '{}' vs '{}': raw={}, normalized={:.3}",
                hh_id, candidate, raw_score, normalized
            );

            if normalized > best {
                best = normalized;
            }
        }

        best
    }

    /// Match household IDs against TIFF files
    pub fn match_ids(
        &self,
        hh_ids: &[String],
        files: &[FileRecord],
        min_similarity: f64,
    ) -> Vec<MatchResult> {
        let total = hh_ids.len();
        let processed = Arc::new(Mutex::new(0usize));
        let progress_callback = self.progress_callback.clone();

        // Perform matching in parallel
        let results: Vec<MatchResult> = hh_ids
            .par_iter()
            .flat_map(|hh_id| {
                let matcher = SkimMatcherV2::default();
                let mut matches = Vec::new();

                // Find best match for this hh_id
                for file in files {
                    let similarity = Self::calculate_similarity(&matcher, hh_id, &file.file_name);

                    if similarity >= min_similarity {
                        matches.push(MatchResult {
                            hh_id: hh_id.clone(),
                            file_id: file.id,
                            file_name: file.file_name.clone(),
                            file_path: file.file_path.clone(),
                            similarity,
                        });
                    }
                }

                // Update progress
                if let Some(ref callback) = progress_callback {
                    let mut count = processed.lock().unwrap();
                    *count += 1;
                    if let Ok(mut cb) = callback.lock() {
                        cb(*count, total);
                    }
                }

                matches
            })
            .collect();

        results
    }

    /// Match IDs and store results in database
    pub fn match_and_store(
        &self,
        hh_ids: &[String],
        db: &Database,
        min_similarity: f64,
    ) -> Result<usize, String> {
        // Get all files from database
        let files = db
            .get_all_files()
            .map_err(|e| format!("Failed to get files from database: {}", e))?;

        if files.is_empty() {
            return Err("No files found in database. Please scan a directory first.".to_string());
        }

        // Clear previous matches
        db.clear_matches()
            .map_err(|e| format!("Failed to clear previous matches: {}", e))?;

        // Perform matching
        let matches = self.match_ids(hh_ids, &files, min_similarity);
        let count = matches.len();

        // Store matches in database
        for match_result in matches {
            db.insert_match(
                &match_result.hh_id,
                match_result.file_id,
                match_result.similarity,
            )
            .map_err(|e| format!("Failed to store match: {}", e))?;
        }

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
}

impl Default for Matcher {
    fn default() -> Self {
        Self::new()
    }
}
