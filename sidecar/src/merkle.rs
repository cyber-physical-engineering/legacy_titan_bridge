//! Merkle Tree Accumulator
//!
//! Thread-safe Merkle tree implementation for batching transaction hashes.
//! Uses SHA-256 for hashing and provides efficient root computation.

use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use rs_merkle::{algorithms::Sha256, MerkleTree};
use sha2::{Digest, Sha256 as Sha256Hasher};
use tracing::info;

/// Status of the Merkle tree accumulator
#[derive(Debug, Clone)]
pub struct TreeStatus {
    /// Number of leaves in current tree
    pub leaf_count: usize,
    /// Number of pending commits (always 1 if tree has leaves, 0 otherwise)
    pub pending_commits: usize,
    /// Last computed root (if any)
    pub last_root: Option<[u8; 32]>,
    /// Timestamp of last commit
    pub last_commit_time: Option<String>,
}

/// Internal state for the Merkle accumulator
struct AccumulatorState {
    /// Current leaves (transaction hashes)
    leaves: Vec<[u8; 32]>,
    /// Last committed root
    last_root: Option<[u8; 32]>,
    /// Last commit timestamp
    last_commit_time: Option<DateTime<Utc>>,
    /// Total transactions processed (lifetime)
    total_processed: u64,
}

/// Thread-safe Merkle tree accumulator
pub struct MerkleAccumulator {
    state: RwLock<AccumulatorState>,
}

impl MerkleAccumulator {
    /// Create a new empty Merkle accumulator
    pub fn new() -> Self {
        Self {
            state: RwLock::new(AccumulatorState {
                leaves: Vec::new(),
                last_root: None,
                last_commit_time: None,
                total_processed: 0,
            }),
        }
    }

    /// Add a transaction hash to the tree
    /// Returns the position (0-indexed) of the new leaf
    pub fn add_leaf(&self, hash: &[u8; 32]) -> usize {
        let mut state = self.state.write();
        let position = state.leaves.len();
        state.leaves.push(*hash);
        state.total_processed += 1;

        info!(
            position = %position,
            total_processed = %state.total_processed,
            "Added leaf to Merkle tree"
        );

        position
    }

    /// Add multiple transaction hashes in batch
    /// Returns the starting position of the batch
    #[allow(dead_code)]
    pub fn add_leaves(&self, hashes: &[[u8; 32]]) -> usize {
        let mut state = self.state.write();
        let start_position = state.leaves.len();
        state.leaves.extend_from_slice(hashes);
        state.total_processed += hashes.len() as u64;

        info!(
            start_position = %start_position,
            count = %hashes.len(),
            total_processed = %state.total_processed,
            "Added batch to Merkle tree"
        );

        start_position
    }

    /// Compute the Merkle root of the current tree
    /// Returns None if the tree is empty
    pub fn compute_root(&self) -> Option<([u8; 32], usize)> {
        let state = self.state.read();

        if state.leaves.is_empty() {
            return None;
        }

        // Build Merkle tree
        let tree: MerkleTree<Sha256> = MerkleTree::from_leaves(&state.leaves);

        let root = tree.root()?;
        let leaf_count = state.leaves.len();

        info!(
            leaf_count = %leaf_count,
            root = %hex::encode(root),
            "Computed Merkle root"
        );

        Some((root, leaf_count))
    }

    /// Reset the tree after a successful commit
    pub fn reset(&self) {
        let mut state = self.state.write();

        // Store the last root before clearing
        if !state.leaves.is_empty() {
            if let Some((root, _)) = self.compute_root_internal(&state.leaves) {
                state.last_root = Some(root);
            }
        }

        state.last_commit_time = Some(Utc::now());
        state.leaves.clear();

        info!("Merkle tree reset after commit");
    }

    /// Get current tree status
    pub fn get_status(&self) -> TreeStatus {
        let state = self.state.read();

        TreeStatus {
            leaf_count: state.leaves.len(),
            pending_commits: if state.leaves.is_empty() { 0 } else { 1 },
            last_root: state.last_root,
            last_commit_time: state.last_commit_time.map(|t| t.to_rfc3339()),
        }
    }

    /// Get the total number of transactions processed (lifetime)
    #[allow(dead_code)]
    pub fn total_processed(&self) -> u64 {
        self.state.read().total_processed
    }

    /// Internal root computation without locking (for use when already holding lock)
    fn compute_root_internal(&self, leaves: &[[u8; 32]]) -> Option<([u8; 32], usize)> {
        if leaves.is_empty() {
            return None;
        }

        let tree: MerkleTree<Sha256> = MerkleTree::from_leaves(leaves);
        let root = tree.root()?;
        Some((root, leaves.len()))
    }

    /// Generate a Merkle proof for a specific leaf index
    /// Returns None if index is out of bounds or tree is empty
    #[allow(dead_code)]
    pub fn generate_proof(&self, index: usize) -> Option<Vec<[u8; 32]>> {
        let state = self.state.read();

        if index >= state.leaves.len() {
            return None;
        }

        let tree: MerkleTree<Sha256> = MerkleTree::from_leaves(&state.leaves);
        let proof = tree.proof(&[index]);

        Some(proof.proof_hashes().to_vec())
    }
}

impl Default for MerkleAccumulator {
    fn default() -> Self {
        Self::new()
    }
}

/// Hash a transaction to create a leaf for the Merkle tree
pub fn hash_transaction(
    tx_id: &str,
    amount: f64,
    from_account: &str,
    to_account: &str,
) -> [u8; 32] {
    let mut hasher = Sha256Hasher::new();

    hasher.update(tx_id.as_bytes());
    hasher.update(amount.to_le_bytes());
    hasher.update(from_account.as_bytes());
    hasher.update(to_account.as_bytes());

    let result = hasher.finalize();
    let mut hash = [0u8; 32];
    hash.copy_from_slice(&result);
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_tree() {
        let acc = MerkleAccumulator::new();
        assert!(acc.compute_root().is_none());
    }

    #[test]
    fn test_single_leaf() {
        let acc = MerkleAccumulator::new();
        let hash = hash_transaction("TX-001", 100.0, "ACC-001", "ACC-002");

        let pos = acc.add_leaf(&hash);
        assert_eq!(pos, 0);

        let (root, count) = acc.compute_root().unwrap();
        assert_eq!(count, 1);
        assert_eq!(root, hash); // Single leaf has itself as root
    }

    #[test]
    fn test_multiple_leaves() {
        let acc = MerkleAccumulator::new();

        for i in 0..100 {
            let hash = hash_transaction(
                &format!("TX-{:03}", i),
                (i as f64) * 100.0,
                "ACC-001",
                "ACC-002",
            );
            acc.add_leaf(&hash);
        }

        let (root, count) = acc.compute_root().unwrap();
        assert_eq!(count, 100);
        assert_ne!(root, [0u8; 32]);
    }

    #[test]
    fn test_reset() {
        let acc = MerkleAccumulator::new();
        let hash = hash_transaction("TX-001", 100.0, "ACC-001", "ACC-002");
        acc.add_leaf(&hash);

        acc.reset();

        let status = acc.get_status();
        assert_eq!(status.leaf_count, 0);
        assert!(status.last_root.is_some());
    }

    #[test]
    fn test_hash_determinism() {
        let hash1 = hash_transaction("TX-001", 100.0, "ACC-001", "ACC-002");
        let hash2 = hash_transaction("TX-001", 100.0, "ACC-001", "ACC-002");
        assert_eq!(hash1, hash2);

        let hash3 = hash_transaction("TX-002", 100.0, "ACC-001", "ACC-002");
        assert_ne!(hash1, hash3);
    }
}
