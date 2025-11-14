use crate::database::Database;
use csv::ReaderBuilder;
use log::info;
use std::fs;
use std::fs::File;

#[derive(Debug, Clone)]
pub struct ReferenceLoadReport {
    pub processed: usize,
    pub inserted: usize,
    pub skipped: usize,
    pub errors: Vec<String>,
}

pub struct ReferenceLoader;

impl ReferenceLoader {
    pub fn new() -> Self {
        ReferenceLoader
    }

    /// Load household IDs from CSV file into the database
    /// Expects a CSV with a column named "hh_id"
    pub fn load_from_csv_with_progress<F>(
        &self,
        csv_path: &str,
        db: &mut Database,
        mut progress_callback: Option<F>,
    ) -> Result<ReferenceLoadReport, String>
    where
        F: FnMut(usize, u64, u64),
    {
        let metadata =
            fs::metadata(csv_path).map_err(|e| format!("Failed to read CSV metadata: {}", e))?;
        let total_bytes = metadata.len().max(1);

        info!(
            "Starting CSV import from '{}' ({} bytes)",
            csv_path,
            metadata.len()
        );

        let file = File::open(csv_path).map_err(|e| format!("Failed to open CSV file: {}", e))?;

        let mut reader = ReaderBuilder::new().has_headers(true).from_reader(file);

        // Get headers to find the hh_id column
        let headers = reader
            .headers()
            .map_err(|e| format!("Failed to read CSV headers: {}", e))?;

        let hh_id_index = headers
            .iter()
            .position(|h| h.trim().eq_ignore_ascii_case("hh_id"))
            .ok_or_else(|| "CSV file must contain a 'hh_id' column".to_string())?;

        let mut processed = 0;
        let mut inserted = 0;
        let mut skipped = 0;
        let mut errors = Vec::new();

        let mut record = csv::StringRecord::new();
        let mut line_index = 0usize;
        let mut import_session = db
            .start_reference_import()
            .map_err(|e| format!("Failed to start reference ID transaction: {}", e))?;

        let mut last_logged_percent = 0usize;

        loop {
            match reader.read_record(&mut record) {
                Ok(true) => {
                    processed += 1;
                    let display_line = line_index + 2;

                    if let Some(raw_hh_id) = record.get(hh_id_index) {
                        let hh_id = raw_hh_id.trim();
                        if hh_id.is_empty() {
                            skipped += 1;
                            errors.push(format!("Line {}: Empty hh_id value", display_line));
                        } else {
                            match import_session.insert(hh_id) {
                                Ok(true) => inserted += 1,
                                Ok(false) => skipped += 1,
                                Err(e) => {
                                    skipped += 1;
                                    errors.push(format!("Line {}: {}", display_line, e));
                                }
                            }
                        }
                    } else {
                        skipped += 1;
                        errors.push(format!("Line {}: Missing hh_id column", display_line));
                    }

                    line_index += 1;
                }
                Ok(false) => break,
                Err(e) => {
                    processed += 1;
                    let display_line = line_index + 2;
                    skipped += 1;
                    errors.push(format!("Line {}: {}", display_line, e));
                    line_index += 1;
                }
            }

            let bytes_read = reader.position().byte();
            if let Some(cb) = progress_callback.as_mut() {
                cb(processed, bytes_read, total_bytes);
            } else {
                let percent = ((bytes_read as f64 / total_bytes as f64) * 100.0)
                    .round()
                    .clamp(0.0, 100.0) as usize;
                if percent >= last_logged_percent.saturating_add(5)
                    || (percent == 100 && percent != last_logged_percent)
                {
                    info!(
                        "CSV import progress: {}% ({} rows processed)",
                        percent, processed
                    );
                    last_logged_percent = percent;
                }
            }
        }

        if processed == 0 {
            drop(import_session);
            return Err("CSV file did not contain any records".to_string());
        }

        import_session
            .commit()
            .map_err(|e| format!("Failed to commit reference IDs: {}", e))?;

        info!(
            "CSV import complete: processed {} rows (inserted {}, skipped {})",
            processed, inserted, skipped
        );

        Ok(ReferenceLoadReport {
            processed,
            inserted,
            skipped,
            errors,
        })
    }
}
