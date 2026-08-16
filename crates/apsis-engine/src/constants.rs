//! Shared constants (port of `_engine/constants.py`).

/// Case-insensitive match of the Python `COMMENTARY_PATTERN`
/// (`commentary|director|description`, IGNORECASE) against a track title.
#[must_use]
pub(crate) fn is_commentary(title: &str) -> bool {
    let t = title.to_lowercase();
    t.contains("commentary") || t.contains("director") || t.contains("description")
}
