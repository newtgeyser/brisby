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
        // entries: file metadata (one row per file)
        // seeders: who has the file (multiple rows per file)
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS entries (
                content_hash BLOB PRIMARY KEY,
                filename TEXT NOT NULL,
                keywords TEXT NOT NULL,
                size INTEGER NOT NULL,
                chunk_count INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS seeders (
                content_hash BLOB NOT NULL,
                nym_address TEXT NOT NULL,
                published_at INTEGER NOT NULL,
                ttl INTEGER NOT NULL,
                PRIMARY KEY (content_hash, nym_address),
                FOREIGN KEY (content_hash) REFERENCES entries(content_hash) ON DELETE CASCADE
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

            CREATE INDEX IF NOT EXISTS idx_seeders_published_at ON seeders(published_at);
            CREATE INDEX IF NOT EXISTS idx_seeders_ttl ON seeders(ttl);
            "#,
        )?;

        Ok(Self { conn })
    }

    /// Add or update an entry in the index
    ///
    /// Inserts or updates the file metadata, and adds the seeder.
    /// Multiple seeders can publish the same file.
    pub fn upsert(&self, entry: &IndexEntry, nym_address: &str) -> Result<()> {
        let keywords = entry.keywords.join(" ");

        // Insert or update file metadata (using ON CONFLICT to avoid CASCADE delete)
        self.conn.execute(
            r#"
            INSERT INTO entries (content_hash, filename, keywords, size, chunk_count)
            VALUES (?, ?, ?, ?, ?)
            ON CONFLICT(content_hash) DO UPDATE SET
                filename = excluded.filename,
                keywords = excluded.keywords,
                size = excluded.size,
                chunk_count = excluded.chunk_count
            "#,
            params![
                entry.content_hash.as_slice(),
                entry.filename,
                keywords,
                entry.size as i64,
                entry.chunk_count as i64,
            ],
        )?;

        // Insert or update seeder info
        self.conn.execute(
            r#"
            INSERT INTO seeders (content_hash, nym_address, published_at, ttl)
            VALUES (?, ?, ?, ?)
            ON CONFLICT(content_hash, nym_address) DO UPDATE SET
                published_at = excluded.published_at,
                ttl = excluded.ttl
            "#,
            params![
                entry.content_hash.as_slice(),
                nym_address,
                entry.published_at as i64,
                entry.ttl as i64,
            ],
        )?;

        Ok(())
    }

    /// Search for entries matching a query
    ///
    /// Returns results with all known seeders aggregated for each file.
    pub fn search(&self, query: &str, max_results: u32) -> Result<Vec<SearchResult>> {
        // First get FTS matches with BM25 ranking, then join with seeders
        let mut stmt = self.conn.prepare(
            r#"
            SELECT
                e.content_hash,
                e.filename,
                e.size,
                e.chunk_count,
                fts_matches.rank,
                GROUP_CONCAT(s.nym_address) as seeders
            FROM (
                SELECT rowid, bm25(entries_fts) as rank
                FROM entries_fts
                WHERE entries_fts MATCH ?
                ORDER BY rank
                LIMIT ?
            ) fts_matches
            JOIN entries e ON e.rowid = fts_matches.rowid
            LEFT JOIN seeders s ON e.content_hash = s.content_hash
            GROUP BY e.content_hash
            ORDER BY fts_matches.rank
            "#,
        )?;

        let results = stmt
            .query_map(params![query, max_results], |row| {
                let hash_bytes: Vec<u8> = row.get(0)?;
                let mut content_hash = [0u8; 32];
                if hash_bytes.len() == 32 {
                    content_hash.copy_from_slice(&hash_bytes);
                }

                // Parse comma-separated seeder addresses
                let seeders_str: Option<String> = row.get(5)?;
                let seeders: Vec<String> = seeders_str
                    .map(|s| {
                        s.split(',')
                            .map(|addr| addr.trim().to_string())
                            .filter(|addr| !addr.is_empty())
                            .collect()
                    })
                    .unwrap_or_default();

                Ok(SearchResult {
                    content_hash,
                    filename: row.get(1)?,
                    size: row.get::<_, i64>(2)? as u64,
                    chunk_count: row.get::<_, i64>(3)? as u32,
                    relevance: -row.get::<_, f64>(4)? as f32,
                    seeders,
                })
            })?
            .collect::<Result<Vec<_>>>()?;

        Ok(results)
    }

    /// Remove expired seeders and orphaned entries
    ///
    /// First removes seeders whose TTL has expired, then removes any entries
    /// that no longer have any seeders.
    pub fn cleanup_expired(&self, current_time: u64) -> Result<usize> {
        // Delete expired seeders
        let expired_seeders = self.conn.execute(
            "DELETE FROM seeders WHERE published_at + ttl < ?",
            params![current_time as i64],
        )?;

        // Delete entries with no remaining seeders
        let orphaned_entries = self.conn.execute(
            "DELETE FROM entries WHERE content_hash NOT IN (SELECT DISTINCT content_hash FROM seeders)",
            [],
        )?;

        Ok(expired_seeders + orphaned_entries)
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

    #[test]
    fn test_multiple_seeders_aggregated() {
        let temp = NamedTempFile::new().unwrap();
        let index = SearchIndex::open(temp.path()).unwrap();

        // Same file published by two different seeders
        let entry = IndexEntry {
            content_hash: [2u8; 32],
            filename: "shared_file.txt".to_string(),
            keywords: vec!["shared".to_string()],
            size: 1024,
            chunk_count: 1,
            published_at: 1000,
            ttl: 3600,
        };

        // First seeder publishes
        index.upsert(&entry, "seeder-one").unwrap();
        // Second seeder publishes same file
        index.upsert(&entry, "seeder-two").unwrap();

        let results = index.search("shared", 10).unwrap();
        assert_eq!(results.len(), 1); // Should be deduplicated by content_hash
        assert_eq!(results[0].seeders.len(), 2);
        assert!(results[0].seeders.contains(&"seeder-one".to_string()));
        assert!(results[0].seeders.contains(&"seeder-two".to_string()));
    }
}
