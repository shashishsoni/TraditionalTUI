//! Platform-isolated OS abstractions.
//!
//! All `#[cfg(target_os = ...)]` / `#[cfg(unix)]` blocks in the entire
//! codebase live inside this module.

pub mod clipboard;
pub mod fs;
pub mod paths;
pub mod process;
pub mod shell;

pub use clipboard::Clipboard;
pub use paths::PlatformPaths;
