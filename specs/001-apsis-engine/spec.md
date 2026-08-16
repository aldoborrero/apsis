# Feature Specification: apsis-engine — transcode planner & command builder

**Feature Branch**: `001-apsis-engine`

**Created**: 2026-08-16

**Status**: Draft

**Input**: User description: "Port the existing pyflows `_engine` to a standalone Rust
crate: given a media file and a profile, decide what to transcode and produce the exact
ffmpeg command per backend, with behavior matching the Python engine."

## User Scenarios & Testing *(mandatory)*

The consumer of this library is the apsis coordinator (planning) and worker (command
building), and transitively the operator. Behavior is validated against the existing
Python `_engine` as the **oracle**.

### User Story 1 - Plan a file against a profile (Priority: P1)

Given a probed media file and a profile, the engine produces an **abstract plan**:
whether video needs re-encoding, which audio/subtitle tracks to keep / convert / drop,
stream reorder, default-track selection, retitling, and whether the file is already
compliant (skip).

**Why this priority**: this is the core decision; every downstream component consumes
the plan. Without it there is no product.

**Independent Test**: feed the fixtures from the Python `_engine` test suite and assert
the produced `FilePlan` equals the oracle's for every fixture.

**Acceptance Scenarios**:

1. **Given** an H.264 1080p file + an HEVC profile, **When** planned, **Then**
   `video.action = Encode(hevc)` and `should_skip = false`.
2. **Given** an already-HEVC file + a profile with `hevc` in `skip_codecs`, **When**
   planned, **Then** `video.action = Copy` and `should_skip = true` (absent other drift).
3. **Given** an English 5.1 EAC3 track + a profile that adds AAC stereo for `eng`,
   **When** planned, **Then** the plan keeps the 5.1 and adds an AAC stereo track with
   English as default.
4. **Given** a file whose only audio is untagged (`und`) and `keep_languages=[eng]`,
   **When** planned, **Then** audio is **not** emptied (never-drop-all safety).

---

### User Story 2 - Build the exact ffmpeg command per backend (Priority: P1)

Given a plan and a backend (VAAPI or CPU), the engine materializes the **exact ffmpeg
argument vector**, including the AMD `sei=hdr` a53_cc workaround for VAAPI and the CPU
fallback command.

**Why this priority**: a wrong command silently produces bad output or the ~40× a53_cc
slowdown; the command builder is the riskiest surface.

**Independent Test**: golden-command tests — assert the exact arg vector for a set of
(file, profile, backend) cases.

**Acceptance Scenarios**:

1. **Given** a plan with video `Encode(hevc)` + VAAPI backend, **When** built, **Then**
   the command uses `hevc_vaapi` with `-sei hdr` and the correct HW/SW decode path for
   the source codec.
2. **Given** `fallback = cpu` and the same plan, **When** the CPU command is built,
   **Then** it uses `libx265` at the configured quality.
3. **Given** a plan with video `Copy`, **When** built, **Then** `-c:v copy` and no
   encoder options.

---

### User Story 3 - Skip already-compliant files cheaply (Priority: P2)

Given a file already matching its profile, the engine reports `should_skip` so no job
is created.

**Why this priority**: enables idempotency and avoids wasted work — but it is a
consequence of Story 1's correctness.

**Independent Test**: plan a known-compliant fixture → `should_skip = true`, no command
built.

**Acceptance Scenarios**:

1. **Given** a compliant file, **When** planned, **Then** `should_skip = true` and no
   ffmpeg command is produced.

---

### Edge Cases

- File with **no video stream** → plan is `unsupported`/skip, not a crash.
- Source with embedded **CEA-608/708 CC** on VAAPI → command drops `a53_cc` via
  `-sei hdr`; CC *recovery* is a separate, later-gated concern.
- **HDR** content with `hdr_policy = copy` → video is copied, not re-encoded.
- Odd/multi-label language tags and **missing titles** → handled without panic.
- **Unreadable/corrupt** file → probe error surfaced as a recoverable result, not a panic.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The engine MUST parse ffprobe output into typed streams (video/audio/
  subtitle) with codec, language, channels, title, default flag, and HDR detection.
- **FR-002**: The engine MUST compute an abstract, **backend-neutral** `FilePlan`
  (video/audio/subtitle actions, reorder, defaults, retitle, container, `should_skip`)
  from a probe + profile.
- **FR-003**: The engine MUST produce **identical planning decisions** to the existing
  Python `_engine` across its full fixture set (oracle parity).
- **FR-004**: The engine MUST materialize an exact ffmpeg argument vector for the VAAPI
  and CPU backends, and MUST emit `-sei hdr` for hevc/h264 VAAPI encodes.
- **FR-005**: The engine MUST choose the HW-decode vs SW-decode path per source codec
  for VAAPI.
- **FR-006**: The engine MUST never emit a plan that drops **all** audio (fallback:
  keep the originals).
- **FR-007**: The engine MUST treat already-target codecs (`skip_codecs`) and HDR
  (`hdr_policy = copy`) as copy/skip, not re-encode.
- **FR-008**: The engine MUST be a **standalone library crate** with no orchestration,
  file-watch, or queue dependencies.
- **FR-009**: The engine MUST expose a `Backend` abstraction so new backends (NVENC, …)
  materialize the same plan **without changing planning logic**.
- **FR-010**: The engine MUST surface probe/parse errors as recoverable results, never
  panics.

### Key Entities

- **FilePlan**: the abstract decision — video/audio/subtitle plans, container,
  `should_skip`. The coordinator→worker contract.
- **Profile**: desired end-state (codecs, quality, languages, commentary, stereo,
  subtitle formats, container). Supplied by the caller.
- **Probe / StreamInfo**: typed ffprobe output.
- **Backend**: turns a `FilePlan` into a concrete `FfmpegCommandSpec` (VAAPI first,
  CPU fallback; NVENC later).
- **FfmpegCommandSpec**: inputs, global args, per-stream maps, output path.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: 100% of the Python `_engine` test fixtures yield an **identical** FilePlan
  through the Rust engine.
- **SC-002**: For the golden-command set, the built ffmpeg arg vector matches the
  expected vector exactly, per backend.
- **SC-003**: Planning a compliant file yields `should_skip = true` for 100% of
  compliant fixtures (zero false "needs work").
- **SC-004**: The crate builds as a standalone `apsis-engine` with zero orchestration
  dependencies.

## Assumptions

- jellyfin-ffmpeg / ffprobe are the reference binaries; behavior is matched to them.
- The Python `_engine` (branch `feat/unmanic-integration`) is the behavioral oracle at
  port time.
- Output codecs HEVC/AV1 and backends VAAPI/CPU are in scope; NVENC is a later `Backend`
  impl (spec 003).
- Profiles are provided by the caller; config parsing/validation is the coordinator's
  concern (spec 002).
- CC recovery (the parked `_RECOVER_CC` pre-pass) is out of scope for this spec.
