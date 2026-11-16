//! Helpers regarding text selection for labels and text edit.

pub mod accesskit_text;

pub(crate) mod cursor_range;
mod label_text_selection;
pub mod text_cursor_state;
pub mod visuals;

pub use cursor_range::CCursorRange;
pub use label_text_selection::LabelSelectionState;
pub use text_cursor_state::TextCursorState;
