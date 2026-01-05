//! Local file index using SQLite FTS5

use brisby_core::{ContentHash, FileMetadata, SearchResult};
use rusqlite::{params, Connection, Result};

/// Local index for shared files
pub struct LocalIndex {
    conn: Connection,
}

impl LocalIndex {
    /// Open or create the local index database
    pub fn open(path: &std::path::Path) -> Result<Self> {
        let conn = Connection::open(path)?;

        // Create tables if they don't exist
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS files (
                content_hash BLOB PRIMARY KEY,
                filename TEXT NOT NULL,
                size INTEGER NOT NULL,
                mime_type TEXT,
                chunk_count INTEGER NOT NULL,
                keywords TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                metadata_json TEXT NOT NULL
            );

            CREATE VIRTUAL TABLE IF NOT EXISTS files_fts USING fts5(
                filename,
                keywords,
                content='files',
                content_rowid='rowid'
            );

            CREATE TRIGGER IF NOT EXISTS files_ai AFTER INSERT ON files BEGIN
                INSERT INTO files_fts(rowid, filename, keywords)
                VALUES (new.rowid, new.filename, new.keywords);
            END;

            CREATE TRIGGER IF NOT EXISTS files_ad AFTER DELETE ON files BEGIN
                INSERT INTO files_fts(files_fts, rowid, filename, keywords)
                VALUES ('delete', old.rowid, old.filename, old.keywords);
            END;

            CREATE TRIGGER IF NOT EXISTS files_au AFTER UPDATE ON files BEGIN
                INSERT INTO files_fts(files_fts, rowid, filename, keywords)
                VALUES ('delete', old.rowid, old.filename, old.keywords);
                INSERT INTO files_fts(rowid, filename, keywords)
                VALUES (new.rowid, new.filename, new.keywords);
            END;
            "#,
        )?;

        Ok(Self { conn })
    }

    /// Add a file to the index
    pub fn add(&self, metadata: &FileMetadata) -> Result<()> {
        let keywords = metadata.keywords.join(" ");
        let metadata_json = serde_json::to_string(metadata).unwrap_or_default();

        self.conn.execute(
            r#"
            INSERT OR REPLACE INTO files
            (content_hash, filename, size, mime_type, chunk_count, keywords, created_at, metadata_json)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#,
            params![
                metadata.content_hash.as_slice(),
                metadata.filename,
                metadata.size as i64,
                metadata.mime_type,
                metadata.chunks.len() as i64,
                keywords,
                metadata.created_at as i64,
                metadata_json,
            ],
        )?;

        Ok(())
    }

    /// Search for files matching a query
    pub fn search(&self, query: &str, max_results: u32) -> Result<Vec<SearchResult>> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT f.content_hash, f.filename, f.size, f.chunk_count, bm25(files_fts) as rank
            FROM files_fts fts
            JOIN files f ON f.rowid = fts.rowid
            WHERE files_fts MATCH ?
            ORDER BY rank
            LIMIT ?
            "#,
        )?;

        let results = stmt
            .query_map(params![query, max_results], |row| {
                let hash_bytes: Vec<u8> = row.get(0)?;
                let mut content_hash = [0u8; 32];
                if hash_bytes.len() == 32 {
                    content_hash.copy_from_slice(&hash_bytes);
                }

                Ok(SearchResult {
                    content_hash,
                    filename: row.get(1)?,
                    size: row.get::<_, i64>(2)? as u64,
                    chunk_count: row.get::<_, i64>(3)? as u32,
                    relevance: -row.get::<_, f64>(4)? as f32, // bm25 returns negative scores
                    seeders: vec![], // Local index doesn't track seeders
                })
            })?
            .collect::<Result<Vec<_>>>()?;

        Ok(results)
    }

    /// Get a file by its content hash
    pub fn get(&self, content_hash: &ContentHash) -> Result<Option<FileMetadata>> {
        let mut stmt = self
            .conn
            .prepare("SELECT metadata_json FROM files WHERE content_hash = ?")?;

        let result: Option<String> = stmt
            .query_row(params![content_hash.as_slice()], |row| row.get(0))
            .ok();

        Ok(result.and_then(|json| serde_json::from_str(&json).ok()))
    }

    /// Remove a file from the index
    pub fn remove(&self, content_hash: &ContentHash) -> Result<bool> {
        let rows = self.conn.execute(
            "DELETE FROM files WHERE content_hash = ?",
            params![content_hash.as_slice()],
        )?;
        Ok(rows > 0)
    }

    /// List all files in the index
    pub fn list(&self) -> Result<Vec<FileMetadata>> {
        let mut stmt = self.conn.prepare("SELECT metadata_json FROM files")?;

        let results = stmt
            .query_map([], |row| {
                let json: String = row.get(0)?;
                Ok(json)
            })?
            .filter_map(|r| r.ok())
            .filter_map(|json| serde_json::from_str(&json).ok())
            .collect();

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    fn create_test_metadata() -> FileMetadata {
        FileMetadata {
            content_hash: [1u8; 32],
            filename: "test_file.txt".to_string(),
            size: 1024,
            mime_type: Some("text/plain".to_string()),
            chunks: vec![brisby_core::ChunkInfo {
                index: 0,
                hash: [2u8; 32],
                size: 1024,
            }],
            keywords: vec!["test".to_string(), "file".to_string()],
            created_at: 1000,
        }
    }

    #[test]
    fn test_add_and_get() {
        let temp = NamedTempFile::new().unwrap();
        let index = LocalIndex::open(temp.path()).unwrap();

        let metadata = create_test_metadata();
        index.add(&metadata).unwrap();

        let retrieved = index.get(&metadata.content_hash).unwrap().unwrap();
        assert_eq!(retrieved.filename, metadata.filename);
        assert_eq!(retrieved.size, metadata.size);
    }

    #[test]
    fn test_search() {
        let temp = NamedTempFile::new().unwrap();
        let index = LocalIndex::open(temp.path()).unwrap();

        let metadata = create_test_metadata();
        index.add(&metadata).unwrap();

        let results = index.search("test", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].filename, "test_file.txt");
    }
}
