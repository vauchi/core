// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Merkle Tree for Efficient Sync
//!
//! A simple Merkle tree implementation using SHA-256 (via `ring`) for
//! efficient comparison of contact state between devices. By comparing
//! root hashes, two devices can quickly determine if they are in sync.
//! If not, the `diff` method identifies which leaf indices differ.

use ring::digest;

/// A Merkle tree built from contact hashes.
///
/// Each leaf is a 32-byte SHA-256 hash representing the state of a contact.
/// Internal nodes are computed by hashing the concatenation of their children.
/// The root hash summarizes the entire contact set.
#[derive(Debug, Clone)]
pub struct MerkleTree {
    root: [u8; 32],
    leaves: Vec<[u8; 32]>,
}

impl MerkleTree {
    /// Builds a Merkle tree from a list of contact state hashes.
    ///
    /// If the list is empty, the root is a hash of an empty byte string.
    /// If the number of leaves is not a power of two, the tree is padded
    /// with zero hashes on the right.
    pub fn from_contact_hashes(hashes: Vec<[u8; 32]>) -> Self {
        let root = Self::compute_root(&hashes);
        MerkleTree {
            root,
            leaves: hashes,
        }
    }

    /// Returns a reference to the root hash.
    pub fn root_hash(&self) -> &[u8; 32] {
        &self.root
    }

    /// Returns the leaf hashes.
    pub fn leaves(&self) -> &[[u8; 32]] {
        &self.leaves
    }

    /// Compares this tree with another and returns the indices of leaves
    /// that differ.
    ///
    /// Only compares up to the length of the shorter leaf list. Indices
    /// present in one tree but not the other are also reported as diffs.
    pub fn diff(&self, other: &MerkleTree) -> Vec<usize> {
        let max_len = self.leaves.len().max(other.leaves.len());
        let mut diffs = Vec::new();

        for i in 0..max_len {
            let a = self.leaves.get(i);
            let b = other.leaves.get(i);

            match (a, b) {
                (Some(ha), Some(hb)) => {
                    if ha != hb {
                        diffs.push(i);
                    }
                }
                // One tree has a leaf at this index but the other doesn't.
                _ => {
                    diffs.push(i);
                }
            }
        }

        diffs
    }

    /// Computes the Merkle root from a list of leaf hashes.
    fn compute_root(leaves: &[[u8; 32]]) -> [u8; 32] {
        if leaves.is_empty() {
            let digest = digest::digest(&digest::SHA256, b"");
            let mut root = [0u8; 32];
            root.copy_from_slice(digest.as_ref());
            return root;
        }

        if leaves.len() == 1 {
            return leaves[0];
        }

        // Build the tree bottom-up by hashing pairs.
        let mut current_level: Vec<[u8; 32]> = leaves.to_vec();

        while current_level.len() > 1 {
            let mut next_level = Vec::new();

            for chunk in current_level.chunks(2) {
                let left = &chunk[0];
                let right = if chunk.len() > 1 {
                    &chunk[1]
                } else {
                    // Odd number of nodes: duplicate the last one.
                    &chunk[0]
                };

                let mut combined = Vec::with_capacity(64);
                combined.extend_from_slice(left);
                combined.extend_from_slice(right);

                let digest = digest::digest(&digest::SHA256, &combined);
                let mut hash = [0u8; 32];
                hash.copy_from_slice(digest.as_ref());
                next_level.push(hash);
            }

            current_level = next_level;
        }

        current_level[0]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::digest;

    fn hash_bytes(data: &[u8]) -> [u8; 32] {
        let d = digest::digest(&digest::SHA256, data);
        let mut h = [0u8; 32];
        h.copy_from_slice(d.as_ref());
        h
    }

    #[test]
    fn test_empty_tree() {
        let tree = MerkleTree::from_contact_hashes(vec![]);
        // Root should be hash of empty string.
        let expected = hash_bytes(b"");
        assert_eq!(tree.root_hash(), &expected);
    }

    #[test]
    fn test_single_leaf() {
        let leaf = hash_bytes(b"contact-1");
        let tree = MerkleTree::from_contact_hashes(vec![leaf]);
        assert_eq!(tree.root_hash(), &leaf);
    }

    #[test]
    fn test_two_leaves() {
        let h1 = hash_bytes(b"contact-1");
        let h2 = hash_bytes(b"contact-2");
        let tree = MerkleTree::from_contact_hashes(vec![h1, h2]);

        // Root = SHA256(h1 || h2)
        let mut combined = Vec::new();
        combined.extend_from_slice(&h1);
        combined.extend_from_slice(&h2);
        let expected = hash_bytes(&combined);
        assert_eq!(tree.root_hash(), &expected);
    }

    #[test]
    fn test_identical_trees_no_diff() {
        let hashes = vec![hash_bytes(b"a"), hash_bytes(b"b"), hash_bytes(b"c")];
        let tree1 = MerkleTree::from_contact_hashes(hashes.clone());
        let tree2 = MerkleTree::from_contact_hashes(hashes);

        assert_eq!(tree1.root_hash(), tree2.root_hash());
        assert!(tree1.diff(&tree2).is_empty());
    }

    #[test]
    fn test_different_trees_diff() {
        let hashes1 = vec![hash_bytes(b"a"), hash_bytes(b"b"), hash_bytes(b"c")];
        let hashes2 = vec![hash_bytes(b"a"), hash_bytes(b"CHANGED"), hash_bytes(b"c")];

        let tree1 = MerkleTree::from_contact_hashes(hashes1);
        let tree2 = MerkleTree::from_contact_hashes(hashes2);

        assert_ne!(tree1.root_hash(), tree2.root_hash());
        assert_eq!(tree1.diff(&tree2), vec![1]);
    }

    #[test]
    fn test_diff_different_lengths() {
        let hashes1 = vec![hash_bytes(b"a"), hash_bytes(b"b")];
        let hashes2 = vec![hash_bytes(b"a"), hash_bytes(b"b"), hash_bytes(b"c")];

        let tree1 = MerkleTree::from_contact_hashes(hashes1);
        let tree2 = MerkleTree::from_contact_hashes(hashes2);

        // Index 2 is present in tree2 but not tree1.
        assert_eq!(tree1.diff(&tree2), vec![2]);
    }

    #[test]
    fn test_deterministic_root() {
        let hashes = vec![
            hash_bytes(b"x"),
            hash_bytes(b"y"),
            hash_bytes(b"z"),
            hash_bytes(b"w"),
        ];

        let tree1 = MerkleTree::from_contact_hashes(hashes.clone());
        let tree2 = MerkleTree::from_contact_hashes(hashes);

        assert_eq!(tree1.root_hash(), tree2.root_hash());
    }
}
