//! Filesystem helpers: the change token and the video-file filter.

use std::path::Path;
use std::time::UNIX_EPOCH;

/// Suffix of the on-disk operator "never transcode this" marker (spec 005 FR-016). It sits
/// beside the media as `<file>.apsisignore`, so an operator *ignore* is **recoverable** (it
/// survives a KV wipe — Principle I) rather than a live-only KV state.
pub const IGNORE_MARKER_SUFFIX: &str = ".apsisignore";

/// Path of the ignore marker for a media file.
fn ignore_marker_path(media: &Path) -> std::path::PathBuf {
    let mut s = media.as_os_str().to_os_string();
    s.push(IGNORE_MARKER_SUFFIX);
    std::path::PathBuf::from(s)
}

/// Whether `media` carries the ignore marker (the reconcile gate consults this).
#[must_use]
pub fn has_ignore_marker(media: &Path) -> bool {
    ignore_marker_path(media).exists()
}

/// Write the ignore marker beside `media` (idempotent).
///
/// # Errors
/// Propagates the I/O error if the marker cannot be created.
pub fn set_ignore_marker(media: &Path) -> std::io::Result<()> {
    std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(ignore_marker_path(media))
        .map(|_| ())
}

/// Remove the ignore marker for `media` (no error if absent).
///
/// # Errors
/// Propagates an I/O error other than "not found".
pub fn clear_ignore_marker(media: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(ignore_marker_path(media)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// Default video extensions (lowercase, no dot) when a library doesn't override.
pub const DEFAULT_VIDEO_EXTENSIONS: &[&str] = &[
    "mkv", "mp4", "avi", "mov", "m4v", "ts", "m2ts", "wmv", "flv", "webm", "mpg", "mpeg",
];

/// The change token `"mtime_nanos:size"` used to detect drift (FR-005). A changed
/// token supersedes any prior state for the path.
///
/// Uses **nanosecond** mtime precision: a same-second, same-size rewrite (e.g. a
/// container re-mux) would be missed at second granularity, so the sub-second
/// component is kept.
///
/// # Errors
/// Returns the underlying I/O error if the file cannot be `stat`'d.
pub fn version_token(path: &Path) -> std::io::Result<String> {
    let meta = std::fs::metadata(path)?;
    let mtime = meta
        .modified()?
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos()); // pre-epoch mtimes are absurd; treat as 0
    Ok(format!("{mtime}:{}", meta.len()))
}

/// Whether `path` is a video file, by extension. `extensions` overrides the
/// default set when non-empty (case-insensitive).
#[must_use]
pub fn is_video(path: &Path, extensions: &[String]) -> bool {
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return false;
    };
    if extensions.is_empty() {
        let ext = ext.to_ascii_lowercase();
        DEFAULT_VIDEO_EXTENSIONS.contains(&ext.as_str())
    } else {
        extensions.iter().any(|e| e.eq_ignore_ascii_case(ext))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_video_uses_defaults_then_overrides() {
        assert!(is_video(Path::new("/m/Show.MKV"), &[]));
        assert!(is_video(Path::new("/m/clip.mp4"), &[]));
        assert!(!is_video(Path::new("/m/poster.jpg"), &[]));
        assert!(!is_video(Path::new("/m/noext"), &[]));
        // override: only .foo counts
        let ov = vec!["foo".to_string()];
        assert!(is_video(Path::new("/m/a.FOO"), &ov));
        assert!(!is_video(Path::new("/m/a.mkv"), &ov));
    }

    #[test]
    fn version_token_reflects_size_and_is_stable() {
        let dir = std::env::temp_dir().join(format!("apsis-fsutil-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("t.bin");
        std::fs::write(&f, b"hello").unwrap();
        let a = version_token(&f).unwrap();
        let b = version_token(&f).unwrap();
        assert_eq!(a, b, "same file → same token");
        assert!(a.ends_with(":5"), "token carries the size: {a}");
        std::fs::write(&f, b"hello world").unwrap();
        assert_ne!(a, version_token(&f).unwrap(), "size change → new token");
        std::fs::remove_dir_all(&dir).ok();
    }
}
