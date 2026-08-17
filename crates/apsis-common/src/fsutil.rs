//! Filesystem helpers: the change token and the video-file filter.

use std::path::Path;
use std::time::UNIX_EPOCH;

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
