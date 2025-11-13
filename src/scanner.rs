use crate::database::Database;
use log::{info, warn};
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use walkdir::WalkDir;

#[derive(Debug, Clone)]
pub struct TiffFile {
    pub path: PathBuf,
    pub name: String,
}

pub struct Scanner {
    progress_callback: Option<Arc<Mutex<dyn FnMut(usize, usize) + Send>>>,
}

#[derive(Debug, Clone)]
pub struct ScanReport {
    pub discovered: usize,
}

impl Scanner {
    pub fn new() -> Self {
        Scanner {
            progress_callback: None,
        }
    }

    pub fn set_progress_callback<F>(&mut self, callback: F)
    where
        F: FnMut(usize, usize) + Send + 'static,
    {
        self.progress_callback = Some(Arc::new(Mutex::new(callback)));
    }

    /// Scan directory for TIFF files
    pub fn scan_directory(&self, dir_path: &str) -> Result<Vec<TiffFile>, String> {
        let path = Path::new(dir_path);

        if !path.exists() {
            return Err(format!("Directory does not exist: {}", dir_path));
        }

        info!("Starting filesystem walk at {}", path.display());

        // First pass: collect all files under the directory
        let entries: Vec<_> = WalkDir::new(path)
            .follow_links(true)
            .into_iter()
            .filter_map(|entry| match entry {
                Ok(e) => {
                    if e.file_type().is_file() {
                        Some(e.into_path())
                    } else {
                        None
                    }
                }
                Err(err) => {
                    warn!("WalkDir error while scanning {}: {}", dir_path, err);
                    None
                }
            })
            .collect();

        let total = entries.len();
        let processed = Arc::new(AtomicUsize::new(0));

        // Second pass: filter TIFF files in parallel
        let tiff_files: Vec<TiffFile> = entries
            .par_iter()
            .filter_map(|entry| {
                let path = entry.as_path();

                // Check if file has .tif or .tiff extension
                if let Some(ext) = path.extension() {
                    let ext_str = ext.to_string_lossy().to_lowercase();
                    if ext_str == "tif" || ext_str == "tiff" {
                        let name = path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_string();

                        Self::report_progress(&self.progress_callback, &processed, total);

                        return Some(TiffFile {
                            path: path.to_path_buf(),
                            name,
                        });
                    }
                }

                Self::report_progress(&self.progress_callback, &processed, total);

                None
            })
            .collect();

        info!(
            "Completed filesystem walk for {}. Found {} TIFF files ({} total files visited).",
            dir_path,
            tiff_files.len(),
            total
        );

        Ok(tiff_files)
    }

    /// Scan directory and store results in database
    pub fn scan_and_store(&self, dir_path: &str, db: &mut Database) -> Result<ScanReport, String> {
        let tiff_files = self.scan_directory(dir_path)?;
        let count = tiff_files.len();

        let mut session = db
            .start_file_import()
            .map_err(|e| format!("Failed to start file import transaction: {}", e))?;

        // Store files in database
        for file in &tiff_files {
            let path_str = file.path.to_string_lossy().to_string();
            session
                .upsert_file(&path_str, &file.name)
                .map_err(|e| format!("Database error storing {}: {}", file.name, e))?;
        }

        session
            .commit()
            .map_err(|e| format!("Failed to commit file import: {}", e))?;

        info!(
            "Persisted {} TIFF files from {} into cache database.",
            count, dir_path
        );

        Ok(ScanReport { discovered: count })
    }
}

impl Scanner {
    fn report_progress(
        callback: &Option<Arc<Mutex<dyn FnMut(usize, usize) + Send>>>,
        processed: &Arc<AtomicUsize>,
        total: usize,
    ) {
        if let Some(ref cb_handle) = callback {
            let current = processed.fetch_add(1, Ordering::Relaxed) + 1;
            if total == 0 {
                if let Ok(mut cb) = cb_handle.lock() {
                    cb(0, 0);
                }
                return;
            }

            let step = (total / 100).max(1);
            if current % step == 0 || current == total {
                if let Ok(mut cb) = cb_handle.lock() {
                    cb(current.min(total), total);
                }
            }
        }
    }
}

impl Default for Scanner {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_scanner_creation() {
        let scanner = Scanner::new();
        assert!(scanner.progress_callback.is_none());
    }

    #[test]
    fn test_scan_finds_test_data_files() {
        let scanner = Scanner::new();
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let data_dir = manifest_dir.join("test_data").join("tiff_files");
        let files = scanner
            .scan_directory(data_dir.to_str().expect("valid test data path"))
            .expect("scanner should succeed on test data");
        assert_eq!(files.len(), 15);
    }
}
