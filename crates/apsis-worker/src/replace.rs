//! Atomic replace of the original with the verified transcode (FR-007/008).
//!
//! The original is mutated only by a single `rename(2)` — atomic within a
//! filesystem — and only after the caller has verified the temp. On any error
//! the original is left byte-identical; the caller cleans up the temp. Ownership,
//! mode, and mtime are re-applied so library indexers (Jellyfin/Sonarr) see the
//! file unchanged (Unmanic's core drops these — we bake it in).

use std::fs::{self, File, FileTimes, OpenOptions, Permissions};
use std::io;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;

/// Install `temp` as `output`, atomically, preserving `input`'s ownership/mode/
/// mtime. When `output == input` (same container) the `rename` replaces in place;
/// when they differ (extension change) the new file lands and the old original is
/// removed afterwards.
///
/// The temp MUST already be on the same filesystem as `output` (same directory)
/// so the `rename` is atomic — the caller guarantees this by writing the temp
/// beside the source.
///
/// # Errors
/// Any I/O failure (`stat`, `fsync`, `rename`, `remove`). On error the original
/// is untouched; the caller removes the temp.
pub(crate) fn atomic_replace(input: &Path, temp: &Path, output: &Path) -> io::Result<()> {
    // Capture the original's metadata to re-apply onto the new file.
    let meta = fs::metadata(input)?;
    let (uid, gid, mode) = (meta.uid(), meta.gid(), meta.mode());
    let mtime = meta.modified()?;
    let atime = meta.accessed().unwrap_or(mtime);

    // Durability: flush the temp's contents and its directory entry before the
    // rename, so a crash can't leave a half-written file swapped in.
    File::open(temp)?.sync_all()?;
    if let Some(dir) = temp.parent() {
        sync_dir(dir)?;
    }

    // The atomic install. Same path → in-place replace; different path (extension
    // change) → new file appears, old original removed only after.
    fs::rename(temp, output)?;
    if output != input {
        fs::remove_file(input)?;
    }

    // Re-apply metadata. Order matters: set times + mode while we still own the
    // file, then chown last (it may drop our ownership). chown is best-effort —
    // it needs privilege the worker may not have; a transcode shouldn't fail over
    // an ownership tweak.
    apply_times(output, mtime, atime)?;
    fs::set_permissions(output, Permissions::from_mode(mode))?;
    let _ = std::os::unix::fs::chown(output, Some(uid), Some(gid));

    if let Some(dir) = output.parent() {
        sync_dir(dir)?;
    }
    Ok(())
}

fn sync_dir(dir: &Path) -> io::Result<()> {
    File::open(dir)?.sync_all()
}

fn apply_times(
    path: &Path,
    mtime: std::time::SystemTime,
    atime: std::time::SystemTime,
) -> io::Result<()> {
    let f = OpenOptions::new().write(true).open(path)?;
    f.set_times(FileTimes::new().set_modified(mtime).set_accessed(atime))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    fn tmpdir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("apsis-replace-{tag}-{}", std::process::id()));
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn same_container_replace_swaps_content_and_preserves_metadata() {
        let d = tmpdir("same");
        let input = d.join("clip.mkv");
        let temp = d.join(".apsis-tmp-x.mkv");
        fs::write(&input, b"ORIGINAL").unwrap();
        fs::write(&temp, b"TRANSCODED-SMALLER").unwrap();

        // Pin a distinctive mtime + mode on the original.
        let want_mtime = UNIX_EPOCH + Duration::from_secs(1_600_000_000);
        fs::set_permissions(&input, Permissions::from_mode(0o640)).unwrap();
        OpenOptions::new()
            .write(true)
            .open(&input)
            .unwrap()
            .set_times(
                FileTimes::new()
                    .set_modified(want_mtime)
                    .set_accessed(want_mtime),
            )
            .unwrap();

        atomic_replace(&input, &temp, &input).unwrap();

        assert_eq!(fs::read(&input).unwrap(), b"TRANSCODED-SMALLER");
        assert!(!temp.exists(), "temp consumed by rename");
        let m = fs::metadata(&input).unwrap();
        assert_eq!(m.mode() & 0o777, 0o640, "mode preserved from original");
        assert_eq!(m.modified().unwrap(), want_mtime, "mtime preserved");
        fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn extension_change_lands_new_and_removes_old() {
        let d = tmpdir("ext");
        let input = d.join("clip.avi");
        let temp = d.join(".apsis-tmp-y.mkv");
        let output = d.join("clip.mkv");
        fs::write(&input, b"AVI").unwrap();
        fs::write(&temp, b"MKV").unwrap();

        atomic_replace(&input, &temp, &output).unwrap();

        assert_eq!(fs::read(&output).unwrap(), b"MKV");
        assert!(!input.exists(), "old original removed after ext change");
        assert!(!temp.exists());
        fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn error_leaves_original_untouched() {
        // A missing temp makes the rename fail; the original must be byte-identical.
        let d = tmpdir("err");
        let input = d.join("clip.mkv");
        fs::write(&input, b"ORIGINAL").unwrap();
        let missing_temp = d.join(".apsis-tmp-gone.mkv");

        let err = atomic_replace(&input, &missing_temp, &input);
        assert!(err.is_err());
        assert_eq!(
            fs::read(&input).unwrap(),
            b"ORIGINAL",
            "original untouched on error"
        );
        fs::remove_dir_all(&d).ok();
    }
}
