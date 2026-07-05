//! herdr-branch-cleanup: return panes to the default branch when theirs is gone.
#![deny(clippy::cognitive_complexity)]

pub mod board;
pub mod cli;
pub mod core;
pub mod daemon;
pub mod gitio;
pub mod herdrio;
pub mod procio;
pub mod sweep;

#[cfg(test)]
pub mod testsupport;
