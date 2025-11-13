use chrono::Utc;
use rusqlite::{params, Connection, Result, Transaction};

pub struct Database {
    conn: Connection,
}

pub struct FileImportSession<'conn> {
    tx: Transaction<'conn>,
}

impl<'conn> FileImportSession<'conn> {
    pub fn upsert_file(&mut self, file_path: &str, file_name: &str) -> Result<()> {
        let scan_date = Utc::now().to_rfc3339();
        let mut stmt = self.tx.prepare_cached(
            "INSERT INTO files (file_path, file_name, scan_date) VALUES (?1, ?2, ?3)
             ON CONFLICT(file_path) DO UPDATE SET file_name=excluded.file_name, scan_date=excluded.scan_date",
        )?;
        stmt.execute(params![file_path, file_name, scan_date])?;
        Ok(())
    }

    pub fn commit(self) -> Result<()> {
        self.tx.commit()
    }
}

#[derive(Debug, Clone)]
pub struct FileRecord {
    pub id: i64,
    pub file_path: String,
    pub file_name: String,
    pub scan_date: String,
}

#[derive(Debug, Clone)]
pub struct ReferenceId {
    pub id: i64,
    pub hh_id: String,
    pub import_date: String,
}

#[derive(Debug, Clone)]
pub struct MatchRecord {
    pub id: i64,
    pub hh_id: String,
    pub file_id: i64,
    pub file_name: String,
    pub file_path: String,
    pub similarity_score: f64,
    pub match_date: String,
}

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub file_name: String,
    pub file_path: String,
    pub similarity_score: f64,
}

pub struct ReferenceImportSession<'conn> {
    tx: Transaction<'conn>,
}

impl<'conn> ReferenceImportSession<'conn> {
    pub fn insert(&mut self, hh_id: &str) -> Result<bool> {
        let import_date = Utc::now().to_rfc3339();
        let mut stmt = self.tx.prepare_cached(
            "INSERT OR IGNORE INTO reference_ids (hh_id, import_date) VALUES (?1, ?2)",
        )?;
        let changed = stmt.execute(params![hh_id, import_date])?;
        Ok(changed > 0)
    }

    pub fn commit(self) -> Result<()> {
        self.tx.commit()
    }
}

impl Database {
    pub fn new(db_path: &str) -> Result<Self> {
        let conn = Connection::open(db_path)?;
        let db = Database { conn };
        db.create_tables()?;
        Ok(db)
    }

    fn create_tables(&self) -> Result<()> {
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS files (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                file_path TEXT NOT NULL UNIQUE,
                file_name TEXT NOT NULL,
                scan_date TEXT NOT NULL
            )",
            [],
        )?;

        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS reference_ids (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                hh_id TEXT NOT NULL UNIQUE,
                import_date TEXT NOT NULL
            )",
            [],
        )?;

        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS matches (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                hh_id TEXT NOT NULL,
                file_id INTEGER NOT NULL,
                similarity_score REAL NOT NULL,
                match_date TEXT NOT NULL,
                FOREIGN KEY (file_id) REFERENCES files(id)
            )",
            [],
        )?;

        // Create indices for better query performance
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_files_path ON files(file_path)",
            [],
        )?;

        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_reference_ids_hh_id ON reference_ids(hh_id)",
            [],
        )?;

        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_matches_hh_id ON matches(hh_id)",
            [],
        )?;

        Ok(())
    }

    pub fn insert_file(&self, file_path: &str, file_name: &str) -> Result<i64> {
        let scan_date = Utc::now().to_rfc3339();

        // Try to insert, if it exists, return the existing id
        match self.conn.execute(
            "INSERT INTO files (file_path, file_name, scan_date) VALUES (?1, ?2, ?3)",
            params![file_path, file_name, scan_date],
        ) {
            Ok(_) => Ok(self.conn.last_insert_rowid()),
            Err(rusqlite::Error::SqliteFailure(err, _))
                if err.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                // File already exists, get its id
                self.get_file_id(file_path)
            }
            Err(e) => Err(e),
        }
    }

    pub fn start_file_import(&mut self) -> Result<FileImportSession<'_>> {
        let tx = self.conn.transaction()?;
        Ok(FileImportSession { tx })
    }

    pub fn get_file_id(&self, file_path: &str) -> Result<i64> {
        self.conn.query_row(
            "SELECT id FROM files WHERE file_path = ?1",
            params![file_path],
            |row| row.get(0),
        )
    }

    pub fn insert_match(&self, hh_id: &str, file_id: i64, similarity_score: f64) -> Result<()> {
        let match_date = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO matches (hh_id, file_id, similarity_score, match_date) VALUES (?1, ?2, ?3, ?4)",
            params![hh_id, file_id, similarity_score, match_date],
        )?;
        Ok(())
    }

    pub fn get_all_files(&self) -> Result<Vec<FileRecord>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, file_path, file_name, scan_date FROM files ORDER BY file_name")?;

        let files = stmt.query_map([], |row| {
            Ok(FileRecord {
                id: row.get(0)?,
                file_path: row.get(1)?,
                file_name: row.get(2)?,
                scan_date: row.get(3)?,
            })
        })?;

        files.collect()
    }

    pub fn get_file_count(&self) -> Result<usize> {
        self.conn
            .query_row("SELECT COUNT(*) FROM files", [], |row| row.get(0))
    }

    pub fn get_all_matches(&self, min_similarity: f64) -> Result<Vec<MatchRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT m.id, m.hh_id, m.file_id, f.file_name, f.file_path, m.similarity_score, m.match_date
             FROM matches m
             JOIN files f ON m.file_id = f.id
             WHERE m.similarity_score >= ?1
             ORDER BY m.similarity_score DESC"
        )?;

        let matches = stmt.query_map(params![min_similarity], |row| {
            Ok(MatchRecord {
                id: row.get(0)?,
                hh_id: row.get(1)?,
                file_id: row.get(2)?,
                file_name: row.get(3)?,
                file_path: row.get(4)?,
                similarity_score: row.get(5)?,
                match_date: row.get(6)?,
            })
        })?;

        matches.collect()
    }

    pub fn clear_matches(&self) -> Result<()> {
        self.conn.execute("DELETE FROM matches", [])?;
        Ok(())
    }

    pub fn clear_matches_for_id(&self, hh_id: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM matches WHERE hh_id = ?1", params![hh_id])?;
        Ok(())
    }

    pub fn clear_files(&self) -> Result<()> {
        self.conn.execute("DELETE FROM files", [])?;
        self.conn.execute("DELETE FROM matches", [])?;
        Ok(())
    }

    // Reference ID management
    pub fn start_reference_import(&mut self) -> Result<ReferenceImportSession<'_>> {
        let tx = self.conn.transaction()?;
        Ok(ReferenceImportSession { tx })
    }

    pub fn insert_reference_id(&self, hh_id: &str) -> Result<bool> {
        let import_date = Utc::now().to_rfc3339();

        // Use INSERT OR IGNORE to skip duplicates and report whether a new row was added
        let changed = self.conn.execute(
            "INSERT OR IGNORE INTO reference_ids (hh_id, import_date) VALUES (?1, ?2)",
            params![hh_id, import_date],
        )?;
        Ok(changed > 0)
    }

    pub fn get_all_reference_ids(&self) -> Result<Vec<ReferenceId>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, hh_id, import_date FROM reference_ids ORDER BY hh_id")?;

        let ids = stmt.query_map([], |row| {
            Ok(ReferenceId {
                id: row.get(0)?,
                hh_id: row.get(1)?,
                import_date: row.get(2)?,
            })
        })?;

        ids.collect()
    }

    pub fn get_reference_id_count(&self) -> Result<usize> {
        self.conn
            .query_row("SELECT COUNT(*) FROM reference_ids", [], |row| row.get(0))
    }

    pub fn clear_reference_ids(&self) -> Result<()> {
        self.conn.execute("DELETE FROM reference_ids", [])?;
        Ok(())
    }

    // Search for a single household ID against all files
    pub fn search_single_id(&self, hh_id: &str, min_similarity: f64) -> Result<Vec<SearchResult>> {
        // This will be called from the matcher with fuzzy-matched results
        // For now, return matches from the matches table for this specific hh_id
        let mut stmt = self.conn.prepare(
            "SELECT f.file_name, f.file_path, m.similarity_score
             FROM matches m
             JOIN files f ON m.file_id = f.id
             WHERE m.hh_id = ?1 AND m.similarity_score >= ?2
             ORDER BY m.similarity_score DESC",
        )?;

        let results = stmt.query_map(params![hh_id, min_similarity], |row| {
            Ok(SearchResult {
                file_name: row.get(0)?,
                file_path: row.get(1)?,
                similarity_score: row.get(2)?,
            })
        })?;

        results.collect()
    }
}
