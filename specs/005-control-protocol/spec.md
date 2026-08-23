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

The single invariant that governs the design: **the control surface issues *intents* over
NATS subjects; the existing owners (worker, coordinator) are the sole writers of state and
execute the intents.** A control client (CLI or, later, the web console) only *publishes* to
control subjects — the `transcode_state` KV is read-only to operators, enforced by NATS
subject permissions (FR-014) — so it cannot mutate media or terminal state directly, corrupt
the core, or bypass crash-safety.

**Recoverability (Principle I) is preserved with one deliberate, named exception.** Every
transcode state stays re-derivable by rescan, and the persisted per-file *decision* is a
rebuildable projection. The single piece of operator intent a rescan cannot reconstruct — an
*ignore* ("never transcode this") — is therefore kept as a **recoverable on-disk marker**
beside the file (FR-016), not a live-only KV variant, so a from-scratch rebuild still
converges. This is called out rather than waved away.

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
3. **Given** cancel with disposition **ignore**, **Then** a **recoverable** on-disk skip
   marker is set (FR-016) and the file is **not** re-queued until its `mtime:size` changes
   ("don't transcode this") — surviving even a KV wipe.
4. **Given** a cancel for a `job_id` that no worker is running, **Then** the request
   returns a clear negative (already done / not running) and nothing changes.

### User Story 3 - Manual state control & introspection (Priority: P3)

The operator wants to nudge state: re-evaluate a file, force one the gates would skip,
retry a dead-lettered failure, mark one done — and to answer "**why** did apsis skip or
transcode this file?" without reading raw logs.

**Why this priority**: convenience/observability over the reconcile state machine; each op
is an idempotent state transition the coordinator already understands.

**Independent Test**: for each op, issue the intent and confirm the KV transitions and the
next reconcile behaves accordingly; query a skipped file and read back its **decision**
(the positive skip reason) from the KV.

**Acceptance Scenarios**:

1. **Given** a `Done` file, **When** *re-queue* is requested, **Then** its KV state is
   cleared and the next reconcile re-plans it.
2. **Given** a compliant file (in `skip_codecs`), **When** *force* is requested, **Then** a
   transcode job is enqueued for it via the plan-level `force` override that suppresses
   `should_skip` (FR-010).
3. **Given** a `Failed@version` file, **When** *retry* is requested, **Then** the failure
   is cleared and the next reconcile re-tries it.
4. **Given** any file, **When** *mark-done* is requested, **Then** it is recorded
   `Done@version` and never transcoded until it changes.
5. **Given** a file the coordinator skipped, **When** its KV entry is read, **Then** it
   carries the **decision** — the *positive* skip reason (which gate / already-compliant
   codec), emitted by the engine (FR-011) — the substrate the web console later renders.

### Edge Cases

- A control intent for a **path/job that does not exist** → the executor no-ops and reports
  it; never a crash.
- **Two operators** issue conflicting intents (mark-done + re-queue) → resolved by the
  coordinator as the **single serialized writer** of its KV keys (FR-015); last-intent-wins,
  no clobber, and both are in the audit log (FR-017).
- A **control op races the reconcile pass** on the same key → they do not race: control ops
  run serialized with the reconcile loop (FR-015), not concurrently.
- Coordinator/worker **down** when an intent is sent → interactive request/reply **fails
  loudly** (timeout) so the client retries; nothing is silently lost (control is
  operator-initiated, not autonomous — no durability needed).
- A **cancel** races a transcode that finishes on its own → the finish wins (atomic
  replace already committed); the cancel returns "already done".
- **Pause set, then the whole deployment restarts** → still paused (durable), surfaced
  clearly so it isn't mistaken for a stall.

## Requirements *(mandatory)*

### Functional Requirements — Pause (US1)

- **FR-001**: Pause MUST be **durable operator state that the target owner persists** (to a
  control key it owns) — not written by the client and not a fire-and-forget signal. The
  worker MUST re-read the pause state on **reconnect**, not solely via the live watch (a
  watch established after a gap can miss the set), so a pause cannot be missed and survives
  restart.
- **FR-002**: Pause MUST support **scope** (global | a specific `worker_id`) and **mode**
  (soft = finish in-flight, withhold new claims; hard = also abort the in-flight transcode
  per FR-006).
- **FR-003**: A soft pause MUST take effect by the worker's **next claim** and MUST NOT
  touch queued jobs (they remain in the stream) or any media.
- **FR-004**: Clearing the pause MUST resume claiming with **no restart**.

### Functional Requirements — Cancel active (US2)

- **FR-005**: A control client MUST be able to request cancellation of a **specific running
  `job_id`** via NATS **request/reply**, receiving confirmation or a clear "not running".
- **FR-006**: On cancel, the worker MUST **kill its ffmpeg child process explicitly** (not
  by dropping the transcode future — the worker's `process()` is deliberately
  non-cancellable precisely because dropping it would orphan the child / leave the source
  replaced but the KV unwritten), discard the temp file, and leave the **source
  byte-identical**. A cancel that arrives **after `atomic_replace` has begun** is too late
  and MUST be treated as "already done". A cancel MUST NEVER install a partial output.
- **FR-007**: Cancel MUST carry a **disposition**: *defer* (a fresh reconcile re-plans the
  file — see FR-008 on why this is a re-plan, not a redelivery) or *ignore* (a recoverable
  operator skip — see FR-016).
- **FR-008**: The worker MUST interrupt its in-flight transcode **by killing the child**,
  correlate a cancel to its own in-flight job only, and **`ack` (or `term`) the cancelled
  job's JetStream message — NEVER `nak` it**. A `nak` would redeliver and re-transcode the
  same job, defeating the cancel. *defer* therefore re-evaluates the file through the next
  reconcile pass, not by redelivering the cancelled message.

### Functional Requirements — State control & introspection (US3)

- **FR-009**: *re-queue*, *retry*, and *mark-done* MUST be requestable and executed by the
  **coordinator** as **idempotent** KV transitions: re-queue clears the entry (the change-
  gate then misses → re-plan); retry clears a `Failed` entry; mark-done writes
  `Done@version` (computing the `mtime:size` token itself if the file was never probed).
- **FR-010**: *force* MUST NOT be modelled as a KV transition — clearing the KV only
  re-plans, which recomputes `should_skip=true` and re-marks the file `Done`. Force requires
  a **plan-level override**: the control request carries a `force` flag that the coordinator
  threads into `plan()` to suppress `should_skip`, so an already-compliant file is transcoded
  to the profile's target. This is an `apsis-engine` signature change and MUST be specified
  as such (not grouped with the trivial KV ops).
- **FR-011**: The **engine** MUST emit a **positive skip/compliance decision** — which gate
  (`skip_if_*`) or already-compliant codec caused a skip. Today `reasons` is populated only
  for *changes-required* files (an empty `reasons` is what *defines* compliance), so a
  compliant or gate-skipped file carries no reason. The coordinator MUST persist this
  decision **as a new, permissive type** in the `StateEntry` (NOT the strict
  `deny_unknown_fields` `PlanReason`/`FilePlan`, whose reuse would break old consumers one
  level deep) — durable, queryable without re-probe. For in-flight files the plan already
  travels in the `Job`.
- **FR-012**: Emitting **live per-file progress** is **new worker work** (today the worker
  only sends JetStream lease heartbeats, not a progress feed). Progress MUST be
  `speed`/`eta`/`out_time` on an ephemeral core-NATS subject — **not `percent`**, which
  needs a source duration the probe does not yet carry (`duration` is currently `0.0`).

### Functional Requirements — Transport, ownership & safety (all)

- **FR-013**: All control MUST travel over **NATS subjects** (pub / request-reply) — no HTTP
  control API, and **no JetStream control stream** (interactive operator actions are retried
  on failure, not persisted). Live signals and confirmations use **core-NATS request/reply**.
- **FR-014**: The **owners MUST be the sole writers** of the `transcode_state` KV and of any
  durable control state (pause). Operators publish to **control subjects only**; they MUST
  NOT write the state KV directly. This MUST be enforced by **NATS subject permissions**
  (operators: publish control subjects + read KV; no KV write) so "intents through owners"
  is a real guarantee, not a convention.
- **FR-015**: State-control ops MUST be **serialized with the reconcile loop** (the
  coordinator is the single writer of its KV keys) — a control op and a reconcile pass MUST
  NOT race on the same key. (The reconcile path writes `Done`/`Failed` with unconditional
  `put()`, so an unsynchronized second writer would clobber.)
- **FR-016**: The *ignore* disposition (and any operator "never transcode this") MUST be
  **recoverable**, not a new ephemeral `Status`. It is operator intent a rescan cannot
  reconstruct, so it MUST be durable, non-live-only state — an **on-disk marker beside the
  file** (git/FS-recoverable) — and the reconcile change-gate MUST consult it (extend
  `is_handled`/the gate). Recording it as a plain new KV `Status` variant would be lost on a
  KV wipe and violate Principle I (full rebuild convergence).
- **FR-017**: Every control op MUST be **logged** (op, target, actor-if-known, timestamp)
  and **counted** (`apsis_control_ops_total{op}`) — an audit trail, since conflicting ops
  resolve last-write-wins.
- **FR-018**: Control MUST NOT touch **config**; nothing here writes `scheduler.toml`/
  `worker.toml`. An invalid/targetless intent MUST be a reported no-op, never a crash.

### Key Entities

- **Pause state** — durable control state persisted by the owner (per scope+mode); operators
  set/clear it via a control subject, never by writing the KV.
- **Cancel request** — request/reply `{job_id, disposition}`; reply is confirmation |
  not-running | already-done.
- **State-control request** — request/reply to the coordinator `{path, op, force?}`,
  op ∈ {requeue, force, retry, mark_done}.
- **Decision** — a **new permissive type** holding the positive skip/compliance reason,
  folded into `StateEntry` for skipped/done files.
- **Ignore marker** — an on-disk, recoverable "never transcode" marker the gate consults.
- **Progress event** — ephemeral `{job_id, speed, eta, out_time}` on a core-NATS subject.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: An operator can pause the deployment (via a `nats` control subject) and
  confirm no new job is claimed within one claim cycle, with the queue intact.
- **SC-002**: A running transcode can be aborted from `nats req` with the **source left
  byte-identical** (checksum unchanged), and the file lands in the chosen defer/ignore
  state.
- **SC-003**: Every US1–US3 operation is demonstrable via the **`nats` CLI alone**, no web
  UI, no config edit.
- **SC-004**: For a skipped file, its **reason is readable from `nats kv get`** (no
  re-probe), proving the introspection substrate for spec 006.
- **SC-005**: The existing **crash-safety and reconcile invariants are unchanged** — the
  full spec 002 test suite still passes; a control client cannot produce a corrupt library
  or a partial install. An added integration test MUST exercise a **control op interleaved
  with a reconcile pass** (pause/cancel/mark-done concurrent with reconcile) and assert no
  KV clobber (FR-015).
- **SC-006**: Sending any intent to a **down** owner fails loudly (timeout) and is safely
  retryable; nothing is silently dropped or later replayed.

## Assumptions

- Single-node today (one coordinator, `concurrency = 1` worker); the subject design allows
  per-`worker_id` targeting so it extends to multi-host (spec 003) unchanged.
- `async-nats` already provides core pub/sub, request/reply, and JetStream KV — no new
  dependency.
- The `nats` CLI is available in the operator's environment (it is in the dev shell).
- **Auth asymmetry is accepted:** the CLI path is trusted via mesh-only NATS credentials
  (no per-op auth), while the web console (spec 006) gates the same actions behind SSO. Both
  face the same threat model (control = mutation); the CLI is protected by *possessing NATS
  creds on the mesh*, the web by OIDC. NATS subject permissions (FR-014) still stop even a
  credentialed operator from writing the state KV directly.
- Two engine-side prerequisites are in-scope new work, not givens: the **positive skip
  decision** (FR-011) and the **`force` plan override** (FR-010) are `apsis-engine` changes;
  and the worker gains a **progress publisher** (FR-012) and **child-kill cancellation**
  (FR-006/008).
- The web console (spec 006) is the *only* consumer that will require a constitution
  amendment; this protocol is deliberately independent of it and ships first.
