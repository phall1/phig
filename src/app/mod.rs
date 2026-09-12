//! Functional application core: state, semantic actions, transitions, and effects.

const PAGE_SIZE: usize = 256;
const PREFETCH_DISTANCE: usize = 24;

mod commands;
mod diff_tree;
mod files;
mod inspect;
mod model;
mod navigation;
mod overlay;
pub(crate) mod patch;
mod presentation;
mod reducer;
mod search;

pub use commands::palette_commands;
pub use diff_tree::{DiffTree, DiffTreeEntry};
pub use inspect::InspectState;
pub use model::*;

#[cfg(test)]
mod tests;
