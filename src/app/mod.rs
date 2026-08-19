//! Functional application core: state, semantic actions, transitions, and effects.

const PAGE_SIZE: usize = 256;
const PREFETCH_DISTANCE: usize = 24;

mod commands;
mod inspect;
mod model;
mod navigation;
mod overlay;
mod reducer;

pub use commands::palette_commands;
pub use inspect::InspectState;
pub use model::*;

#[cfg(test)]
mod tests;
