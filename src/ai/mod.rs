//! Unified AI UX orchestration.

pub mod context;
pub mod coordinator;
pub mod diff;

pub use context::{ContextAssembler, ContextBundle, ContextLimits, ContextSection};
pub use coordinator::{AiCoordinator, AiError, AiEvent, AiState, ChatEntry, ChatRole, ChatThread};
pub use diff::{DiffLine, DiffLineKind, DiffPreview};
