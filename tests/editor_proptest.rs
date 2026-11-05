//! Property-based invariants for the rope editing engine.

use proptest::prelude::*;
use ttui::editor::{Document, EditKind};

proptest! {
    #[test]
    fn typing_then_undo_redo_round_trips(chars in prop::collection::vec(any::<char>(), 0..96)) {
        let mut document = Document::from_text("");
        let mut typed = String::new();
        for ch in chars {
            document
                .insert_text(&ch.to_string(), EditKind::Insert)
                .expect("insert");
            typed.push(ch);
        }
        prop_assert_eq!(document.rope.to_string(), typed.clone());

        while document.undo() {}
        prop_assert_eq!(document.rope.to_string(), String::new());

        while document.redo() {}
        prop_assert_eq!(document.rope.to_string(), typed);
    }

    #[test]
    fn multi_cursor_ranges_are_sorted_and_disjoint(
        a in 0usize..500,
        b in 0usize..500,
        c in 0usize..500,
    ) {
        let mut document = Document::from_text("abcdefghij".repeat(60));
        document.set_cursors([a, b, c]);
        let ranges: Vec<_> = document
            .cursors
            .as_slice()
            .iter()
            .map(|cursor| (cursor.start(), cursor.end()))
            .collect();
        for pair in ranges.windows(2) {
            prop_assert!(pair[0].0 <= pair[1].0);
            prop_assert!(pair[0].1 <= pair[1].0);
        }
        for (start, end) in &ranges {
            prop_assert!(start <= end);
            prop_assert!(*end <= document.rope.len_chars());
        }
    }

    #[test]
    fn selection_replacement_is_undoable(text in "[a-z\\n]{0,64}") {
        let mut document = Document::from_text(&text);
        let original = document.rope.to_string();
        document.select_all();
        document
            .insert_text("replacement", EditKind::Replace)
            .expect("replace");
        prop_assert_eq!(document.rope.to_string(), "replacement");
        prop_assert!(document.undo());
        prop_assert_eq!(document.rope.to_string(), original);
    }

    #[test]
    fn cursor_positions_never_exceed_buffer(positions in prop::collection::vec(0usize..10_000, 1..8)) {
        let mut document = Document::from_text("hello\nworld\n");
        document.set_cursors(positions);
        for cursor in document.cursors.as_slice() {
            prop_assert!(cursor.head <= document.rope.len_chars());
            prop_assert!(cursor.anchor <= document.rope.len_chars());
        }
    }
}
