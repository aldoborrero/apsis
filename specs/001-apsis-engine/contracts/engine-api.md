# Phase 1 — Public API contract: apsis-engine

For a library, the "contract" is its **public surface**. Consumers: the coordinator
(planning) and the worker (command building).

## Planning

```rust
/// Decide the full end-state for `probe` under `profile`. Never panics.
pub fn plan(probe: &Probe, profile: &Profile) -> FilePlan;
```
Contract:
- Pure and deterministic: same (probe, profile) → identical `FilePlan`.
- `should_skip == true` iff the file already matches the profile (no drift).
- MUST NOT produce a plan whose audio list is empty when the source had audio (FR-006).
- Already-target codec (`skip_codecs`) and `hdr_policy = Copy` → `VideoAction::Copy`.

## Probe parsing (pure) + optional exec helper

```rust
/// Parse `ffprobe -of json -show_streams -show_format` output.
pub fn parse_probe(json: &str) -> Result<Probe, EngineError>;

/// Optional convenience (cargo feature `probe-exec`): run ffprobe on a path.
#[cfg(feature = "probe-exec")]
pub fn probe_file(path: &Path, ffprobe: &Path) -> Result<Probe, EngineError>;
```

## Command building

```rust
pub trait Backend {
    fn kind(&self) -> BackendKind;
    /// Materialize `plan` into a concrete ffmpeg command for this backend.
    fn build(&self, plan: &FilePlan, input: &Path, output: &Path) -> FfmpegCommandSpec;
}

pub struct VaapiBackend { pub device: String, pub hw_decode: Vec<String>,
                          pub sei_workaround: bool, pub async_depth: u32 }
pub struct CpuBackend   { pub preset: String }

impl FfmpegCommandSpec { pub fn to_args(&self) -> Vec<String>; }
```
Contract:
- `VaapiBackend::build` MUST emit `-sei hdr` for `hevc`/`h264` encodes when
  `sei_workaround` (a53_cc), and select HW vs SW decode by source codec.
- `CpuBackend::build` uses `libx265`/`libsvtav1` at `plan.video` quality.
- For `VideoAction::Copy`, both backends emit `-c:v copy` and no encoder options.
- `to_args()` is deterministic (stable ordering) so golden tests are byte-exact.

## Config loading (caller-side)

`Profile` derives `serde::Deserialize` + `garde::Validate`; the caller deserializes it
from `scheduler.toml` and calls `profile.validate()`. The engine does **not** read files
or env (that is the coordinator's job, spec 002).

## Error behavior

Every fallible entry point returns `Result<_, EngineError>`. A file with no video stream
yields a `FilePlan` with `VideoAction::Unsupported` + `should_skip = true` (not an error,
not a panic).
