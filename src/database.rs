use bytemuck::cast_slice;
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension, Result, Transaction};

pub struct Database {
    conn: Connection,
}

pub struct FileImportSession<'conn> {
    tx: Transaction<'conn>,
}

pub struct MatchImportSession<'conn> {
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

impl<'conn> MatchImportSession<'conn> {
    /// Clear all matches in the database (use with caution - prefer clear_for_ids for incremental updates)
    #[allow(dead_code)]
    pub fn clear_all(&mut self) -> Result<()> {
        self.tx.execute("DELETE FROM matches", [])?;
        Ok(())
    }

    pub fn clear_for_ids(&mut self, hh_ids: &[String]) -> Result<()> {
        if hh_ids.is_empty() {
            return Ok(());
        }

        // Build placeholders for the IN clause
        let placeholders = hh_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let query = format!("DELETE FROM matches WHERE hh_id IN ({})", placeholders);

        // Convert hh_ids to params
        let params: Vec<&dyn rusqlite::ToSql> =
            hh_ids.iter().map(|s| s as &dyn rusqlite::ToSql).collect();

        self.tx.execute(&query, params.as_slice())?;
        Ok(())
    }

    pub fn insert_match(&mut self, hh_id: &str, file_id: i64, similarity_score: f64) -> Result<()> {
        let match_date = Utc::now().to_rfc3339();
        self.tx.execute(
            "INSERT INTO matches (hh_id, file_id, similarity_score, match_date) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(hh_id, file_id) DO UPDATE SET similarity_score=excluded.similarity_score, match_date=excluded.match_date",
            params![hh_id, file_id, similarity_score, match_date],
        )?;
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

        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS file_vectors (
                file_id INTEGER PRIMARY KEY,
                fingerprint INTEGER NOT NULL,
                vector_blob BLOB NOT NULL,
                updated_at TEXT NOT NULL,
                FOREIGN KEY(file_id) REFERENCES files(id) ON DELETE CASCADE
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

        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_matches_file_id ON matches(file_id)",
            [],
        )?;

        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_matches_hh_similarity ON matches(hh_id, similarity_score DESC)",
            [],
        )?;

        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_file_vectors_fingerprint ON file_vectors(fingerprint)",
            [],
        )?;

        // Add unique constraint to prevent duplicate matches
        self.conn.execute(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_matches_unique ON matches(hh_id, file_id)",
            [],
        )?;

        Ok(())
    }

    pub fn start_file_import(&mut self) -> Result<FileImportSession<'_>> {
        let tx = self.conn.transaction()?;
        Ok(FileImportSession { tx })
    }

    pub fn start_match_import(&mut self) -> Result<MatchImportSession<'_>> {
        let tx = self.conn.transaction()?;
        Ok(MatchImportSession { tx })
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
            "INSERT INTO matches (hh_id, file_id, similarity_score, match_date) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(hh_id, file_id) DO UPDATE SET similarity_score=excluded.similarity_score, match_date=excluded.match_date",
            params![hh_id, file_id, similarity_score, match_date],
        )?;
        Ok(())
    }

    pub fn get_all_files(&self) -> Result<Vec<FileRecord>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, file_path, file_name FROM files ORDER BY file_name")?;

        let files = stmt.query_map([], |row| {
            Ok(FileRecord {
                id: row.get(0)?,
                file_path: row.get(1)?,
                file_name: row.get(2)?,
            })
        })?;

        files.collect()
    }

    pub fn get_file_count(&self) -> Result<usize> {
        self.conn
            .query_row("SELECT COUNT(*) FROM files", [], |row| row.get(0))
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

    pub fn get_all_reference_ids(&self) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT hh_id FROM reference_ids ORDER BY hh_id")?;

        let ids = stmt.query_map([], |row| row.get(0))?;

        ids.collect()
    }

    pub fn get_reference_id_count(&self) -> Result<usize> {
        self.conn
            .query_row("SELECT COUNT(*) FROM reference_ids", [], |row| row.get(0))
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

    pub fn get_file_vector(&self, file_id: i64, fingerprint: u64) -> Result<Option<Vec<f32>>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT fingerprint, vector_blob FROM file_vectors WHERE file_id = ?1",
        )?;
        let row = stmt
            .query_row(params![file_id], |row| {
                let stored: i64 = row.get(0)?;
                let blob: Vec<u8> = row.get(1)?;
                Ok((stored as u64, blob))
            })
            .optional()?;

        if let Some((stored_fingerprint, blob)) = row {
            if stored_fingerprint == fingerprint {
                if blob.len() % std::mem::size_of::<f32>() != 0 {
                    return Ok(None);
                }
                let floats = cast_slice::<u8, f32>(&blob).to_vec();
                return Ok(Some(floats));
            }
        }

        Ok(None)
    }

    pub fn upsert_file_vector(&self, file_id: i64, fingerprint: u64, data: &[f32]) -> Result<()> {
        let blob = cast_slice(data);
        self.conn.execute(
            "INSERT INTO file_vectors (file_id, fingerprint, vector_blob, updated_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(file_id) DO UPDATE SET
                 fingerprint=excluded.fingerprint,
                 vector_blob=excluded.vector_blob,
                 updated_at=excluded.updated_at",
            params![file_id, fingerprint as i64, blob, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn cleanup_orphan_vectors(&self) -> Result<()> {
        self.conn.execute(
            "DELETE FROM file_vectors WHERE file_id NOT IN (SELECT id FROM files)",
            [],
        )?;
        Ok(())
    }
}
