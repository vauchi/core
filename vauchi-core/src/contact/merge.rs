// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Contact Duplicate Detection and Merge

use crate::contact::Contact;

/// A detected duplicate pair with similarity score.
#[derive(Debug)]
pub struct DuplicatePair {
    /// ID of the first contact.
    pub id1: String,
    /// ID of the second contact.
    pub id2: String,
    /// Similarity score (0.0 to 1.0).
    pub similarity: f64,
}

/// Finds potential duplicate contacts based on name and field similarity.
///
/// Returns pairs of contacts that exceed the similarity threshold (0.7).
pub fn find_duplicates(contacts: &[Contact]) -> Vec<DuplicatePair> {
    let threshold = 0.7;
    let mut duplicates = Vec::new();

    for i in 0..contacts.len() {
        for j in (i + 1)..contacts.len() {
            let sim = compute_similarity(&contacts[i], &contacts[j]);
            if sim >= threshold {
                duplicates.push(DuplicatePair {
                    id1: contacts[i].id().to_string(),
                    id2: contacts[j].id().to_string(),
                    similarity: sim,
                });
            }
        }
    }

    duplicates.sort_by(|a, b| {
        b.similarity
            .partial_cmp(&a.similarity)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    duplicates
}

/// Computes similarity between two contacts (0.0 to 1.0).
fn compute_similarity(a: &Contact, b: &Contact) -> f64 {
    let mut score = 0.0;
    let mut max_score = 0.0;

    // Name similarity (weight: 2.0)
    max_score += 2.0;
    let name_sim = string_similarity(a.display_name(), b.display_name());
    score += name_sim * 2.0;

    // Field value overlap (weight: 1.0 each)
    for field_a in a.card().fields() {
        for field_b in b.card().fields() {
            if field_a.field_type() == field_b.field_type() {
                max_score += 1.0;
                let field_sim = string_similarity(field_a.value(), field_b.value());
                score += field_sim;
            }
        }
    }

    if max_score == 0.0 {
        return 0.0;
    }

    score / max_score
}

/// Simple string similarity using normalized Levenshtein-like comparison.
fn string_similarity(a: &str, b: &str) -> f64 {
    let a_lower = a.to_lowercase();
    let b_lower = b.to_lowercase();

    if a_lower == b_lower {
        return 1.0;
    }

    if a_lower.is_empty() || b_lower.is_empty() {
        return 0.0;
    }

    // Check if one contains the other
    if a_lower.contains(&b_lower) || b_lower.contains(&a_lower) {
        return 0.8;
    }

    // Simple character overlap ratio
    let a_chars: std::collections::HashSet<char> = a_lower.chars().collect();
    let b_chars: std::collections::HashSet<char> = b_lower.chars().collect();
    let intersection = a_chars.intersection(&b_chars).count();
    let union = a_chars.union(&b_chars).count();

    if union == 0 {
        0.0
    } else {
        intersection as f64 / union as f64
    }
}
