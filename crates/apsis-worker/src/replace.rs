//! Atomic replace of the original with the verified transcode (FR-007/008).
//!
//! The original is mutated only by an atomic rename — within a filesystem — and
//! only after the caller has verified the temp. On any error the original is left
//! byte-identical; the caller cleans up the temp. Ownership, mode, and mtime are
//! re-applied so library indexers (Jellyfin/Sonarr) see the file unchanged
//! (Unmanic's core drops these — we bake it in).
//!
//! **Closing the source-changed-during-transcode TOCTOU.** A long encode races a
//! new import overwriting the source: if the source moved to a newer version while
//! we transcoded, installing our (now stale) output would silently revert the
//! user's newer content. For the common same-container replace we do this
//! atomically with `renameat2(RENAME_EXCHANGE)`: swap temp ↔ source in one syscall,
//! then inspect the content we swapped *out* — it is a snapshot taken at the
//! instant of the swap, so there is no check-then-act window. If it isn't the
//! version we transcoded from, we swap back (restoring the original untouched) and
//! report [`ReplaceOutcome::Superseded`]. Filesystems without `RENAME_EXCHANGE`
//! (NFS, older ZFS) fall back to a re-stat + plain rename, which keeps a sub-ms
//! window that cannot be closed without the syscall.

use std::fs::{self, File, FileTimes, OpenOptions, Permissions};
use std::io;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;

use nix::errno::Errno;
use nix::fcntl::{RenameFlags, renameat2};

/// Whether the verified output actually replaced the original.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ReplaceOutcome {
    /// The transcode is installed as `output`; the temp is consumed.
    Installed,
    /// The source changed to a newer version during the transcode, so our output
    /// is stale. The original is untouched; the temp still holds our output for
    /// the caller to discard.
    Superseded,
}

/// Install `temp` as `output`, atomically, preserving `input`'s ownership/mode/
/// mtime, but only if `input` is still the `expected_version` we transcoded from.
///
/// When `output == input` (same container) the swap is an atomic `RENAME_EXCHANGE`
/// with a verify-the-swapped-out-content (see the module docs); when they differ
/// (extension change) there is no existing `output` to exchange, so we re-stat the
/// source and plain-rename. A newer source ⇒ [`ReplaceOutcome::Superseded`], never
/// a revert.
///
/// The temp MUST already be on the same filesystem as `output` (same directory)
/// so the rename is atomic — the caller guarantees this by writing the temp beside
/// the source.
///
/// # Errors
/// Any I/O failure (`stat`, `fsync`, `rename`, `remove`). On error the original is
/// untouched; the caller removes the temp.
pub(crate) fn atomic_replace(
    input: &Path,
    temp: &Path,
    output: &Path,
    expected_version: &str,
) -> io::Result<ReplaceOutcome> {
    // Capture the original's metadata to re-apply onto the new file.
    let meta = fs::metadata(input)?;
    let (uid, gid, mode) = (meta.uid(), meta.gid(), meta.mode());
    let mtime = meta.modified()?;
    let atime = meta.accessed().unwrap_or(mtime);

    // Durability: flush the temp's contents and its directory entry before the
    // rename, so a crash can't leave a half-written file swapped in. The dir fd is
    // reused as the `renameat2` anchor below (relative names → no re-resolving the
    // parent path).
    File::open(temp)?.sync_all()?;
    let dir = File::open(temp.parent().unwrap_or_else(|| Path::new(".")))?;
    dir.sync_all()?;

    if output == input {
        match exchange(&dir, temp, input) {
            // Swap done. `temp` now holds what `input` was AT THE SWAP; if that
            // isn't the version we transcoded from, the source changed under us.
            Ok(()) => {
                if version_of(temp).as_deref() == Some(expected_version) {
                    let _ = fs::remove_file(temp); // old content, no longer needed
                } else {
                    let _ = exchange(&dir, temp, input); // undo: original restored
                    return Ok(ReplaceOutcome::Superseded);
                }
            }
            // Filesystem without RENAME_EXCHANGE (NFS, older ZFS): fall back to a
            // re-stat + plain rename. A sub-ms TOCTOU remains here, unavoidable
            // without the syscall.
            Err(Errno::EINVAL | Errno::ENOSYS | Errno::EOPNOTSUPP) => {
                if version_of(input).as_deref() != Some(expected_version) {
                    return Ok(ReplaceOutcome::Superseded);
                }
                fs::rename(temp, output)?;
            }
            Err(e) => return Err(io::Error::from_raw_os_error(e as i32)),
        }
    } else {
        // Extension change: no existing `output` to exchange with, so re-stat the
        // source and plain-rename (narrow TOCTOU).
        if version_of(input).as_deref() != Some(expected_version) {
            return Ok(ReplaceOutcome::Superseded);
        }
        // --- THE COMMIT POINT --- nothing below may turn a committed install into
        // an error, so every step after the rename is best-effort.
        fs::rename(temp, output)?;
        if fs::remove_file(input).is_err() {
            eprintln!(
                "apsis-worker: installed {} but could not remove old {} — a duplicate remains",
                output.display(),
                input.display()
            );
        }
    }

    // Re-apply metadata (best-effort). Times + mode while we still own the file,
    // chown last (may drop our ownership); none of these is worth failing the
    // already-committed replace over.
    let _ = apply_times(output, mtime, atime);
    let _ = fs::set_permissions(output, Permissions::from_mode(mode));
    let _ = std::os::unix::fs::chown(output, Some(uid), Some(gid));
    let _ = dir.sync_all();
    Ok(ReplaceOutcome::Installed)
}

/// Atomically swap two files within `dir` (relative names anchor the exchange to
/// the already-open dir fd). `Err(EINVAL/ENOSYS/EOPNOTSUPP)` ⇒ the filesystem does
/// not support `RENAME_EXCHANGE`.
fn exchange(dir: &File, a: &Path, b: &Path) -> Result<(), Errno> {
    let name = |p: &Path| p.file_name().map(std::ffi::OsStr::to_os_string);
    let (Some(an), Some(bn)) = (name(a), name(b)) else {
        return Err(Errno::EINVAL); // a temp/source with no filename can't be swapped
    };
    renameat2(
        dir,
        an.as_os_str(),
        dir,
        bn.as_os_str(),
        RenameFlags::RENAME_EXCHANGE,
    )
}

/// The `"mtime:size"` change token of `path`, or `None` if it can't be stat'd.
fn version_of(path: &Path) -> Option<String> {
    apsis_common::version_token(path).ok()
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

        // The job was planned for the original's current token.
        let version = apsis_common::version_token(&input).unwrap();
        let out = atomic_replace(&input, &temp, &input, &version).unwrap();

        assert_eq!(out, ReplaceOutcome::Installed);
        assert_eq!(fs::read(&input).unwrap(), b"TRANSCODED-SMALLER");
        assert!(!temp.exists(), "temp consumed by the swap");
        let m = fs::metadata(&input).unwrap();
        assert_eq!(m.mode() & 0o777, 0o640, "mode preserved from original");
        assert_eq!(m.modified().unwrap(), want_mtime, "mtime preserved");
        fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn same_container_superseded_restores_original() {
        // The source is overwritten by a newer import DURING our transcode: its
        // token no longer matches the job's, so our output must be discarded and
        // the newer content left byte-identical (no silent revert).
        let d = tmpdir("superseded");
        let input = d.join("clip.mkv");
        fs::write(&input, b"ORIGINAL-V1").unwrap();
        let planned_version = apsis_common::version_token(&input).unwrap();

        // New import lands (different size → different token) while we encoded.
        fs::write(&input, b"NEWER-IMPORT-V2-LONGER").unwrap();
        let temp = d.join(".apsis-tmp-x.mkv");
        fs::write(&temp, b"TRANSCODED-FROM-V1").unwrap();

        let out = atomic_replace(&input, &temp, &input, &planned_version).unwrap();

        assert_eq!(out, ReplaceOutcome::Superseded);
        assert_eq!(
            fs::read(&input).unwrap(),
            b"NEWER-IMPORT-V2-LONGER",
            "newer source content preserved, not reverted"
        );
        assert!(
            temp.exists(),
            "our stale output left for the caller to discard"
        );
        assert_eq!(fs::read(&temp).unwrap(), b"TRANSCODED-FROM-V1");
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

        let version = apsis_common::version_token(&input).unwrap();
        let out = atomic_replace(&input, &temp, &output, &version).unwrap();

        assert_eq!(out, ReplaceOutcome::Installed);
        assert_eq!(fs::read(&output).unwrap(), b"MKV");
        assert!(!input.exists(), "old original removed after ext change");
        assert!(!temp.exists());
        fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn extension_change_superseded_leaves_both_files() {
        // Ext-change path: a changed source is not overwritten; our .mkv output is
        // kept for the caller to discard and the .avi original stays intact.
        let d = tmpdir("ext-superseded");
        let input = d.join("clip.avi");
        let output = d.join("clip.mkv");
        fs::write(&input, b"AVI-V1").unwrap();
        let planned_version = apsis_common::version_token(&input).unwrap();
        fs::write(&input, b"AVI-V2-CHANGED").unwrap();
        let temp = d.join(".apsis-tmp-y.mkv");
        fs::write(&temp, b"MKV").unwrap();

        let out = atomic_replace(&input, &temp, &output, &planned_version).unwrap();

        assert_eq!(out, ReplaceOutcome::Superseded);
        assert_eq!(
            fs::read(&input).unwrap(),
            b"AVI-V2-CHANGED",
            "source untouched"
        );
        assert!(!output.exists(), "nothing installed at output");
        assert!(temp.exists(), "our output left for the caller to discard");
        fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn error_leaves_original_untouched() {
        // A missing temp makes the pre-rename fsync fail; the original must be
        // byte-identical.
        let d = tmpdir("err");
        let input = d.join("clip.mkv");
        fs::write(&input, b"ORIGINAL").unwrap();
        let missing_temp = d.join(".apsis-tmp-gone.mkv");
        let version = apsis_common::version_token(&input).unwrap();

        let err = atomic_replace(&input, &missing_temp, &input, &version);
        assert!(err.is_err());
        assert_eq!(
            fs::read(&input).unwrap(),
            b"ORIGINAL",
            "original untouched on error"
        );
        fs::remove_dir_all(&d).ok();
    }
}
