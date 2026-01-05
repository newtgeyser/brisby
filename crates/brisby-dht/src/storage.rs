//! DHT storage for content hash to seeder mappings

use brisby_core::{ContentHash, Seeder};
use std::collections::HashMap;

/// Storage for DHT entries
pub struct DhtStorage {
    /// Map from content hash to list of seeders
    entries: HashMap<ContentHash, Vec<Seeder>>,
    /// Maximum seeders per content hash
    max_seeders_per_key: usize,
}

impl DhtStorage {
    pub fn new(max_seeders_per_key: usize) -> Self {
        Self {
            entries: HashMap::new(),
            max_seeders_per_key,
        }
    }

    /// Store a seeder for a content hash
    pub fn store(&mut self, key: ContentHash, seeder: Seeder) {
        let seeders = self.entries.entry(key).or_insert_with(Vec::new);

        // Check if seeder already exists (by nym_address)
        if let Some(existing) = seeders.iter_mut().find(|s| s.nym_address == seeder.nym_address) {
            // Update existing entry
            *existing = seeder;
            return;
        }

        // Add new seeder if space available
        if seeders.len() < self.max_seeders_per_key {
            seeders.push(seeder);
        } else {
            // Replace oldest entry
            seeders.sort_by_key(|s| s.last_seen);
            if let Some(oldest) = seeders.first_mut() {
                if oldest.last_seen < seeder.last_seen {
                    *oldest = seeder;
                }
            }
        }
    }

    /// Get seeders for a content hash
    pub fn get(&self, key: &ContentHash) -> Option<&Vec<Seeder>> {
        self.entries.get(key)
    }

    /// Remove stale entries older than the given timestamp
    pub fn cleanup(&mut self, min_timestamp: u64) {
        for seeders in self.entries.values_mut() {
            seeders.retain(|s| s.last_seen >= min_timestamp);
        }
        self.entries.retain(|_, v| !v.is_empty());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_store_and_get() {
        let mut storage = DhtStorage::new(10);
        let key = [1u8; 32];
        let seeder = Seeder {
            nym_address: "test-address".to_string(),
            chunk_bitmap: vec![0xff],
            last_seen: 1000,
        };

        storage.store(key, seeder.clone());

        let seeders = storage.get(&key).unwrap();
        assert_eq!(seeders.len(), 1);
        assert_eq!(seeders[0].nym_address, "test-address");
    }
}
