// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Cached content takes as little space as possible: files are stored
//! compressed and read back whole (vauchi/private#733 D9). A stored file
//! that does not inflate, or inflates past the size cap, is dropped with
//! the manifest so the next update cycle downloads it again (DC-03).

use std::fs;
use std::path::Path;
use tempfile::TempDir;
use vauchi_app::content::{
    ContentCache, ContentIndex, ContentManifest, ContentType, compute_checksum,
};

const CAP: u64 = 64 * 1024;

fn locale_bytes() -> Vec<u8> {
    let entries: Vec<String> = (0..2000)
        .map(|i| format!("\"key.{i}\": \"A translated string number {i}\""))
        .collect();
    format!("{{{}}}", entries.join(",")).into_bytes()
}

fn manifest() -> ContentManifest {
    ContentManifest {
        schema_version: 1,
        generated_at: "2026-10-22T00:00:00Z".to_string(),
        base_url: "https://cdn.vauchi.app/v1/".to_string(),
        content: ContentIndex::default(),
        signature: None,
    }
}

fn bytes_on_disk(dir: &Path) -> u64 {
    fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|entry| entry.metadata().unwrap().len())
        .sum()
}

fn overwrite_stored_files(dir: &Path, bytes: &[u8]) {
    for entry in fs::read_dir(dir).unwrap().flatten() {
        fs::write(entry.path(), bytes).unwrap();
    }
}

fn cache_with_stored_locale(temp: &TempDir) -> (ContentCache, Vec<u8>) {
    let cache = ContentCache::new(temp.path())
        .unwrap()
        .with_max_content_size(CAP);
    let data = locale_bytes();
    cache
        .save_content(
            ContentType::Locales,
            "de.json",
            &data,
            &compute_checksum(&data),
        )
        .unwrap();
    cache.save_manifest(&manifest()).unwrap();
    (cache, data)
}

// @internal
#[test]
fn a_cached_file_is_stored_compressed_and_read_back_whole() {
    let temp = TempDir::new().unwrap();
    let (cache, data) = cache_with_stored_locale(&temp);

    let stored = bytes_on_disk(&cache.content_dir(ContentType::Locales));

    assert!(
        stored * 4 < data.len() as u64,
        "stored {stored} bytes for {} bytes of content",
        data.len()
    );
    assert_eq!(
        cache.get_content(ContentType::Locales, "de.json"),
        Some(data)
    );
}

// @internal
#[test]
fn the_cached_files_of_a_type_are_listed_by_their_names() {
    let temp = TempDir::new().unwrap();
    let (cache, data) = cache_with_stored_locale(&temp);
    cache
        .save_content(
            ContentType::Locales,
            "fr.json",
            &data,
            &compute_checksum(&data),
        )
        .unwrap();

    let mut names = cache.content_files(ContentType::Locales);
    names.sort();

    assert_eq!(names, ["de.json", "fr.json"]);
}

// @internal
#[test]
fn a_stored_file_that_does_not_inflate_is_dropped_with_the_manifest() {
    let truncated = {
        let temp = TempDir::new().unwrap();
        let (cache, _) = cache_with_stored_locale(&temp);
        let path = fs::read_dir(cache.content_dir(ContentType::Locales))
            .unwrap()
            .flatten()
            .next()
            .unwrap()
            .path();
        let stored = fs::read(path).unwrap();
        stored[..stored.len() / 2].to_vec()
    };
    for (label, bytes) in [
        ("truncated", truncated),
        ("not gzip", br#"{"key.0": "plain json"}"#.to_vec()),
        ("empty", Vec::new()),
    ] {
        let temp = TempDir::new().unwrap();
        let (cache, _) = cache_with_stored_locale(&temp);
        overwrite_stored_files(&cache.content_dir(ContentType::Locales), &bytes);

        assert_eq!(
            cache.get_content(ContentType::Locales, "de.json"),
            None,
            "{label}"
        );
        assert_eq!(
            cache.content_files(ContentType::Locales),
            Vec::<String>::new(),
            "{label}"
        );
        assert!(cache.get_manifest().is_none(), "{label}: manifest kept");
    }
}

// @internal
#[test]
fn a_stored_file_that_inflates_past_the_cap_is_dropped() {
    use std::io::Write;
    let temp = TempDir::new().unwrap();
    let (cache, _) = cache_with_stored_locale(&temp);
    let mut bomb = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    bomb.write_all(&vec![b' '; (CAP * 16) as usize]).unwrap();
    overwrite_stored_files(
        &cache.content_dir(ContentType::Locales),
        &bomb.finish().unwrap(),
    );

    assert_eq!(cache.get_content(ContentType::Locales, "de.json"), None);
    assert!(cache.get_manifest().is_none(), "manifest kept");
}
