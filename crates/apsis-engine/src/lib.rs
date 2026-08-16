//! apsis-engine — transcode planner & ffmpeg command builder.
//!
//! Port of the pyflows `_engine`: given a [`Probe`] + `Profile`, decide the
//! plan (`plan`) and materialize the exact ffmpeg command per backend. Pure
//! logic — no orchestration, queue, or file-watch.
//!
//! See `specs/001-apsis-engine/` for the spec, plan, and data model. Modules
//! (`probe`, `config`, `plan`, `audio`, `subtitles`, `command`, `backend`,
//! `error`) are added in the Foundational phase.
