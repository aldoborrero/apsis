# Feature Specification: Single-node library reconcile & safe transcode

**Feature Branch**: `002-single-node-transcode`

**Created**: 2026-08-16

**Status**: Draft

**Input**: User description: "apsis watches configured libraries on a single host
(rhea), reconciles each file against its profile using the engine, transcodes
non-compliant files (VAAPI with CPU fallback), and atomically replaces originals only
after verification. Idempotent, crash-safe, observable. Phase 1 — no distribution."

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Non-compliant file is transcoded and safely replaced (Priority: P1)

A watched library contains a file that does not match its profile; apsis transcodes it
and replaces the original **only after** the new output is verified.

**Why this priority**: this is the core value — an automatic, *safe* library transcode.

**Independent Test**: drop a non-compliant sample into a test library; after a cycle the
file is compliant and intact (plays, correct streams) and the original is replaced.

**Acceptance Scenarios**:

1. **Given** an H.264 file in a library mapped to an HEVC profile, **When** apsis runs,
   **Then** the file becomes HEVC and plays correctly.
2. **Given** the output fails verification (missing streams / truncated), **When** the
   job finishes, **Then** the original is left untouched and the job is marked failed.
3. **Given** VAAPI encode fails, **When** the job runs, **Then** apsis falls back to CPU
   and still produces a valid output.

---

### User Story 2 - Idempotent reconcile (Priority: P1)

A compliant library produces no jobs; running again with no changes does no work.

**Why this priority**: prevents re-transcoding the whole library each scan (Unmanic's
re-probe cost) and guarantees convergence.

**Independent Test**: reconcile a compliant library twice → 0 jobs both times; reconcile
a mixed library → only the non-compliant files are queued.

**Acceptance Scenarios**:

1. **Given** a compliant library, **When** reconciled, **Then** zero jobs are created.
2. **Given** a file already processed (now compliant), **When** the next scan runs,
   **Then** it is skipped, not re-queued.
3. **Given** an unchanged file previously seen, **When** reconciled, **Then** it is not
   re-probed (the change-token cache short-circuits).

---

### User Story 3 - Crash-safe (Priority: P2)

An interrupted transcode (killed process, power loss) never corrupts the original; the
file is retried on the next cycle.

**Why this priority**: protects the library and underpins trust — but it is a
consequence of the atomic-replace design.

**Independent Test**: kill the transcode mid-run → original intact; next cycle retries
and completes.

**Acceptance Scenarios**:

1. **Given** a transcode killed mid-encode, **When** apsis restarts, **Then** the
   original is intact and the partial output is discarded.
2. **Given** a repeatedly-failing file, **When** it exceeds the retry limit, **Then** it
   is recorded as failed and not retried until it changes.

---

### User Story 4 - Observable without a UI (Priority: P3)

apsis exposes metrics (queue depth, throughput, successes/failures, bytes saved) for the
existing Grafana stack; there is no bespoke web UI.

**Why this priority**: operability for running in prod — not required for the core loop.

**Independent Test**: scrape the metrics endpoint → the documented series are present and
update as jobs run.

**Acceptance Scenarios**:

1. **Given** jobs are running, **When** the metrics endpoint is scraped, **Then** queue
   depth, in-flight, throughput, and outcome counters are present and current.

---

### Edge Cases

- File still being written by Sonarr/Radarr → **debounce**; not queued until size-stable.
- Non-video / sidecar files (`.nfo`/`.srt`/`.jpg`) → ignored.
- Library path temporarily unavailable → scan skips gracefully, retries next cycle.
- Output larger than input / no disk space → job fails, original untouched, alert.
- Invalid config → apsis refuses to start; the last-good deployment keeps running.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: apsis MUST discover files in each configured library via periodic scan and
  filesystem events, ignoring non-video files.
- **FR-002**: apsis MUST NOT act on a file whose size is still changing within a
  configurable debounce window.
- **FR-003**: apsis MUST select a file's profile by the **longest-matching** configured
  library path.
- **FR-004**: apsis MUST decide compliance and the transcode plan via the engine
  (spec 001), acting only on drift.
- **FR-005**: apsis MUST record per-file state keyed by `path` + a change token
  (`mtime:size`) so unchanged files are neither re-probed nor re-queued.
- **FR-006**: apsis MUST transcode via VAAPI and MUST fall back to CPU on VAAPI failure.
- **FR-007**: apsis MUST write output to a temp on the **same filesystem**, verify it
  (streams/duration/not-truncated), and only then **atomically replace** the original,
  preserving ownership/mtime/permissions.
- **FR-008**: apsis MUST never leave the original corrupted on any failure; partial
  outputs MUST be discarded.
- **FR-009**: apsis MUST retry a failing file with backoff up to a limit, then mark it
  failed and suppress re-queue until the file changes.
- **FR-010**: apsis MUST bound transcode concurrency (configurable; single-session on
  AMD VCN HEVC).
- **FR-011**: apsis MUST expose Prometheus metrics and structured logs and MUST NOT ship
  a web UI.
- **FR-012**: apsis MUST fail fast on invalid configuration and keep the last-good
  deployment running.
- **FR-013**: The entire desired state MUST live in versioned config; runtime state MUST
  be re-derivable by a rescan.

### Key Entities

- **Library**: name, path, profile, extensions.
- **Profile**: desired end-state (config; consumed by the engine).
- **StateEntry**: per-file status (unknown/pending/in_progress/done/failed) + version +
  attempts.
- **Job**: one file's transcode unit (plan + paths).
- **Worker**: the local executor (VAAPI + CPU backends).

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: A non-compliant test library becomes 100% compliant after one full cycle,
  every file playable and correctly-streamed.
- **SC-002**: A second reconcile with no file changes creates **0** jobs.
- **SC-003**: In a kill-mid-transcode test, **0** originals are corrupted across N trials.
- **SC-004**: A repeatedly-failing file is retried at most the configured number of
  times, then suppressed until it changes.
- **SC-005**: All documented metrics appear in Grafana and update within one scrape
  interval of a job event.

## Assumptions

- Single host (rhea); media on a local/NFS path; no distribution in this phase.
- A local durable queue (no external broker) is acceptable for Phase 1.
- The engine (spec 001) is available (or the Python engine is shelled out as an interim).
- Runs alongside Unmanic (prod) on a separate **test** library during validation.
