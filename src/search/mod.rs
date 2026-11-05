//! Search engines: lock-free fuzzy file matching and streaming project search.

mod fuzzy;
mod project;

pub use fuzzy::{FuzzyMatch, FuzzyPicker};
pub use project::{SearchMatch, index_files, search_project};
