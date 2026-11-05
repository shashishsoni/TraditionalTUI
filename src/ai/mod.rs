//! Unified AI UX orchestration.

pub mod context;
pub mod coordinator;

pub use context::{ContextAssembler, ContextBundle, ContextLimits, ContextSection};
pub use coordinator::{AiCoordinator, AiError, AiEvent, AiState, ChatEntry, ChatRole, ChatThread};
