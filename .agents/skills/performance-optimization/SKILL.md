---
name: performance-optimization
description: Profile, benchmark, and optimize startup latency (<25ms), idle RAM (<25MB), idle CPU (0%), keystroke-to-render latency (<2ms), zero-allocation viewport rendering, and large-file degradation guards. Use when optimizing hot paths or writing Criterion benchmarks in benches/.
---

# Performance Optimization Skill (`performance-optimization`)

## 1. Purpose
Enforce `TradinationalTUI (ttui)`'s strict latency, memory, and CPU budgets through zero-allocation hot paths, $O(1)$ rope structural sharing, demand-driven rendering, and quantitative Criterion benchmarks.

## 2. Relevant Architecture
- **Event Loop (`src/app/mod.rs`)**: Demand-driven `dirty` flag, `biased` input prioritization, and burst event coalescing (draining up to 64 background events per frame).
- **Text Buffer (`src/editor/document.rs`)**: `ropey::Rope` $O(\log N)$ edits + `SmallVec<[Cursor; 1]>` inline single-cursor storage.
- **Viewport Renderer (`src/ui/editor_view.rs`)**: $O(H \times W)$ bounded cell writer (never touches lines outside the visible viewport).
- **Benchmark Suite (`benches/core_benchmarks.rs`)**: Criterion benchmarks measuring startup, rope loading, single/multi-cursor typing, viewport rendering, and fuzzy matching.

## 3. Implementation Guidelines
1. **Measure Before & After**:
   - Never claim a performance improvement without running `cargo bench` and comparing Criterion median/p99 latency numbers.
2. **Eliminate Per-Frame Heap Allocations**:
   - In `src/ui/`, avoid `.to_string()`, `format!(...)`, or `Vec::new()` inside per-line or per-cell loops.
   - Reuse buffers or write slices directly into `ratatui::buffer::Buffer`.
3. **Keep Cold Startup Non-Blocking (< 15 ms)**:
   - Never spawn LSP servers, ACP agents, or full-repo file walkers synchronously before the first `terminal.draw(...)` call.
4. **Large File Guardrails (> 10 MB / Lines > 4,096 chars)**:
   - Check `doc.is_large_file()` (`rope.len_bytes() > 10 * 1024 * 1024`) to automatically bypass whole-file AST queries and keep editing latency $O(\log N)$.

## 4. Code Conventions
- Use `SmallVec` for collections that have cardinality 1 in 99% of frames (such as `CursorSet`).
- Pass `rope.clone()` ($O(1)$ atomic increment) to background tasks instead of locking `Document` or copying strings.

## 5. Dependencies
- `criterion` (dev-dependency for `benches/`)
- `smallvec`
- `ropey`
- `nucleo-matcher`

## 6. Common Mistakes to Avoid
- **Mistake**: Running a continuous `tokio::time::interval(16ms)` loop that calls `terminal.draw` even when nothing changed on screen.
  - **Fix**: Gate the tick branch on `self.has_pending_visual_tick()` and only draw when `self.dirty == true`.
- **Mistake**: Calling `rope.to_string()` to send buffer text to a background task.
  - **Fix**: Pass `rope.clone()` to the background task and iterate `rope.chunks()` off the main thread.

## 7. Testing & Benchmarking Requirements
- Run `cargo bench` and verify:
  - Single-cursor insert + undo snapshot: `< 10 µs`
  - 100-cursor batch insert: `< 250 µs`
  - Full 5-panel workbench render (`200x60` grid): `< 1.5 ms`
  - Fuzzy matching 10,000+ paths: `< 2 ms`

## 8. Project Documentation References
- [`docs/PERFORMANCE.md`](../../../docs/PERFORMANCE.md)
- [`docs/EDITOR_ENGINE.md`](../../../docs/EDITOR_ENGINE.md)
