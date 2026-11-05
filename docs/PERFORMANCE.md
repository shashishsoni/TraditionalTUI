# Performance Requirements, Budgets & Benchmarking (`PERFORMANCE.md`)

## 1. Quantitative Performance Budgets

Performance is a hard architectural constraint for `TradinationalTUI (ttui)`. Every pull request and phase milestone is evaluated against the following measurable targets:

| Metric | Target Baseline | Hard Ceiling (Failure Threshold) | Verification Method |
|--------|-----------------|----------------------------------|---------------------|
| **Cold Startup Time (to first frame)** | **8 – 15 ms** | **25 ms** | `hyperfine` + `benches/startup_bench.rs` |
| **Idle Memory Consumption (RSS)** | **12 – 18 MB** | **25 MB** | `/proc/self/statm` / `ps -o rss=` after startup |
| **Idle CPU Utilization** | **0.0 %** | **0.1 %** | `pidstat` / `strace -c` over 60s idle window |
| **Input-to-Render Latency (p99)** | **< 2.0 ms** | **5.0 ms** | Criterion `benches/editor_bench.rs` + `ui_render_bench.rs` |
| **Viewport Scroll Frame Time (120 FPS)** | **< 1.5 ms / frame** | **4.0 ms / frame** | Criterion `benches/ui_render_bench.rs` (200x60 grid) |
| **Large File Open (50 MB source file)** | **< 45 ms** | **100 ms** | Criterion `rope_load_50mb` |
| **Multi-Cursor Edit (100 cursors)** | **< 250 µs** | **1.0 ms** | Criterion `multi_cursor_100_insert` |
| **Fuzzy File Search (100,000 paths)** | **< 8 ms** | **20 ms** | Criterion `nucleo_fuzzy_100k` |

---

## 2. Core Optimization Techniques

### 2.1 Fast Cold Startup (< 15 ms)
1. **Zero Blocking I/O Before First Frame**:
   - Startup parses CLI arguments, loads `config.toml`, opens the initial file buffer (or empty scratch buffer), initializes the terminal alternate screen, and renders Frame #1 immediately.
   - Workspace file tree walking (`ignore`), Git repository status (`gix`), LSP server spawning, and ACP agent discovery happen **asynchronously after Frame #1 is visible**.
2. **Lazy Grammar & Theme Loading**:
   - Tree-sitter grammars are initialized only for the languages of actively opened buffers, never all at once at startup.

### 2.2 Zero-Allocation Hot Render Path
1. **Viewport-Bounded Iteration**:
   - `EditorWidget::render` only inspects `rope.lines_at(top_line).take(viewport_height)`. A 1,000,000-line log file costs the exact same CPU time to render as a 50-line file.
2. **Direct `ratatui::buffer::Buffer` Cell Writing**:
   - Instead of allocating `Vec<String>` for every line on every frame, the editor renderer iterates over `RopeSlice::chunks()` and writes grapheme clusters directly into `buf.cell_mut((x, y))`.
3. **SmallVec for Single-Cursor Fast Path**:
   - `CursorSet` uses `SmallVec<[Cursor; 1]>` and `SmallVec<[HighlightSpan; 8]>` to avoid heap allocations during normal single-cursor typing.

### 2.3 Large-File Degradation Guards (> 10 MB / Long Lines)
When opening files larger than `config.performance.large_file_threshold_mb` (default `10 MB`) or lines longer than `4,096` columns (e.g., minified JS bundles):
- `ropey::Rope` editing and viewport slicing remain enabled at full speed.
- Whole-file synchronous Tree-sitter parsing and expensive LSP semantic tokens are automatically disabled or restricted to viewport-only regex highlighting, preventing UI stalls.

### 2.4 Lock-Free Background Concurrency
- **Zero `Mutex` on `Document`**: Background workers never lock the active `Document`. They receive a cheap $O(1)$ `rope.clone()` snapshot tagged with `version: u64`. When the background task finishes and sends its result via `AppEvent`, `app` checks if `doc.version == result.version` (or rebases line ranges) in $O(1)$ time.

---

## 3. Criterion Benchmark Suite (`benches/`)

`ttui` includes an automated Criterion benchmark suite configured in `Cargo.toml`:

1. **Cold Startup & State Initialization (`startup_bench`)**:
   - Measures full `Config::load` + `Workspace::new` + `AppState::new` + initial headless `UiRenderer::render` on a 200x60 terminal grid.
2. **Text Engine Benchmarks (`editor_bench`)**:
   - `rope_load_1mb` / `rope_load_50mb`: Streaming file load into `ropey::Rope`.
   - `single_cursor_typing_burst`: 1,000 sequential character insertions + undo history recording.
   - `multi_cursor_100_insert`: Simultaneous insertion across 100 cursors in a 100,000-line file.
   - `viewport_slice_scrolling`: 500-frame continuous vertical scroll through a 250,000-line file.
3. **Rendering & Diff Benchmarks (`ui_render_bench`)**:
   - Full 5-panel workbench render + `ratatui::Buffer::diff` computation at `120x40` (1080p split) and `240x80` (4K fullscreen) resolutions.
4. **Fuzzy Search Benchmarks (`search_bench`)**:
   - Filtering 100,000 synthetic repository file paths via `nucleo-matcher`.

### Running Benchmarks & Profiling
```bash
# Run the full Criterion benchmark suite
cargo bench

# Measure cold binary startup to exit with hyperfine
cargo build --release
hyperfine --warmup 5 './target/release/ttui --version'
hyperfine --warmup 5 './target/release/ttui --headless-smoke-test'

# Verify idle memory RSS
/usr/bin/time -v ./target/release/ttui --headless-smoke-test
```
