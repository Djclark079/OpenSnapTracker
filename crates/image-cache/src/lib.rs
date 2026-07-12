//! Image cache bookkeeping.
//!
//! The cache is URL-keyed and stores files on disk. Image bytes are not stored
//! in SQLite.

use sha2::{Digest, Sha256};
use std::{
    fs, io,
    path::{Path, PathBuf},
};
use thiserror::Error;
use time::OffsetDateTime;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImageCacheEntry {
    pub image_url: String,
    pub local_path: PathBuf,
    pub content_type: Option<String>,
    pub content_length: Option<u64>,
    pub last_modified: Option<String>,
    pub downloaded_at: Option<OffsetDateTime>,
    pub last_accessed_at: Option<OffsetDateTime>,
    pub status: DownloadStatus,
    pub failure_count: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DownloadStatus {
    Missing,
    Downloaded,
    Failed,
}

#[derive(Debug, Error)]
pub enum ImageCacheError {
    #[error("image URL must use https: {0}")]
    NonHttpsUrl(String),
    #[error("image exceeds configured size limit: {actual} > {limit}")]
    TooLarge { actual: u64, limit: u64 },
    #[error(transparent)]
    Io(#[from] io::Error),
}

#[must_use]
pub fn cache_file_name(image_url: &str) -> String {
    format!("{}.webp", hex_hash(image_url.as_bytes()))
}

pub fn store_image_bytes(
    cache_dir: &Path,
    image_url: &str,
    bytes: &[u8],
    max_bytes: u64,
) -> Result<ImageCacheEntry, ImageCacheError> {
    if !image_url.starts_with("https://") {
        return Err(ImageCacheError::NonHttpsUrl(image_url.to_string()));
    }
    let len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    if len > max_bytes {
        return Err(ImageCacheError::TooLarge {
            actual: len,
            limit: max_bytes,
        });
    }
    fs::create_dir_all(cache_dir)?;
    let local_path = cache_dir.join(cache_file_name(image_url));
    fs::write(&local_path, bytes)?;
    Ok(ImageCacheEntry {
        image_url: image_url.to_string(),
        local_path,
        content_type: Some("image/webp".to_string()),
        content_length: Some(len),
        last_modified: None,
        downloaded_at: Some(OffsetDateTime::now_utc()),
        last_accessed_at: None,
        status: DownloadStatus::Downloaded,
        failure_count: 0,
    })
}

fn hex_hash(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn stores_image_by_full_url_hash() {
        let dir = tempdir().expect("tempdir");
        let url = "https://game-assets.snap.fan/card_variant_images/Abomination-hash.webp";
        let entry = store_image_bytes(dir.path(), url, b"webp-bytes", 1024).expect("store image");

        assert_eq!(entry.image_url, url);
        assert!(entry.local_path.exists());
        assert_eq!(entry.status, DownloadStatus::Downloaded);
        assert!(
            entry
                .local_path
                .file_name()
                .expect("file name")
                .to_string_lossy()
                .ends_with(".webp")
        );
    }

    #[test]
    fn rejects_oversized_or_non_https_assets() {
        let dir = tempdir().expect("tempdir");
        assert!(matches!(
            store_image_bytes(dir.path(), "http://example.test/card.webp", b"x", 10),
            Err(ImageCacheError::NonHttpsUrl(_))
        ));
        assert!(matches!(
            store_image_bytes(
                dir.path(),
                "https://example.test/card.webp",
                b"too large",
                3
            ),
            Err(ImageCacheError::TooLarge { .. })
        ));
    }
}
