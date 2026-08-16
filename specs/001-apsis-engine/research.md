# Phase 0 — Research & Decisions: apsis-engine

The design doc (`docs/design/rust-scheduler.md`) and the existing Python `_engine`
resolve almost everything; this records the decisions and the few open points.

## Decisions

- **Port to Rust (not FFI).** The crate is pure Rust — no calling into Python. (The
  *interim shell-out to the Python engine* mentioned in the design rollout is a
  coordinator-level option in spec 002, not part of this crate.) Rationale: the
  single-static-binary goal (Constitution II).
- **Pure logic, probe passed in.** The engine's core takes a parsed `Probe` +
  `Profile` and returns a `FilePlan`; it does **not** run ffprobe itself. An optional
  `probe-exec` cargo feature provides a thin `probe_file(path)` helper (invokes ffprobe,
  parses JSON) for convenience/tests. Keeps the crate dependency-light and orchestration-
  free (FR-008).
- **ffprobe JSON via `serde_json`.** Deserialize `ffprobe -of json -show_streams
  -show_format` into typed structs; unknown fields ignored.
- **Rust enums for closed sets.** `VideoCodec {Hevc, Av1}`, stream `Action {Copy, Encode,
  Drop, Generate}`, `HdrPolicy {Copy, Tonemap, Encode}`, `Encoder {Vaapi, Cpu}` — replaces
  the Python `Literal[...]` types; invalid values are deserialization errors.
- **Validation via `garde`.** Derive-based (`quality ∈ 0..=51`, etc.), replacing pydantic
  validators. Cross-references that need the whole config (e.g. `library.profile` exists)
  are the coordinator's concern (spec 002), not the engine.
- **Deterministic command output.** `FfmpegCommandSpec` builds a stable, ordered arg
  vector so golden tests are byte-exact (SC-002). Stream mapping order mirrors the Python
  builder.
- **VAAPI `-sei hdr` workaround** lives only in `backend/vaapi.rs` for hevc/h264 encodes
  (the AMD 780M a53_cc bug); AV1 unaffected. HW-decode for hevc/av1/vp9, SW-decode for
  h264 (the affected path).
- **Oracle = the Python `_engine` test suite** (branch `feat/unmanic-integration`).
  Fixtures (ffprobe JSON + expected plan/command) are ported into `tests/fixtures/`.

## Open / deferred (not blocking)

- **CC recovery** (`_RECOVER_CC` ASS pre-pass): out of scope for this crate; a later
  worker-side pre-pass (spec 002/003).
- **NVENC backend**: a later `Backend` impl (spec 003); the trait must not assume VAAPI.
- **AV1 encode tuning** (svtav1 params): mirror the Python defaults for now.
- **`target_vmaf` quality mode** (FileFlows AutoCRF idea): future profile option, not in
  this port.

## Risks

- **ffprobe output drift** between ffmpeg builds could change parsed fields → pin
  behavior to jellyfin-ffmpeg and cover with fixtures.
- **Byte-exact goldens are brittle** to trivial arg-order changes → keep the builder's
  ordering deterministic and centralized; treat any goldens diff as a deliberate change.
