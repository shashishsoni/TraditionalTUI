//! Terminal UI presentation layer.

pub mod ai_panel;
pub mod bottom_panel;
pub mod editor_view;
pub mod explorer_view;
pub mod layout;
pub mod overlay;
pub mod renderer;
pub mod session;
pub mod status_bar;
pub mod theme;

pub use renderer::UiRenderer;
pub use session::{TerminalSession, TuiTerminal};
