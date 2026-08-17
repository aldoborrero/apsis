//! apsis-coordinator — scan, reconcile, enqueue (spec 002 US2).
//!
//! Under construction: profile-match + the reconcile decision are in; discovery
//! (walk/inotify) and the daemon loop wire them next, hence the crate-level
//! `dead_code` allow.
#![allow(dead_code)]

mod profile_match;
mod reconcile;

fn main() {
    eprintln!("apsis-coordinator: not implemented yet (see specs/002-single-node-transcode)");
}
