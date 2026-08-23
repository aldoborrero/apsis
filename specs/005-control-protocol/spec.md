# Feature Specification: Control & introspection protocol over NATS

**Feature Branch**: `005-control-protocol`
**Created**: 2026-08-23
**Status**: Draft
**Input**: Operator control of a running apsis deployment (pause, cancel the active
transcode, re-queue/force/retry/mark files) plus the introspection substrate ("why was
this file skipped?", live progress) — expressed as NATS messages and KV fields, usable
from the `nats` CLI, with **no** web UI in this spec.

## Constitution alignment *(read first — governance gate)* — RESOLVED, no amendment

This feature is **constitution-clean**. It adds NATS control subjects and two small KV/
message fields; apsis already uses NATS for everything (jobs, state KV, results), so this
is more of the same transport, within Principle V's *declarative, bounded* surface. It
ships **no bespoke UI**, no visual editor, and does **not** edit config (config stays
git-only TOML — recoverability preserved). Control is exercised via the `nats` CLI and,
later, by the `apsis-web` console (spec 006) — which is the artifact that *will* require a
Principle V amendment. Nothing here does.

The single invariant that governs the design: **the control surface issues *intents*; the
existing owners (worker, coordinator) validate and execute them.** A control client
(CLI or, later, the web console) only *publishes* — it never mutates media or KV terminal
state directly, so it cannot corrupt the core or bypass crash-safety.

## User Scenarios & Testing *(mandatory)*

An operator runs a single-node apsis (coordinator + worker over local NATS JetStream). A
long transcode is running; more are queued. Everything below is reachable with `nats pub`
/ `nats req` / `nats kv` — no bespoke tooling.

### User Story 1 - Pause and resume work (Priority: P1)

The operator needs the machine's GPU back (gaming, another job) and wants apsis to stop
taking new work — optionally aborting what's running — then resume later, without losing
the queue or corrupting anything.

**Why this priority**: the most-requested, highest-value, lowest-risk control. Pausing is
purely additive (a gate the worker consults) and touches no media.

**Independent Test**: set the pause control while a job runs; confirm no *new* job is
claimed while paused and the queue is intact; clear it and confirm claiming resumes — all
via `nats kv`.

**Acceptance Scenarios**:

1. **Given** a worker idle-polling, **When** pause is set (soft, global), **Then** it
   claims no new jobs; queued jobs stay in the stream untouched.
2. **Given** a worker mid-transcode, **When** pause is set (soft), **Then** the running
   transcode **finishes normally** and only the *next* claim is withheld.
3. **Given** a worker mid-transcode, **When** pause is set (hard), **Then** the running
   transcode is also aborted (as in US2) and nothing new is claimed.
4. **Given** several workers, **When** pause targets one `worker_id`, **Then** only that
   worker stops (drain one host); the others keep working.
5. **Given** a paused deployment, **When** pause is cleared, **Then** claiming resumes on
   the next poll with no restart. A pause **survives a worker restart** (it is durable
   state, not a fire-and-forget signal) and is clearly reported.

### User Story 2 - Cancel the active transcode (Priority: P2)

A transcode is obviously wrong (wrong output, too slow, wrong file) and the operator wants
to abort *this* encode now — leaving the original byte-identical — and decide whether the
file retries or is left alone.

**Why this priority**: high value but genuinely invasive — it requires the worker's
transcode to become cancellable mid-run. Depends on the safety machinery already present
(temp-file + atomic replace).

**Independent Test**: `nats req` a cancel for the running `job_id`; confirm ffmpeg is
killed, the temp discarded, the original untouched, and the file lands in the chosen
follow-on state; a cancel for a job nobody is running returns a clear "not running".

**Acceptance Scenarios**:

1. **Given** a running transcode of `job_id`, **When** cancel is requested, **Then** the
   worker kills its ffmpeg, discards the temp file, leaves the **source byte-identical**,
   and replies with confirmation.
2. **Given** cancel with disposition **defer**, **Then** the file returns to the queue and
   the next reconcile re-plans it ("skip this one now").
3. **Given** cancel with disposition **ignore**, **Then** the file is recorded
   `Skipped@version` and is **not** re-queued until its `mtime:size` changes ("don't
   transcode this").
4. **Given** a cancel for a `job_id` that no worker is running, **Then** the request
   returns a clear negative (already done / not running) and nothing changes.

### User Story 3 - Manual state control & introspection (Priority: P3)

The operator wants to nudge state: re-evaluate a file, force one the gates would skip,
retry a dead-lettered failure, mark one done — and to answer "**why** did apsis skip or
transcode this file?" without reading raw logs.

**Why this priority**: convenience/observability over the reconcile state machine; each op
is an idempotent state transition the coordinator already understands.

**Independent Test**: for each op, issue the intent and confirm the KV transitions and the
next reconcile behaves accordingly; query a skipped file and read back its decision
(`reasons`) from the KV.

**Acceptance Scenarios**:

1. **Given** a `Done` file, **When** *re-queue* is requested, **Then** its KV state is
   cleared and the next reconcile re-plans it.
2. **Given** a compliant file (in `skip_codecs`), **When** *force* is requested, **Then** a
   transcode job is enqueued for it despite `should_skip`.
3. **Given** a `Failed@version` file, **When** *retry* is requested, **Then** the failure
   is cleared and the next reconcile re-tries it.
4. **Given** any file, **When** *mark-done* is requested, **Then** it is recorded
   `Done@version` and never transcoded until it changes.
5. **Given** a file the coordinator skipped, **When** its KV entry is read, **Then** it
   carries the **decision** (`reasons`: why it was compliant/skipped) — the substrate the
   web console later renders.

### Edge Cases

- A control intent for a **path/job that does not exist** → the executor no-ops and reports
  it; never a crash.
- **Two operators** issue conflicting intents (mark-done + re-queue) → last-write-wins via
  the KV's existing CAS; no corruption.
- Coordinator/worker **down** when an intent is sent → interactive request/reply **fails
  loudly** (timeout) so the client retries; nothing is silently lost (control is
  operator-initiated, not autonomous — no durability needed).
- A **cancel** races a transcode that finishes on its own → the finish wins (atomic
  replace already committed); the cancel returns "already done".
- **Pause set, then the whole deployment restarts** → still paused (durable), surfaced
  clearly so it isn't mistaken for a stall.

## Requirements *(mandatory)*

### Functional Requirements — Pause (US1)

- **FR-001**: Pause MUST be **durable operator state the workers consult** (a KV control
  key they watch), not a fire-and-forget signal — so it survives reconnects/restarts and
  cannot be missed.
- **FR-002**: Pause MUST support **scope** (global | a specific `worker_id`) and **mode**
  (soft = finish in-flight, withhold new claims; hard = also abort the in-flight transcode
  per FR-006).
- **FR-003**: A soft pause MUST take effect by the worker's **next claim** and MUST NOT
  touch queued jobs (they remain in the stream) or any media.
- **FR-004**: Clearing the pause MUST resume claiming with **no restart**.

### Functional Requirements — Cancel active (US2)

- **FR-005**: A control client MUST be able to request cancellation of a **specific running
  `job_id`** via NATS **request/reply**, receiving confirmation or a clear "not running".
- **FR-006**: On cancel, the worker MUST **abort its ffmpeg**, discard the temp file, and
  leave the **source byte-identical** — reusing the existing temp+atomic-replace safety;
  a cancel MUST NEVER install a partial output.
- **FR-007**: Cancel MUST carry a **disposition**: *defer* (clear state → re-queued next
  reconcile) or *ignore* (`Skipped@version`, sticky until the file changes).
- **FR-008**: The worker's transcode task MUST be **cancellable mid-run** (cooperative
  cancellation) and MUST correlate a cancel to its own in-flight job only.

### Functional Requirements — State control & introspection (US3)

- **FR-009**: The following MUST be requestable and executed by the **coordinator** (the
  single reconcile owner), each an **idempotent** KV transition (+ enqueue where noted):
  *re-queue* (clear state), *force* (enqueue despite `should_skip`), *retry* (clear
  `Failed`), *mark-done* (`Done@version`).
- **FR-010**: The coordinator MUST record its **per-file decision** — the plan's `reasons`
  (why compliant/skipped) — **in the KV entry** for files it marks `Done`/skipped, so the
  reason is durable, survives restart, and is queryable without re-probing. For in-flight
  files the decision already travels in the `Job`.
- **FR-011**: The worker MUST publish **live transcode progress** (job, percent/eta) to an
  ephemeral NATS subject, so progress can be observed (by CLI now, the console later)
  without polling. Progress is fire-and-forget (core NATS) — it is only meaningful live.

### Functional Requirements — Transport & safety (all)

- **FR-012**: All control MUST travel over **NATS** — no separate HTTP control API. Live
  signals (cancel) and confirmations use **core-NATS request/reply**; pause is a **watched
  KV key**; progress is a **core-NATS subject**. No JetStream control stream (interactive
  operator actions are retried, not persisted).
- **FR-013**: Every control op MUST be expressible from the **`nats` CLI** (pub/req/kv) —
  no bespoke client required. This is the acceptance surface for this spec.
- **FR-014**: A control client MUST NOT be able to **mutate media or terminal KV state
  directly in a way that bypasses an owner** — it publishes intents; the worker/coordinator
  validate and execute. An invalid/targetless intent is a reported no-op, never a crash
  (FR-010 engine contract extends to the daemons).
- **FR-015**: Control MUST NOT touch **config**; nothing here writes `scheduler.toml`/
  `worker.toml`. Recoverability is unchanged: control state is ephemeral operational state
  (pause defaults off; progress is transient; decisions in KV are a rebuildable projection).

### Key Entities

- **Pause control key** — a KV key encoding scope+mode; watched by workers; defaults absent
  (= not paused).
- **Cancel request** — a request/reply message `{job_id, disposition}`; reply is
  confirmation | not-running.
- **State-control request** — a request/reply to the coordinator `{path, op}` where op ∈
  {requeue, force, retry, mark_done}.
- **Decision** — the plan `reasons` folded into the `StateEntry` for skipped/done files.
- **Progress event** — ephemeral `{job_id, percent, eta}` on a core-NATS subject.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: An operator can pause the deployment and confirm no new job is claimed —
  entirely from `nats kv` — within one claim cycle, with the queue intact.
- **SC-002**: A running transcode can be aborted from `nats req` with the **source left
  byte-identical** (checksum unchanged), and the file lands in the chosen defer/ignore
  state.
- **SC-003**: Every US1–US3 operation is demonstrable via the **`nats` CLI alone**, no web
  UI, no config edit.
- **SC-004**: For a skipped file, its **reason is readable from `nats kv get`** (no
  re-probe), proving the introspection substrate for spec 006.
- **SC-005**: The existing **crash-safety and reconcile invariants are unchanged** — the
  full spec 002 test suite still passes; a control client cannot produce a corrupt library
  or a partial install.
- **SC-006**: Sending any intent to a **down** owner fails loudly (timeout) and is safely
  retryable; nothing is silently dropped or later replayed.

## Assumptions

- Single-node today (one coordinator, `concurrency = 1` worker); the subject design allows
  per-`worker_id` targeting so it extends to multi-host (spec 003) unchanged.
- `async-nats` already provides core pub/sub, request/reply, and JetStream KV — no new
  dependency.
- The `nats` CLI is available in the operator's environment (it is in the dev shell).
- The web console (spec 006) is the *only* consumer that will require a constitution
  amendment; this protocol is deliberately independent of it and ships first.
