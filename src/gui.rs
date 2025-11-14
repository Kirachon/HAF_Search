use crate::database::{Database, SearchResult};
use crate::match_engine::{self, MatchEngineKind, MatchProgressCallback};
use crate::opener;
use crate::reference_loader::{ReferenceLoadReport, ReferenceLoader};
use crate::scanner::Scanner;
use crate::searcher::Searcher;
use eframe::egui;
use log::error;
use rfd::FileDialog;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;

#[derive(Debug, Clone, PartialEq)]
enum AppState {
    Idle,
    Scanning,
    LoadingReferenceIds,
    Matching,
    Searching,
}

// Messages sent from background threads to GUI
enum BackgroundMessage {
    ScanProgress {
        processed: usize,
        total: usize,
    },
    ScanComplete {
        discovered: usize,
        db_total: usize,
    },
    ScanError {
        error: String,
    },
    ReferenceIdsProgress {
        processed_rows: usize,
        bytes_read: u64,
        total_bytes: u64,
    },
    ReferenceIdsLoaded {
        report: ReferenceLoadReport,
        total: usize,
    },
    ReferenceIdsError {
        error: String,
    },
    MatchingProgress {
        processed: usize,
        total: usize,
    },
    MatchingComplete {
        match_count: usize,
        engine: MatchEngineKind,
    },
    MatchingError {
        error: String,
    },
    MatchingEngineNotice {
        message: String,
    },
    SearchComplete {
        results: Vec<SearchResult>,
        cache_error: Option<String>,
    },
    SearchError {
        error: String,
    },
}

pub struct TiffLocatorApp {
    // Paths
    folder_path: String,
    csv_path: String,
    cache_path: String,

    // Settings
    similarity_threshold: f64,
    use_gpu_matcher: bool,
    gpu_available: bool,

    // State
    state: AppState,
    progress: f64,
    progress_text: String,

    // Search
    search_input: String,
    search_results: Vec<SearchResult>,

    // Pagination for results
    results_page: usize,
    results_per_page: usize,

    // Database
    db: Option<Arc<Mutex<Database>>>,
    file_count: usize,

    // Status messages
    status_message: String,
    error_message: String,

    // Reference ID count and import details
    reference_id_count: usize,
    last_reference_report: Option<ReferenceLoadReport>,

    // Channel for background thread communication
    bg_receiver: Receiver<BackgroundMessage>,
    bg_sender: Sender<BackgroundMessage>,
}

impl Default for TiffLocatorApp {
    fn default() -> Self {
        let (bg_sender, bg_receiver) = mpsc::channel();
        let cache_path = "cache.db".to_string();

        let (db, reference_id_count, file_count, status_message, error_message) =
            match Database::new(&cache_path) {
                Ok(db) => {
                    let reference_id_count = db.get_reference_id_count().unwrap_or(0);
                    let file_count = db.get_all_files().map(|files| files.len()).unwrap_or(0);
                    (
                        Some(Arc::new(Mutex::new(db))),
                        reference_id_count,
                        file_count,
                        String::from("Ready"),
                        String::new(),
                    )
                }
                Err(e) => (
                    None,
                    0,
                    0,
                    String::from("Database unavailable"),
                    format!("Failed to initialize cache: {}", e),
                ),
            };

        Self {
            folder_path: String::new(),
            csv_path: String::new(),
            cache_path,
            similarity_threshold: 0.7,
            state: AppState::Idle,
            progress: 0.0,
            progress_text: String::new(),
            search_input: String::new(),
            search_results: Vec::new(),
            results_page: 0,
            results_per_page: 500,
            db,
            file_count,
            status_message,
            error_message,
            reference_id_count,
            last_reference_report: None,
            bg_receiver,
            bg_sender,
            use_gpu_matcher: false,
            gpu_available: true,
        }
    }
}

impl TiffLocatorApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Self::default()
    }

    fn db_handle(&self) -> Result<Arc<Mutex<Database>>, String> {
        self.db
            .as_ref()
            .cloned()
            .ok_or_else(|| "Database is unavailable. Check cache.db permissions.".to_string())
    }

    fn lock_db<'a>(db: &'a Arc<Mutex<Database>>) -> Result<MutexGuard<'a, Database>, String> {
        db.lock()
            .map_err(|e| format!("Database access error: {}", e))
    }

    fn select_folder(&mut self) {
        if let Some(path) = FileDialog::new().pick_folder() {
            self.folder_path = path.to_string_lossy().to_string();
            self.status_message = format!("Selected folder: {}", self.folder_path);
            self.error_message.clear();
        }
    }

    fn select_csv(&mut self) {
        if let Some(path) = FileDialog::new().add_filter("CSV", &["csv"]).pick_file() {
            self.csv_path = path.to_string_lossy().to_string();
            self.status_message = format!("Selected CSV: {}", self.csv_path);
            self.error_message.clear();
        }
    }

    fn load_reference_ids(&mut self) {
        if self.csv_path.is_empty() {
            self.error_message = "Please select a CSV file first".to_string();
            return;
        }

        if self.db.is_none() {
            self.error_message = "Database is unavailable. Check cache.db permissions.".to_string();
            return;
        }

        self.state = AppState::LoadingReferenceIds;
        self.progress = 0.0;
        self.progress_text = "Loading reference IDs...".to_string();
        self.error_message.clear();
        self.status_message.clear();
        self.last_reference_report = None;

        let csv_path = self.csv_path.clone();
        let cache_path = self.cache_path.clone();
        let sender = self.bg_sender.clone();

        thread::spawn(move || {
            let loader = ReferenceLoader::new();
            let mut db = match Database::new(&cache_path) {
                Ok(db) => db,
                Err(e) => {
                    let _ = sender.send(BackgroundMessage::ReferenceIdsError {
                        error: format!("Database access error while loading IDs: {}", e),
                    });
                    return;
                }
            };

            let progress_sender = sender.clone();
            let progress_callback =
                move |processed_rows: usize, bytes_read: u64, total_bytes: u64| {
                    let _ = progress_sender.send(BackgroundMessage::ReferenceIdsProgress {
                        processed_rows,
                        bytes_read,
                        total_bytes,
                    });
                };

            let load_result =
                loader.load_from_csv_with_progress(&csv_path, &mut db, Some(progress_callback));

            match load_result {
                Ok(report) => {
                    let total = db
                        .get_reference_id_count()
                        .map_err(|e| format!("Failed to refresh reference ID count: {}", e));

                    match total {
                        Ok(total) => {
                            let _ = sender
                                .send(BackgroundMessage::ReferenceIdsLoaded { report, total });
                        }
                        Err(e) => {
                            let _ = sender.send(BackgroundMessage::ReferenceIdsError { error: e });
                        }
                    }
                }
                Err(e) => {
                    let _ = sender.send(BackgroundMessage::ReferenceIdsError { error: e });
                }
            }
        });
    }

    fn start_scanning(&mut self) {
        if self.folder_path.is_empty() {
            self.error_message = "Please select a folder first".to_string();
            return;
        }

        if self.db.is_none() {
            self.error_message = "Database is unavailable. Check cache.db permissions.".to_string();
            return;
        }

        self.state = AppState::Scanning;
        self.progress = 0.0;
        self.progress_text = "Scanning...".to_string();
        self.error_message.clear();
        self.status_message.clear();

        let folder_path = self.folder_path.clone();
        let cache_path = self.cache_path.clone();
        let sender = self.bg_sender.clone();

        thread::spawn(move || {
            let mut scanner = Scanner::new();
            let progress_sender = sender.clone();
            scanner.set_progress_callback(move |processed, total| {
                let _ = progress_sender.send(BackgroundMessage::ScanProgress { processed, total });
            });

            let mut db = match Database::new(&cache_path) {
                Ok(db) => db,
                Err(e) => {
                    let _ = sender.send(BackgroundMessage::ScanError {
                        error: format!("Database access error while scanning: {}", e),
                    });
                    return;
                }
            };

            let result = match scanner.scan_and_store(&folder_path, &mut db) {
                Ok(report) => match db.get_file_count() {
                    Ok(total_files) => Ok((report, total_files)),
                    Err(e) => Err(format!("Failed to refresh cached file count: {}", e)),
                },
                Err(e) => Err(e),
            };

            match result {
                Ok((report, total_files)) => {
                    let _ = sender.send(BackgroundMessage::ScanComplete {
                        discovered: report.discovered,
                        db_total: total_files,
                    });
                }
                Err(e) => {
                    let _ = sender.send(BackgroundMessage::ScanError { error: e });
                }
            }
        });
    }

    fn search_household_id(&mut self) {
        let search_id = self.search_input.trim();

        if search_id.is_empty() {
            self.error_message = "Please enter a household ID to search".to_string();
            return;
        }

        if self.db.is_none() {
            self.error_message = "Database is unavailable. Check cache.db permissions.".to_string();
            return;
        }

        self.state = AppState::Searching;
        self.progress = 0.0;
        self.progress_text = format!("Searching for '{}'...", search_id);
        self.error_message.clear();
        self.status_message.clear();
        self.results_page = 0; // Reset pagination

        let search_id = search_id.to_string();
        let threshold = self.similarity_threshold;
        let sender = self.bg_sender.clone();
        let cache_path = self.cache_path.clone();

        thread::spawn(move || {
            let searcher = Searcher::new();
            let db = match Database::new(&cache_path) {
                Ok(db) => db,
                Err(e) => {
                    let _ = sender.send(BackgroundMessage::SearchError {
                        error: format!("Database access error while searching: {}", e),
                    });
                    return;
                }
            };

            let cached_results = match db.search_single_id(&search_id, threshold) {
                Ok(results) => results,
                Err(e) => {
                    let _ = sender.send(BackgroundMessage::SearchError {
                        error: format!("Failed to read cached matches: {}", e),
                    });
                    return;
                }
            };

            if !cached_results.is_empty() {
                let _ = sender.send(BackgroundMessage::SearchComplete {
                    results: cached_results,
                    cache_error: None,
                });
                return;
            }

            let results = match searcher.search_single_id(&search_id, &db, threshold) {
                Ok(results) => results,
                Err(e) => {
                    let _ = sender.send(BackgroundMessage::SearchError { error: e });
                    return;
                }
            };

            let cache_error = searcher.store_results(&search_id, &results, &db).err();

            let _ = sender.send(BackgroundMessage::SearchComplete {
                results,
                cache_error,
            });
        });
    }

    fn start_matching(&mut self) {
        if self.reference_id_count == 0 {
            self.error_message = "No reference IDs loaded. Please import a CSV first.".to_string();
            return;
        }

        if self.file_count == 0 {
            self.error_message = "No TIFF files have been scanned yet.".to_string();
            return;
        }

        if self.db.is_none() {
            self.error_message = "Database is unavailable. Check cache.db permissions.".to_string();
            return;
        }

        self.state = AppState::Matching;
        self.progress = 0.0;
        self.progress_text = "Matching household IDs...".to_string();
        self.error_message.clear();
        self.status_message.clear();

        let sender = self.bg_sender.clone();
        let cache_path = self.cache_path.clone();
        let threshold = self.similarity_threshold;
        let prefer_gpu = self.use_gpu_matcher && self.gpu_available;

        thread::spawn(move || {
            let mut db = match Database::new(&cache_path) {
                Ok(db) => db,
                Err(e) => {
                    let _ = sender.send(BackgroundMessage::MatchingError {
                        error: format!("Database access error while matching: {}", e),
                    });
                    return;
                }
            };

            let hh_ids = match db.get_all_reference_ids() {
                Ok(ids) => ids,
                Err(e) => {
                    let _ = sender.send(BackgroundMessage::MatchingError {
                        error: format!("Failed to read reference IDs: {}", e),
                    });
                    return;
                }
            };

            let desired_engine = if prefer_gpu {
                MatchEngineKind::Gpu
            } else {
                MatchEngineKind::Cpu
            };

            let mut fallback_notice = None;
            let mut engine = match match_engine::create_engine(desired_engine) {
                Ok(engine) => engine,
                Err(err) => {
                    if desired_engine == MatchEngineKind::Gpu {
                        fallback_notice = Some(format!(
                            "GPU matcher unavailable ({}). Falling back to CPU matcher.",
                            err
                        ));
                        match match_engine::create_engine(MatchEngineKind::Cpu) {
                            Ok(engine) => engine,
                            Err(cpu_err) => {
                                let _ = sender.send(BackgroundMessage::MatchingError {
                                    error: format!(
                                        "Failed to initialize CPU matcher after GPU fallback: {}",
                                        cpu_err
                                    ),
                                });
                                return;
                            }
                        }
                    } else {
                        let _ = sender.send(BackgroundMessage::MatchingError { error: err });
                        return;
                    }
                }
            };

            if let Some(message) = fallback_notice {
                let _ = sender.send(BackgroundMessage::MatchingEngineNotice { message });
            }

            let progress_sender = sender.clone();
            let progress_callback: MatchProgressCallback =
                Arc::new(Mutex::new(move |processed, total| {
                    let _ = progress_sender
                        .send(BackgroundMessage::MatchingProgress { processed, total });
                }));

            match engine.match_and_store(&hh_ids, &mut db, threshold, Some(progress_callback)) {
                Ok(count) => {
                    let _ = sender.send(BackgroundMessage::MatchingComplete {
                        match_count: count,
                        engine: engine.kind(),
                    });
                }
                Err(e) => {
                    let _ = sender.send(BackgroundMessage::MatchingError { error: e });
                }
            }
        });
    }

    fn export_to_csv(&mut self) {
        if self.search_results.is_empty() {
            return;
        }

        if let Some(path) = FileDialog::new()
            .set_file_name("search_results.csv")
            .add_filter("CSV", &["csv"])
            .save_file()
        {
            match self.write_results_to_csv(&path.to_string_lossy()) {
                Ok(_) => {
                    self.status_message = format!("Exported search results to {}", path.display());
                    self.error_message.clear();
                }
                Err(e) => {
                    self.error_message = format!("Export error: {}", e);
                    self.status_message.clear();
                }
            }
        }
    }

    fn write_results_to_csv(&self, path: &str) -> Result<(), String> {
        let mut writer =
            csv::Writer::from_path(path).map_err(|e| format!("Failed to create CSV: {}", e))?;

        // Write headers
        writer
            .write_record(["file_name", "file_path", "similarity"])
            .map_err(|e| format!("Failed to write headers: {}", e))?;

        // Write data
        for result in &self.search_results {
            writer
                .write_record([
                    &result.file_name,
                    &result.file_path,
                    &format!("{:.2}%", result.similarity_score * 100.0),
                ])
                .map_err(|e| format!("Failed to write record: {}", e))?;
        }

        writer
            .flush()
            .map_err(|e| format!("Failed to flush CSV: {}", e))?;

        Ok(())
    }

    fn clear_cache(&mut self) {
        let db = match self.db_handle() {
            Ok(db) => db,
            Err(err) => {
                self.error_message = err;
                return;
            }
        };

        let clear_result = {
            match Self::lock_db(&db) {
                Ok(db_guard) => db_guard
                    .clear_files()
                    .map_err(|e| format!("Failed to clear cache: {}", e)),
                Err(err) => Err(err),
            }
        };

        match clear_result {
            Ok(_) => {
                self.file_count = 0;
                self.search_results.clear();
                self.status_message = "Cache cleared successfully".to_string();
                self.error_message.clear();
            }
            Err(e) => {
                self.error_message = e;
                self.status_message.clear();
            }
        }
    }

    fn process_background_messages(&mut self, ctx: &egui::Context) {
        // Process all pending messages from background threads
        while let Ok(msg) = self.bg_receiver.try_recv() {
            match msg {
                BackgroundMessage::ScanProgress { processed, total } => {
                    if total > 0 {
                        self.progress = (processed as f64 / total as f64).min(1.0);
                    }
                    self.progress_text = format!("Scanning files... ({}/{})", processed, total);
                }
                BackgroundMessage::ScanComplete {
                    discovered,
                    db_total,
                } => {
                    self.state = AppState::Idle;
                    self.progress = 1.0;
                    self.status_message = format!(
                        "Scan complete: {} TIFF files found ({} cached total)",
                        discovered, db_total
                    );
                    self.file_count = db_total;
                    self.error_message.clear();
                }
                BackgroundMessage::ScanError { error } => {
                    self.state = AppState::Idle;
                    self.progress = 0.0;
                    self.error_message = format!("Scan error: {}", error);
                    self.status_message.clear();
                }
                BackgroundMessage::ReferenceIdsProgress {
                    processed_rows,
                    bytes_read,
                    total_bytes,
                } => {
                    let percent = if total_bytes > 0 {
                        (bytes_read as f64 / total_bytes as f64).min(1.0)
                    } else {
                        0.0
                    };
                    self.progress = percent;
                    self.progress_text = format!(
                        "Loading reference IDs... {} rows processed ({:.0}%)",
                        processed_rows,
                        percent * 100.0
                    );
                }
                BackgroundMessage::ReferenceIdsLoaded { report, total } => {
                    self.state = AppState::Idle;
                    self.progress = 1.0;
                    self.reference_id_count = total;
                    self.last_reference_report = Some(report.clone());
                    self.status_message = format!(
                        "Loaded {} reference IDs (processed {}, skipped {}). Database total: {}",
                        report.inserted, report.processed, report.skipped, total
                    );

                    if report.errors.is_empty() {
                        self.error_message.clear();
                    } else {
                        let preview: String = report
                            .errors
                            .iter()
                            .take(5)
                            .cloned()
                            .collect::<Vec<_>>()
                            .join("\n");
                        self.error_message = format!(
                            "{} rows failed to load. Sample errors:\n{}{}",
                            report.errors.len(),
                            preview,
                            if report.errors.len() > 5 { "\n..." } else { "" }
                        );
                    }
                }
                BackgroundMessage::ReferenceIdsError { error } => {
                    self.state = AppState::Idle;
                    self.progress = 0.0;
                    self.error_message = format!("Failed to load reference IDs: {}", error);
                    self.status_message.clear();
                }
                BackgroundMessage::MatchingProgress { processed, total } => {
                    if total > 0 {
                        self.progress = (processed as f64 / total as f64).min(1.0);
                    }
                    self.progress_text = format!("Matching IDs... ({}/{})", processed, total);
                }
                BackgroundMessage::MatchingComplete {
                    match_count,
                    engine,
                } => {
                    self.state = AppState::Idle;
                    self.progress = 1.0;
                    self.status_message = format!(
                        "Matching complete using {:?}: {} candidate matches stored",
                        engine, match_count
                    );
                    self.error_message.clear();
                }
                BackgroundMessage::MatchingEngineNotice { message } => {
                    self.status_message = message;
                    self.gpu_available = false;
                    self.use_gpu_matcher = false;
                }
                BackgroundMessage::MatchingError { error } => {
                    self.state = AppState::Idle;
                    self.progress = 0.0;
                    self.error_message = format!("Matching error: {}", error);
                    self.status_message.clear();
                }
                BackgroundMessage::SearchComplete {
                    results,
                    cache_error,
                } => {
                    self.state = AppState::Idle;
                    self.progress = 1.0;
                    self.search_results = results;
                    self.status_message = format!(
                        "Found {} matches for '{}'",
                        self.search_results.len(),
                        self.search_input.trim()
                    );
                    if let Some(err) = cache_error {
                        self.error_message =
                            format!("Search completed but failed to save cache: {}", err);
                    } else {
                        self.error_message.clear();
                    }
                    self.results_page = 0; // Reset to first page
                }
                BackgroundMessage::SearchError { error } => {
                    self.state = AppState::Idle;
                    self.progress = 0.0;
                    self.error_message = format!("Search error: {}", error);
                    self.status_message.clear();
                }
            }
            // Request repaint when we receive a message
            ctx.request_repaint();
        }
    }
}

impl eframe::App for TiffLocatorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Process messages from background threads
        self.process_background_messages(ctx);

        // Only request repaint if we're in an active state
        if self.state != AppState::Idle {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("🔍 TiffLocator");
            ui.add_space(10.0);

            // Folder selection
            ui.horizontal(|ui| {
                if ui.button("📁 Select Folder").clicked() {
                    self.select_folder();
                }
                ui.label(&self.folder_path);
                if self.file_count > 0 {
                    ui.label(format!("({} TIFF files cached)", self.file_count));
                }
            });

            ui.add_space(5.0);

            // CSV selection and reference ID loading
            ui.horizontal(|ui| {
                if ui.button("📄 Select CSV").clicked() {
                    self.select_csv();
                }
                ui.label(&self.csv_path);
            });

            ui.add_space(5.0);

            ui.horizontal(|ui| {
                let can_load =
                    self.state == AppState::Idle && !self.csv_path.is_empty() && self.db.is_some();
                if ui
                    .add_enabled(can_load, egui::Button::new("📥 Load Reference IDs"))
                    .clicked()
                {
                    self.load_reference_ids();
                }
                if self.reference_id_count > 0 {
                    ui.label(format!(
                        "({} reference IDs loaded)",
                        self.reference_id_count
                    ));
                }
            });

            if let Some(report) = &self.last_reference_report {
                ui.label(format!(
                    "Last import summary: processed {}, inserted {}, skipped {}",
                    report.processed, report.inserted, report.skipped
                ));
                if !report.errors.is_empty() {
                    ui.colored_label(
                        egui::Color32::YELLOW,
                        format!("{} rows reported issues", report.errors.len()),
                    );
                }
            }

            ui.add_space(10.0);
            ui.separator();
            ui.add_space(10.0);

            // Similarity threshold slider
            ui.horizontal(|ui| {
                ui.label("Similarity Threshold:");
                ui.add(egui::Slider::new(&mut self.similarity_threshold, 0.5..=1.0).text(""));
                ui.label(format!("{:.0}%", self.similarity_threshold * 100.0));
            });

            ui.horizontal(|ui| {
                let checkbox = egui::Checkbox::new(
                    &mut self.use_gpu_matcher,
                    "Use GPU matcher (experimental)",
                );
                let response = ui.add_enabled(self.gpu_available, checkbox);
                if !self.gpu_available {
                    ui.label(
                        egui::RichText::new("GPU support unavailable for this build").italics(),
                    );
                } else if response.changed() && self.use_gpu_matcher {
                    self.status_message =
                        "GPU matcher enabled. Results will match the CPU baseline.".to_string();
                }
            });

            ui.add_space(10.0);

            // Action buttons
            ui.horizontal(|ui| {
                let can_scan = self.state == AppState::Idle
                    && !self.folder_path.is_empty()
                    && self.db.is_some();
                if ui
                    .add_enabled(can_scan, egui::Button::new("🔍 Scan Directory"))
                    .clicked()
                {
                    self.start_scanning();
                }

                let can_match = self.state == AppState::Idle
                    && self.reference_id_count > 0
                    && self.file_count > 0
                    && self.db.is_some();
                if ui
                    .add_enabled(can_match, egui::Button::new("🔗 Match IDs"))
                    .clicked()
                {
                    self.start_matching();
                }

                if ui
                    .add_enabled(
                        !self.search_results.is_empty(),
                        egui::Button::new("📤 Export Results"),
                    )
                    .clicked()
                {
                    self.export_to_csv();
                }

                if ui
                    .add_enabled(
                        self.state == AppState::Idle && self.db.is_some(),
                        egui::Button::new("🗑 Clear Cache"),
                    )
                    .clicked()
                {
                    self.clear_cache();
                }
            });

            ui.add_space(10.0);
            ui.separator();
            ui.add_space(10.0);

            // Search section
            ui.heading("🔎 Search for Household ID");
            ui.add_space(5.0);

            ui.horizontal(|ui| {
                ui.label("Household ID:");
                ui.text_edit_singleline(&mut self.search_input);

                let can_search = self.state == AppState::Idle
                    && !self.search_input.trim().is_empty()
                    && self.db.is_some();
                if ui
                    .add_enabled(can_search, egui::Button::new("🔍 Search"))
                    .clicked()
                {
                    self.search_household_id();
                }
            });

            ui.add_space(10.0);

            // Progress bar
            if self.state != AppState::Idle {
                ui.label(&self.progress_text);
                ui.add(egui::ProgressBar::new(self.progress as f32).show_percentage());
                ui.add_space(5.0);
            }

            // Status messages
            if !self.status_message.is_empty() {
                ui.colored_label(egui::Color32::GREEN, &self.status_message);
            }
            if !self.error_message.is_empty() {
                ui.colored_label(egui::Color32::RED, &self.error_message);
            }

            ui.add_space(10.0);
            ui.separator();
            ui.add_space(10.0);

            // Search results table with pagination
            if !self.search_results.is_empty() {
                let total_results = self.search_results.len();
                let start_idx = self.results_page * self.results_per_page;
                let end_idx = (start_idx + self.results_per_page).min(total_results);
                let total_pages = total_results.div_ceil(self.results_per_page);

                ui.heading(format!("Search Results ({} matches)", total_results));

                // Pagination controls
                ui.horizontal(|ui| {
                    ui.label(format!("Page {} of {}", self.results_page + 1, total_pages));

                    if ui
                        .add_enabled(self.results_page > 0, egui::Button::new("◀ Previous"))
                        .clicked()
                    {
                        self.results_page = self.results_page.saturating_sub(1);
                    }

                    if ui
                        .add_enabled(
                            self.results_page < total_pages - 1,
                            egui::Button::new("Next ▶"),
                        )
                        .clicked()
                    {
                        self.results_page += 1;
                    }

                    ui.label(format!(
                        "Showing {}-{} of {}",
                        start_idx + 1,
                        end_idx,
                        total_results
                    ));
                });

                ui.add_space(5.0);

                egui::ScrollArea::vertical()
                    .max_height(400.0)
                    .show(ui, |ui| {
                        egui::Grid::new("results_grid")
                            .striped(true)
                            .spacing([10.0, 4.0])
                            .show(ui, |ui| {
                                // Headers
                                ui.label(egui::RichText::new("File Name").strong());
                                ui.label(egui::RichText::new("Similarity").strong());
                                ui.label(egui::RichText::new("Action").strong());
                                ui.end_row();

                                // Data rows - only render current page (NO CLONE!)
                                for result in &self.search_results[start_idx..end_idx] {
                                    ui.label(&result.file_name);
                                    ui.label(format!("{:.1}%", result.similarity_score * 100.0));

                                    let file_path = result.file_path.clone();
                                    if ui.button("📂 Open Location").clicked() {
                                        match opener::open_file_location(&file_path) {
                                            Ok(_) => {
                                                self.status_message = format!(
                                                    "Opened file location for {}",
                                                    result.file_name
                                                );
                                                self.error_message.clear();
                                            }
                                            Err(e) => {
                                                error!("Failed to open location: {}", e);
                                                self.error_message =
                                                    format!("Failed to open location: {}", e);
                                            }
                                        }
                                    }
                                    ui.end_row();
                                }
                            });
                    });
            } else {
                ui.label("Enter a household ID and click Search to find matching TIFF files.");
            }
        });
    }
}
