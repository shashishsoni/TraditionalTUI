//! Criterion benchmarks for the core hot paths.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ropey::Rope;
use ttui::app::state::AppState;
use ttui::editor::{Document, EditKind};
use ttui::search::FuzzyPicker;
use ttui::ui::UiRenderer;

fn bench_rope(c: &mut Criterion) {
    let text = "fn hello() { println!(\"world\"); }\n".repeat(30_000);
    c.bench_function("rope_load_1mb", |b| {
        b.iter(|| Rope::from_str(black_box(&text)))
    });

    c.bench_function("single_cursor_typing_burst", |b| {
        b.iter(|| {
            let mut document = Document::from_text("");
            for _ in 0..1_000 {
                document.insert_text("a", EditKind::Insert).expect("insert");
            }
            black_box(document.rope.len_chars())
        })
    });

    c.bench_function("multi_cursor_100_insert", |b| {
        b.iter(|| {
            let mut document = Document::from_text("x\n".repeat(20_000));
            document.set_cursors((0..100).map(|index| index * 100));
            document
                .insert_text("y", EditKind::MultiCursor)
                .expect("insert");
            black_box(document.rope.len_chars())
        })
    });
}

fn bench_fuzzy(c: &mut Criterion) {
    let items: Vec<String> = (0..100_000)
        .map(|index| format!("src/module_{index}/file_{index}.rs"))
        .collect();
    let mut picker = FuzzyPicker::new(items);
    c.bench_function("nucleo_fuzzy_100k", |b| {
        b.iter(|| black_box(picker.filter("module_9999/file", 50)))
    });
}

fn bench_render(c: &mut Criterion) {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("main.rs"),
        "fn main() {\n    let value = 42;\n}\n".repeat(200),
    )
    .expect("write");
    let state = AppState::bootstrap(dir.path(), None).expect("state");
    let renderer = UiRenderer::new();

    for (width, height, name) in [
        (120u16, 40u16, "ui_render_120x40"),
        (200, 60, "ui_render_200x60"),
    ] {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("terminal");
        c.bench_function(name, |b| {
            b.iter(|| {
                terminal
                    .draw(|frame| renderer.render(frame, &state))
                    .expect("draw");
            })
        });
    }
}

criterion_group!(benches, bench_rope, bench_fuzzy, bench_render);
criterion_main!(benches);
