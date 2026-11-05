//! Editor engine facade.

pub mod cursor;
pub mod document;
pub mod error;
pub mod fold;
pub mod history;
pub mod motion;
pub mod search;
pub mod syntax;
pub mod text;

pub use cursor::{Cursor, CursorSet};
pub use document::{
    BufferId, DiagnosticSeverity, Document, EditorDiagnostic, GutterDiff, GutterMark, LineEnding,
    TextEncoding,
};
pub use error::EditorError;
pub use fold::{FoldMap, FoldRange};
pub use history::{EditKind, UndoHistory};
pub use motion::Motion;
pub use search::{BufferSearch, find_all};
pub use syntax::{HighlightState, Span, TokenKind, language_id_for_path};
