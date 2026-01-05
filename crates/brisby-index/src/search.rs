//! Search index for the index provider

use brisby_core::{IndexEntry, SearchResult};
use rusqlite::{params, Connection, Result};

/// Search index for the index provider
pub struct SearchIndex {
    conn: Connection,
}

impl SearchIndex {
    /// Open or create the search index database
    pub fn open(path: &std::path::Path) -> Result<Self> {
        let conn = Connection::open(path)?;

        // Create tables if they don't exist
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS entries (
                content_hash BLOB PRIMARY KEY,
                filename TEXT NOT NULL,
                keywords TEXT NOT NULL,
                size INTEGER NOT NULL,
                chunk_count INTEGER NOT NULL,
                published_at INTEGER NOT NULL,
                ttl INTEGER NOT NULL,
                nym_address TEXT NOT NULL
            );

            CREATE VIRTUAL TABLE IF NOT EXISTS entries_fts USING fts5(
                filename,
                keywords,
                content='entries',
                content_rowid='rowid'
            );

            CREATE TRIGGER IF NOT EXISTS entries_ai AFTER INSERT ON entries BEGIN
                INSERT INTO entries_fts(rowid, filename, keywords)
                VALUES (new.rowid, new.filename, new.keywords);
            END;

            CREATE TRIGGER IF NOT EXISTS entries_ad AFTER DELETE ON entries BEGIN
                INSERT INTO entries_fts(entries_fts, rowid, filename, keywords)
                VALUES ('delete', old.rowid, old.filename, old.keywords);
            END;

            CREATE TRIGGER IF NOT EXISTS entries_au AFTER UPDATE ON entries BEGIN
                INSERT INTO entries_fts(entries_fts, rowid, filename, keywords)
                VALUES ('delete', old.rowid, old.filename, old.keywords);
                INSERT INTO entries_fts(rowid, filename, keywords)
                VALUES (new.rowid, new.filename, new.keywords);
            END;

            CREATE INDEX IF NOT EXISTS idx_published_at ON entries(published_at);
            CREATE INDEX IF NOT EXISTS idx_ttl ON entries(ttl);
            "#,
        )?;

        Ok(Self { conn })
    }

    /// Add or update an entry in the index
    pub fn upsert(&self, entry: &IndexEntry, nym_address: &str) -> Result<()> {
        let keywords = entry.keywords.join(" ");

        self.conn.execute(
            r#"
            INSERT OR REPLACE INTO entries
            (content_hash, filename, keywords, size, chunk_count, published_at, ttl, nym_address)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#,
            params![
                entry.content_hash.as_slice(),
                entry.filename,
                keywords,
                entry.size as i64,
                entry.chunk_count as i64,
                entry.published_at as i64,
                entry.ttl as i64,
                nym_address,
            ],
        )?;

        Ok(())
    }

    /// Search for entries matching a query
    pub fn search(&self, query: &str, max_results: u32) -> Result<Vec<SearchResult>> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT e.content_hash, e.filename, e.size, e.chunk_count, bm25(entries_fts) as rank, e.nym_address
            FROM entries_fts fts
            JOIN entries e ON e.rowid = fts.rowid
            WHERE entries_fts MATCH ?
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
                let nym_address: String = row.get(5)?;

                Ok(SearchResult {
                    content_hash,
                    filename: row.get(1)?,
                    size: row.get::<_, i64>(2)? as u64,
                    chunk_count: row.get::<_, i64>(3)? as u32,
                    relevance: -row.get::<_, f64>(4)? as f32,
                    seeders: vec![nym_address],
                })
            })?
            .collect::<Result<Vec<_>>>()?;

        Ok(results)
    }

    /// Remove expired entries
    pub fn cleanup_expired(&self, current_time: u64) -> Result<usize> {
        let rows = self.conn.execute(
            "DELETE FROM entries WHERE published_at + ttl < ?",
            params![current_time as i64],
        )?;
        Ok(rows)
    }

    /// Get statistics about the index
    pub fn stats(&self) -> Result<IndexStats> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM entries", [], |row| row.get(0))?;

        let total_size: i64 = self
            .conn
            .query_row("SELECT COALESCE(SUM(size), 0) FROM entries", [], |row| {
                row.get(0)
            })?;

        Ok(IndexStats {
            entry_count: count as u64,
            total_size_bytes: total_size as u64,
        })
    }
}

/// Statistics about the search index
#[derive(Debug, Clone)]
pub struct IndexStats {
    pub entry_count: u64,
    pub total_size_bytes: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_upsert_and_search() {
        let temp = NamedTempFile::new().unwrap();
        let index = SearchIndex::open(temp.path()).unwrap();

        let entry = IndexEntry {
            content_hash: [1u8; 32],
            filename: "test_movie.mkv".to_string(),
            keywords: vec!["test".to_string(), "movie".to_string()],
            size: 1024 * 1024 * 100,
            chunk_count: 400,
            published_at: 1000,
            ttl: 3600,
        };

        index.upsert(&entry, "test-nym-address").unwrap();

        let results = index.search("movie", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].filename, "test_movie.mkv");
        assert_eq!(results[0].seeders, vec!["test-nym-address"]);
    }
}
