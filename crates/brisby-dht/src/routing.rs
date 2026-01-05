//! Kademlia routing table implementation

use brisby_core::ContentHash;
use std::collections::VecDeque;

/// XOR distance between two node IDs
pub fn xor_distance(a: &ContentHash, b: &ContentHash) -> ContentHash {
    let mut result = [0u8; 32];
    for i in 0..32 {
        result[i] = a[i] ^ b[i];
    }
    result
}

/// Find the index of the most significant bit that differs
pub fn bucket_index(distance: &ContentHash) -> usize {
    for (i, byte) in distance.iter().enumerate() {
        if *byte != 0 {
            let leading = byte.leading_zeros() as usize;
            return 255 - (i * 8 + leading);
        }
    }
    0
}

/// Information about a node in the routing table
#[derive(Debug, Clone)]
pub struct NodeInfo {
    pub node_id: ContentHash,
    pub nym_address: String,
    pub last_seen: u64,
}

/// A k-bucket in the routing table
#[derive(Debug, Clone)]
pub struct KBucket {
    /// Maximum number of nodes in this bucket
    k: usize,
    /// Nodes in this bucket, ordered by last seen (most recent at back)
    nodes: VecDeque<NodeInfo>,
}

impl KBucket {
    pub fn new(k: usize) -> Self {
        Self {
            k,
            nodes: VecDeque::with_capacity(k),
        }
    }

    /// Add or update a node in the bucket
    /// Returns true if the node was added/updated, false if bucket is full
    pub fn upsert(&mut self, node: NodeInfo) -> bool {
        // Check if node already exists
        if let Some(pos) = self.nodes.iter().position(|n| n.node_id == node.node_id) {
            // Move to back (most recently seen)
            self.nodes.remove(pos);
            self.nodes.push_back(node);
            return true;
        }

        // Add new node if space available
        if self.nodes.len() < self.k {
            self.nodes.push_back(node);
            return true;
        }

        false
    }

    /// Get all nodes in the bucket
    pub fn nodes(&self) -> impl Iterator<Item = &NodeInfo> {
        self.nodes.iter()
    }

    /// Check if bucket is full
    pub fn is_full(&self) -> bool {
        self.nodes.len() >= self.k
    }
}

/// Kademlia routing table
pub struct RoutingTable {
    /// Our node ID
    local_id: ContentHash,
    /// K-buckets (256 buckets for 256-bit IDs)
    buckets: Vec<KBucket>,
    /// K parameter
    k: usize,
}

impl RoutingTable {
    pub fn new(local_id: ContentHash, k: usize) -> Self {
        Self {
            local_id,
            buckets: (0..256).map(|_| KBucket::new(k)).collect(),
            k,
        }
    }

    /// Add or update a node in the routing table
    pub fn upsert(&mut self, node: NodeInfo) {
        let distance = xor_distance(&self.local_id, &node.node_id);
        let bucket_idx = bucket_index(&distance);
        self.buckets[bucket_idx].upsert(node);
    }

    /// Find the k closest nodes to a target
    pub fn closest_nodes(&self, target: &ContentHash, count: usize) -> Vec<NodeInfo> {
        let mut all_nodes: Vec<_> = self
            .buckets
            .iter()
            .flat_map(|b| b.nodes())
            .cloned()
            .collect();

        // Sort by distance to target
        all_nodes.sort_by(|a, b| {
            let dist_a = xor_distance(&a.node_id, target);
            let dist_b = xor_distance(&b.node_id, target);
            dist_a.cmp(&dist_b)
        });

        all_nodes.truncate(count);
        all_nodes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_xor_distance() {
        let a = [0u8; 32];
        let mut b = [0u8; 32];
        b[31] = 1;

        let dist = xor_distance(&a, &b);
        assert_eq!(dist[31], 1);
    }

    #[test]
    fn test_bucket_index() {
        let mut dist = [0u8; 32];
        dist[31] = 1;
        assert_eq!(bucket_index(&dist), 0);

        dist[31] = 0x80;
        assert_eq!(bucket_index(&dist), 7);

        dist[0] = 0x80;
        dist[31] = 0;
        assert_eq!(bucket_index(&dist), 255);
    }
}
