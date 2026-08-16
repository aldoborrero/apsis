# Feature Specification: Distributed transcoding across GPU hosts

**Feature Branch**: `003-distributed-transcoding`

**Created**: 2026-08-16

**Status**: Draft

**Input**: User description: "apsis distributes transcode jobs across multiple GPU hosts
(rhea VAAPI, sirius/WSL NVENC) over a durable message queue. Media is shared via NFS so
only job metadata is sent. Jobs are leased so a crashed worker's job is redelivered.
Each host uses its own backend and path mapping. Phase 2 — builds on spec 002."

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Any free GPU host processes work (Priority: P1)

With multiple GPU workers (rhea, sirius) and a queue of jobs, whichever host has free
capacity pulls and processes the next job.

**Why this priority**: the whole point of distribution — use all GPU capacity, balanced
by real availability.

**Independent Test**: enqueue N jobs with both workers up → work spreads across hosts by
capacity; with one worker down → the other drains the queue.

**Acceptance Scenarios**:

1. **Given** rhea busy and sirius idle, **When** a job is enqueued, **Then** sirius
   processes it.
2. **Given** both idle, **When** many jobs enqueue, **Then** both process concurrently up
   to their limits.

---

### User Story 2 - Leased jobs are never stranded (Priority: P1)

A crashed worker's in-flight job is redelivered to another worker and completed; the
original file is never corrupted.

**Why this priority**: reliability — a crash must not lose, strand, or corrupt work.

**Independent Test**: kill a worker mid-job → the job reappears and a peer finishes it;
the file ends compliant and intact.

**Acceptance Scenarios**:

1. **Given** a worker killed mid-encode, **When** its lease lapses, **Then** the job is
   redelivered and completed by a peer.
2. **Given** a job that always fails, **When** it exceeds max deliveries, **Then** it
   dead-letters and raises an alert (no infinite loop).

---

### User Story 3 - Per-host backend & path mapping (Priority: P2)

On heterogeneous hosts, each worker uses its own backend (rhea VAAPI+`sei`, sirius NVENC)
and maps library paths to its local mounts.

**Why this priority**: correctness on heterogeneous hardware — it is what makes sirius
usable at all.

**Independent Test**: a job on sirius uses `hevc_nvenc` and sirius's NFS mount path; the
same job on rhea uses `hevc_vaapi` and `/hdd`.

**Acceptance Scenarios**:

1. **Given** a job on sirius, **When** the command is built, **Then** it uses the NVENC
   backend and sirius's `path_map`.
2. **Given** a host lacking its declared mount at startup, **When** it registers, **Then**
   it reports unhealthy and does not transcode to a dead path.

---

### User Story 4 - Shadow-mode validation before cutover (Priority: P3)

Run apsis dry-run beside Unmanic on prod libraries; its decisions are recorded and
diffable so divergences are caught before replacing anything.

**Why this priority**: the migration safety gate — not steady-state, but essential to
trust cutover.

**Independent Test**: dry-run on a prod library → a "would transcode" report comparable
to Unmanic's decisions; nothing is written.

**Acceptance Scenarios**:

1. **Given** dry-run mode, **When** apsis reconciles a prod library, **Then** it produces
   plans + metrics and writes nothing to the library.

---

### Edge Cases

- Queue service temporarily unavailable → workers reconnect with backoff+jitter; running
  jobs continue; new work pauses.
- A worker much slower than another → pull model self-balances (no fixed assignment).
- Network partition to sirius (NetBird) → sirius stops pulling, rhea keeps working, sirius
  resumes on reconnect.
- Duplicate delivery (at-least-once) → idempotent: a job on an already-compliant file is a
  no-op.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: apsis MUST distribute jobs via a **durable work-queue that carries only
  metadata** (media is accessed via shared storage, never transferred).
- **FR-002**: Workers MUST **pull** work sized to their free capacity (no central push
  assignment).
- **FR-003**: Each running job MUST hold a **lease**; a lapsed lease (worker crash) MUST
  cause redelivery to another worker.
- **FR-004**: A job exceeding a max-delivery count MUST be **dead-lettered and alerted**,
  never retried indefinitely.
- **FR-005**: Execution MUST be **idempotent** under at-least-once delivery (a compliant
  file → no-op).
- **FR-006**: Each worker MUST use its own configured backend(s) and a per-host path map.
- **FR-007**: A worker missing its declared storage mount MUST mark itself unhealthy and
  not process jobs.
- **FR-008**: Per-worker **priority** and **schedule** MUST influence which worker is
  favored / eligible.
- **FR-009**: Coordinator and workers MUST reconnect to the queue with **exponential
  backoff + jitter**; running transcodes MUST continue across a queue outage.
- **FR-010**: apsis MUST support a **dry-run (shadow)** mode that plans + emits metrics
  without writing, for pre-cutover validation.
- **FR-011**: Distributed state (per-file status) MUST remain re-derivable by a rescan (no
  live-only state).
- **FR-012**: Alerts MUST fire for a down worker and any dead-letter event.

### Key Entities

- **Worker (node)**: id, host, backends, concurrency, priority, schedule, path_map, health.
- **Job / Result / StateEntry**: as in the data model — metadata only over the wire.
- **Work-queue**: durable, lease-based, with a dead-letter path.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: With both workers up, N jobs complete distributed across hosts; with one
  down, the other completes all N.
- **SC-002**: In a kill-a-worker-mid-job test, 100% of jobs eventually complete and **0**
  originals are corrupted.
- **SC-003**: A poison job dead-letters within the configured max deliveries and raises
  exactly one alert.
- **SC-004**: A job routed to each host uses that host's backend + path map (verifiable in
  the built command).
- **SC-005**: Dry-run on a prod library writes nothing and produces a decisions report
  diffable against Unmanic.

## Assumptions

- Media is reachable at each worker via NFS (rhea export; sirius over NetBird).
- A single durable-queue service (NATS JetStream) is available and reachable by all hosts.
- Builds on spec 002 (single-node reconcile + safe transcode + engine).
- One coordinator; not HA (a brief restart is tolerated).
