//! History database (SQLite).
//!
//! Two-phase commit, ring buffer undo, pruning, corruption recovery.
//! See §7 of the technical spec.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};
use zeroize::Zeroize;

use crate::error::CcvvError;
use crate::transforms::ContentType;

/// Maximum number of committed entries to keep.
const MAX_HISTORY_ENTRIES: usize = 50;

/// Maximum ring buffer size for undo.
const MAX_UNDO_ENTRIES: usize = 10;

/// An undo entry that zeroes its memory on drop.
#[derive(Debug, Clone)]
pub struct UndoEntry {
    pub raw_text: String,
    pub cleaned_text: String,
    pub raw_hash: String,
}

impl Drop for UndoEntry {
    fn drop(&mut self) {
        self.raw_text.zeroize();
        self.cleaned_text.zeroize();
        self.raw_hash.zeroize();
    }
}

/// History entry returned from queries.
#[derive(Debug, Clone)]
pub struct HistoryEntry {
    pub id: i64,
    pub raw_hash: String,
    pub raw_text: Option<String>,
    pub cleaned_text: String,
    pub content_type: Option<String>,
    pub preview: String,
    pub committed: bool,
    pub created_at: i64,
}

/// History database backed by SQLite.
pub struct HistoryDb {
    conn: Mutex<Connection>,
    undo_buffer: Mutex<VecDeque<UndoEntry>>,
    #[allow(dead_code)]
    db_path: PathBuf,
}

impl HistoryDb {
    /// Open or create the history database.
    pub fn open(path: &Path) -> Result<Self, CcvvError> {
        // If the database file exists and is corrupt, rename and start fresh
        if path.exists()
            && Connection::open(path)
                .and_then(|conn| {
                    conn.execute_batch("SELECT count(*) FROM sqlite_master")?;
                    Ok(conn)
                })
                .is_err()
        {
            let backup = path.with_extension("db.corrupt");
            std::fs::rename(path, &backup).map_err(|e| {
                CcvvError::DatabaseCorrupt(format!("Failed to rename corrupt DB: {}", e))
            })?;
        }

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(path).map_err(|e| CcvvError::Database(e.to_string()))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            let _ = std::fs::set_permissions(path, perms);
        }

        // Configure pragmas
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA busy_timeout=5000;
             PRAGMA synchronous=NORMAL;",
        )
        .map_err(|e| CcvvError::Database(e.to_string()))?;

        // Create schema
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                raw_hash TEXT NOT NULL,
                raw_text TEXT,
                cleaned_text TEXT NOT NULL,
                content_type TEXT,
                preview TEXT NOT NULL DEFAULT '',
                committed INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
            );
            CREATE INDEX IF NOT EXISTS idx_history_raw_hash ON history(raw_hash);
            CREATE INDEX IF NOT EXISTS idx_history_committed ON history(committed);
            CREATE INDEX IF NOT EXISTS idx_history_created_at ON history(created_at);",
        )
        .map_err(|e| CcvvError::Database(e.to_string()))?;

        let db = HistoryDb {
            conn: Mutex::new(conn),
            undo_buffer: Mutex::new(VecDeque::with_capacity(MAX_UNDO_ENTRIES)),
            db_path: path.to_path_buf(),
        };

        // Clean up any uncommitted entries from crashes
        db.cleanup_uncommitted()?;

        Ok(db)
    }

    /// Two-phase commit: prepare (committed=0).
    /// Returns the entry ID.
    pub fn prepare(
        &self,
        raw_text: &str,
        cleaned_text: &str,
        content_type: Option<ContentType>,
        store_raw: bool,
    ) -> Result<i64, CcvvError> {
        let raw_hash = compute_hash(raw_text);
        let preview = make_preview(cleaned_text);
        let ct_str = content_type.map(|ct| format!("{:?}", ct));
        let raw_store = if store_raw {
            Some(raw_text.to_string())
        } else {
            None
        };

        let conn = self
            .conn
            .lock()
            .map_err(|e| CcvvError::Database(format!("Lock error: {}", e)))?;

        conn.execute(
            "INSERT INTO history (raw_hash, raw_text, cleaned_text, content_type, preview, committed)
             VALUES (?1, ?2, ?3, ?4, ?5, 0)",
            params![raw_hash, raw_store, cleaned_text, ct_str, preview],
        )
        .map_err(|e| CcvvError::Database(e.to_string()))?;

        let id = conn.last_insert_rowid();

        // Add to undo buffer
        let mut buffer = self
            .undo_buffer
            .lock()
            .map_err(|e| CcvvError::Database(format!("Lock error: {}", e)))?;
        if buffer.len() >= MAX_UNDO_ENTRIES {
            buffer.pop_front();
        }
        buffer.push_back(UndoEntry {
            raw_text: raw_text.to_string(),
            cleaned_text: cleaned_text.to_string(),
            raw_hash,
        });

        Ok(id)
    }

    /// Two-phase commit: commit (committed=1).
    pub fn commit_entry(&self, id: i64) -> Result<(), CcvvError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| CcvvError::Database(format!("Lock error: {}", e)))?;

        conn.execute(
            "UPDATE history SET committed = 1 WHERE id = ?1",
            params![id],
        )
        .map_err(|e| CcvvError::Database(e.to_string()))?;

        // Prune old entries
        self.prune_entries(&conn)?;

        Ok(())
    }

    /// Rollback an uncommitted entry.
    pub fn rollback_entry(&self, id: i64) -> Result<(), CcvvError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| CcvvError::Database(format!("Lock error: {}", e)))?;

        conn.execute(
            "DELETE FROM history WHERE id = ?1 AND committed = 0",
            params![id],
        )
        .map_err(|e| CcvvError::Database(e.to_string()))?;

        Ok(())
    }

    /// Clean up uncommitted entries (from crashes).
    pub fn cleanup_uncommitted(&self) -> Result<(), CcvvError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| CcvvError::Database(format!("Lock error: {}", e)))?;

        conn.execute("DELETE FROM history WHERE committed = 0", [])
            .map_err(|e| CcvvError::Database(e.to_string()))?;

        Ok(())
    }

    /// Get the raw text for undo (from ring buffer).
    pub fn undo_raw(&self) -> Option<String> {
        let mut buffer = self.undo_buffer.lock().ok()?;
        buffer.pop_back().map(|entry| entry.raw_text.clone())
    }

    /// Get recent history entries.
    pub fn recent(&self, limit: usize) -> Result<Vec<HistoryEntry>, CcvvError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| CcvvError::Database(format!("Lock error: {}", e)))?;

        let mut stmt = conn
            .prepare(
                "SELECT id, raw_hash, raw_text, cleaned_text, content_type, preview, committed, created_at
                 FROM history
                 WHERE committed = 1
                 ORDER BY created_at DESC
                 LIMIT ?1",
            )
            .map_err(|e| CcvvError::Database(e.to_string()))?;

        let entries = stmt
            .query_map(params![limit as i64], |row| {
                Ok(HistoryEntry {
                    id: row.get(0)?,
                    raw_hash: row.get(1)?,
                    raw_text: row.get(2)?,
                    cleaned_text: row.get(3)?,
                    content_type: row.get(4)?,
                    preview: row.get(5)?,
                    committed: row.get::<_, i32>(6)? == 1,
                    created_at: row.get::<_, i64>(7)?,
                })
            })
            .map_err(|e| CcvvError::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| CcvvError::Database(e.to_string()))?;

        Ok(entries)
    }

    /// Search history by cleaned text content.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<HistoryEntry>, CcvvError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| CcvvError::Database(format!("Lock error: {}", e)))?;

        let mut stmt = conn
            .prepare(
                "SELECT id, raw_hash, raw_text, cleaned_text, content_type, preview, committed, created_at
                 FROM history
                 WHERE committed = 1 AND cleaned_text LIKE ?1 ESCAPE '\\'
                 ORDER BY created_at DESC
                 LIMIT ?2",
            )
            .map_err(|e| CcvvError::Database(e.to_string()))?;

        let pattern = format!("%{}%", escape_like(query));
        let entries = stmt
            .query_map(params![pattern, limit as i64], |row| {
                Ok(HistoryEntry {
                    id: row.get(0)?,
                    raw_hash: row.get(1)?,
                    raw_text: row.get(2)?,
                    cleaned_text: row.get(3)?,
                    content_type: row.get(4)?,
                    preview: row.get(5)?,
                    committed: row.get::<_, i32>(6)? == 1,
                    created_at: row.get::<_, i64>(7)?,
                })
            })
            .map_err(|e| CcvvError::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| CcvvError::Database(e.to_string()))?;

        Ok(entries)
    }

    /// Filter history by content type.
    pub fn by_type(
        &self,
        content_type: &str,
        limit: usize,
    ) -> Result<Vec<HistoryEntry>, CcvvError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| CcvvError::Database(format!("Lock error: {}", e)))?;

        let mut stmt = conn
            .prepare(
                "SELECT id, raw_hash, raw_text, cleaned_text, content_type, preview, committed, created_at
                 FROM history
                 WHERE committed = 1 AND content_type = ?1
                 ORDER BY created_at DESC
                 LIMIT ?2",
            )
            .map_err(|e| CcvvError::Database(e.to_string()))?;

        let entries = stmt
            .query_map(params![content_type, limit as i64], |row| {
                Ok(HistoryEntry {
                    id: row.get(0)?,
                    raw_hash: row.get(1)?,
                    raw_text: row.get(2)?,
                    cleaned_text: row.get(3)?,
                    content_type: row.get(4)?,
                    preview: row.get(5)?,
                    committed: row.get::<_, i32>(6)? == 1,
                    created_at: row.get::<_, i64>(7)?,
                })
            })
            .map_err(|e| CcvvError::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| CcvvError::Database(e.to_string()))?;

        Ok(entries)
    }

    /// Prune old entries, keeping only the last MAX_HISTORY_ENTRIES.
    fn prune_entries(&self, conn: &Connection) -> Result<(), CcvvError> {
        conn.execute(
            "DELETE FROM history WHERE committed = 1 AND id NOT IN (
                SELECT id FROM history WHERE committed = 1
                ORDER BY created_at DESC
                LIMIT ?1
            )",
            params![MAX_HISTORY_ENTRIES as i64],
        )
        .map_err(|e| CcvvError::Database(e.to_string()))?;

        Ok(())
    }
}

/// Escape LIKE wildcards for safe use in SQL LIKE patterns.
fn escape_like(query: &str) -> String {
    query
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

/// Compute SHA-256 hash of text content.
fn compute_hash(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Create a short preview of text (first 80 chars, single line).
fn make_preview(text: &str) -> String {
    let single_line = text.replace('\n', " ");
    if single_line.chars().count() <= 80 {
        single_line
    } else {
        let prefix: String = single_line.chars().take(77).collect();
        format!("{}...", prefix)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_db_path() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join("ccvv-test");
        std::fs::create_dir_all(&dir).unwrap();
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        dir.join(format!(
            "test-{}-{}-{}.db",
            std::process::id(),
            id,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn test_open_creates_db() {
        let path = temp_db_path();
        let _db = HistoryDb::open(&path).unwrap();
        assert!(path.exists());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_two_phase_commit() {
        let path = temp_db_path();
        let db = HistoryDb::open(&path).unwrap();

        let id = db.prepare("raw text", "cleaned text", None, false).unwrap();
        db.commit_entry(id).unwrap();

        let recent = db.recent(10).unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].cleaned_text, "cleaned text");
        assert!(recent[0].committed);

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_rollback() {
        let path = temp_db_path();
        let db = HistoryDb::open(&path).unwrap();

        let id = db.prepare("raw", "cleaned", None, false).unwrap();
        db.rollback_entry(id).unwrap();

        let recent = db.recent(10).unwrap();
        assert_eq!(recent.len(), 0);

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_undo_buffer() {
        let path = temp_db_path();
        let db = HistoryDb::open(&path).unwrap();

        db.prepare("raw1", "cleaned1", None, false).unwrap();
        db.prepare("raw2", "cleaned2", None, false).unwrap();

        let undone = db.undo_raw().unwrap();
        assert_eq!(undone, "raw2");

        let undone2 = db.undo_raw().unwrap();
        assert_eq!(undone2, "raw1");

        assert!(db.undo_raw().is_none());

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_search() {
        let path = temp_db_path();
        let db = HistoryDb::open(&path).unwrap();

        let id1 = db.prepare("raw1", "hello world", None, false).unwrap();
        db.commit_entry(id1).unwrap();
        let id2 = db.prepare("raw2", "goodbye world", None, false).unwrap();
        db.commit_entry(id2).unwrap();

        let results = db.search("hello", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].cleaned_text.contains("hello"));

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_pruning() {
        let path = temp_db_path();
        let db = HistoryDb::open(&path).unwrap();

        // Insert more than MAX_HISTORY_ENTRIES
        for i in 0..60 {
            let id = db
                .prepare(&format!("raw{}", i), &format!("cleaned{}", i), None, false)
                .unwrap();
            db.commit_entry(id).unwrap();
        }

        let recent = db.recent(100).unwrap();
        assert!(recent.len() <= MAX_HISTORY_ENTRIES);

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_raw_text_storage() {
        let path = temp_db_path();
        let db = HistoryDb::open(&path).unwrap();

        let id = db.prepare("raw text", "cleaned", None, true).unwrap();
        db.commit_entry(id).unwrap();

        let recent = db.recent(10).unwrap();
        assert_eq!(recent[0].raw_text.as_deref(), Some("raw text"));

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_raw_text_not_stored_by_default() {
        let path = temp_db_path();
        let db = HistoryDb::open(&path).unwrap();

        let id = db.prepare("raw text", "cleaned", None, false).unwrap();
        db.commit_entry(id).unwrap();

        let recent = db.recent(10).unwrap();
        assert!(recent[0].raw_text.is_none());

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_search_escapes_like_wildcards() {
        let path = temp_db_path();
        let db = HistoryDb::open(&path).unwrap();

        let id1 = db.prepare("raw1", "100% complete", None, false).unwrap();
        db.commit_entry(id1).unwrap();
        let id2 = db.prepare("raw2", "user_name is set", None, false).unwrap();
        db.commit_entry(id2).unwrap();
        let id3 = db.prepare("raw3", "unrelated text", None, false).unwrap();
        db.commit_entry(id3).unwrap();

        // Searching for literal "%" should only match the entry containing "%"
        let results = db.search("%", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].cleaned_text.contains("%"));

        // Searching for literal "_" should only match the entry containing "_"
        let results = db.search("_", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].cleaned_text.contains("_"));

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_make_preview_handles_unicode_safely() {
        let text =
            "Still applies — you're capped at max_containers=3. Worth reviewing as you grow. Extra";
        let preview = make_preview(text);
        assert!(
            preview.ends_with("..."),
            "preview should truncate with ellipsis"
        );
        assert!(
            preview.chars().count() <= 80,
            "preview should be at most 80 chars"
        );
    }
}
