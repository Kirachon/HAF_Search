use crate::database::{Database, SearchResult};
use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use log::debug;
use rayon::prelude::*;

pub struct Searcher {
    matcher: SkimMatcherV2,
}

impl Searcher {
    pub fn new() -> Self {
        Searcher {
            matcher: SkimMatcherV2::default(),
        }
    }

    /// Search for a single household ID against all TIFF files in the database
    /// Returns results sorted by similarity score (highest first)
    pub fn search_single_id(
        &self,
        hh_id: &str,
        db: &Database,
        min_similarity: f64,
    ) -> Result<Vec<SearchResult>, String> {
        // Get all files from database
        let files = db
            .get_all_files()
            .map_err(|e| format!("Failed to get files from database: {}", e))?;

        if files.is_empty() {
            return Ok(Vec::new());
        }

        let needle = hh_id.to_lowercase();
        let perfect_score = Self::perfect_score(&self.matcher, &needle);
        let mut results: Vec<SearchResult> = files
            .par_iter()
            .filter_map(|file| {
                let file_name_lower = file.file_name.to_lowercase();

                if let Some(score) = self.matcher.fuzzy_match(&file_name_lower, &needle) {
                    let normalized_score =
                        Self::normalize_score(score, &file_name_lower, &needle, perfect_score);
                    if normalized_score >= min_similarity {
                        return Some(SearchResult {
                            file_name: file.file_name.clone(),
                            file_path: file.file_path.clone(),
                            similarity_score: normalized_score,
                        });
                    }
                }

                if let Some(stem) = Self::strip_tiff_suffix(&file.file_name) {
                    let stem_lower = stem.to_lowercase();
                    if let Some(score) = self.matcher.fuzzy_match(&stem_lower, &needle) {
                        let normalized_score =
                            Self::normalize_score(score, &stem_lower, &needle, perfect_score);
                        if normalized_score >= min_similarity {
                            return Some(SearchResult {
                                file_name: file.file_name.clone(),
                                file_path: file.file_path.clone(),
                                similarity_score: normalized_score,
                            });
                        }
                    }
                }

                None
            })
            .collect();

        // Sort by similarity score (highest first)
        results.sort_by(|a, b| {
            b.similarity_score
                .partial_cmp(&a.similarity_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        Ok(results)
    }

    /// Store search results in the database (optional - for caching)
    pub fn store_results(
        &self,
        hh_id: &str,
        results: &[SearchResult],
        db: &Database,
    ) -> Result<(), String> {
        db.clear_matches_for_id(hh_id)
            .map_err(|e| format!("Failed to clear previous matches: {}", e))?;

        for result in results {
            let file_id = db
                .get_file_id(&result.file_path)
                .map_err(|e| format!("Failed to fetch file id for {}: {}", result.file_path, e))?;

            db.insert_match(hh_id, file_id, result.similarity_score)
                .map_err(|e| format!("Failed to persist match for {}: {}", hh_id, e))?;
        }

        Ok(())
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
        let normalized = (base * len_ratio).min(1.0);

        debug!(
            "Searcher score '{}' vs '{}': raw={}, base={:.3}, len_ratio={:.3}, normalized={:.3}",
            query, candidate, score, base, len_ratio, normalized
        );

        normalized
    }

    fn perfect_score(matcher: &SkimMatcherV2, query: &str) -> i64 {
        matcher
            .fuzzy_match(query, query)
            .unwrap_or((query.len().max(1) as i64) * 10)
            .max(1)
    }

    fn strip_tiff_suffix(name: &str) -> Option<&str> {
        name.strip_suffix(".tif")
            .or_else(|| name.strip_suffix(".tiff"))
            .or_else(|| name.strip_suffix(".TIF"))
            .or_else(|| name.strip_suffix(".TIFF"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn longer_candidates_get_penalized() {
        let matcher = SkimMatcherV2::default();
        let query = "HH001".to_lowercase();
        let perfect = Searcher::perfect_score(&matcher, &query);
        let exact_score = matcher.fuzzy_match(&query, &query).unwrap();
        let exact_norm = Searcher::normalize_score(exact_score, &query, &query, perfect);
        assert!((exact_norm - 1.0).abs() < f64::EPSILON);

        let suffix_candidate = "HH001_document".to_lowercase();
        let suffix_score = matcher
            .fuzzy_match(&suffix_candidate, &query)
            .expect("suffix score");
        let suffix_norm =
            Searcher::normalize_score(suffix_score, &suffix_candidate, &query, perfect);
        assert!(suffix_norm < 1.0);
        assert!(suffix_norm > 0.2);

        let prefix_candidate = "document_HH001".to_lowercase();
        let prefix_score = matcher
            .fuzzy_match(&prefix_candidate, &query)
            .expect("prefix score");
        let prefix_norm =
            Searcher::normalize_score(prefix_score, &prefix_candidate, &query, perfect);
        assert!(prefix_norm < 1.0);
        assert!(prefix_norm > 0.2);
    }
}
