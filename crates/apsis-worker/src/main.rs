//! apsis-worker — pull a job, run ffmpeg, verify, atomically replace (spec 002 US1).
//!
//! Under construction: the run/verify/pull-loop modules land incrementally; until
//! the pull loop wires them together (T016), some module fns are not yet called
//! from `main`, hence the crate-level `dead_code` allow below.
#![allow(dead_code)]

mod replace;
mod verify;

fn main() {
    eprintln!("apsis-worker: not implemented yet (see specs/002-single-node-transcode)");
}
