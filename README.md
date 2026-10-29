# Markdown viewer + Markdown→PDF (Rust, no HTML in the middle)

Three crates, one pipeline:

```
Markdown ──parse──▶ AST ──layout──▶ positioned draw ops ──┬─▶ PDF bytes   (mdpdf CLI, mdview "Export PDF")
                                                          └─▶ screen      (mdview, painted with egui)
```

| Crate | What it is | Dependencies |
|---|---|---|
| `crates/mdpdf-core` | Markdown parser, layout engine, PDF 1.4 writer — all hand-written | **none** |
| `crates/mdpdf-cli` (`mdpdf`) | `mdpdf notes.md -o notes.pdf` | core only |
| `crates/mdview` (`mdview`) | Native desktop viewer (Linux / macOS / Windows) | core + `eframe`/`egui` (window, GPU painting) + `rfd` (file dialogs) |

There is no HTML, CSS, browser engine or web view anywhere. The viewer and the
PDF share one layout engine and the same font metrics, so what you see is what
you export.

## Use

```bash
cargo run --release -p mdpdf -- examples/showcase.md -o showcase.pdf
cargo run --release -p mdview -- examples/showcase.md
```

`mdpdf` options: `--page a4|letter`, `--font sans|serif`, `--size 11`, `--margin 60`,
`--no-page-numbers`, `-` for stdin/stdout.

`mdview`: Ctrl+O open · Ctrl+E export PDF · Ctrl+D dark mode · Ctrl +/−/0 zoom (or Ctrl+scroll) ·
outline panel (headings) · clickable links, `#anchors` and relative `.md` links · drag a file onto the window ·
**auto-reloads when the file changes on disk.**

As a library:

```rust
let pdf = mdpdf_core::markdown_to_pdf(&markdown, &mdpdf_core::Config::a4());
```

## Supported Markdown

Headings (ATX + setext), paragraphs, hard/soft breaks, **bold**, *italic*, ~~strikethrough~~,
`code`, fenced and indented code blocks, block quotes (nested), ordered/unordered/nested lists,
task lists, GFM tables with alignment (header row repeats across pages), horizontal rules,
inline/reference/auto links and bare URLs, backslash escapes, common entities.

PDF output includes clickable link annotations, a nested bookmark outline built from the
headings, document title, and "n / N" page numbers.

## Design notes and known limits

- **Fonts:** PDFs use the 14 standard base fonts (Helvetica/Times/Courier), so no font files are
  embedded and PDFs stay small. Widths come from the real AFM metrics (`tools/gen_font_metrics.py`
  generates `metrics_data.rs`). The viewer loads the metric-compatible system fonts
  (Liberation / Arial / Times New Roman / Courier New) so on-screen wrapping matches the PDF; if
  none are installed it falls back to egui's built-in fonts and wrapping may differ slightly.
- **Characters:** base fonts cover WinAnsi (Western European). A few symbols are mapped to ASCII
  look-alikes (`→` becomes `->`); anything else, e.g. emoji, CJK, becomes `?`. Embedding TrueType
  fonts (subsetting + ToUnicode) is the natural next step.
- **Images** are rendered as a linked `[image: alt]` placeholder — decoding PNG/JPEG is not implemented yet.
- No syntax highlighting, footnotes, math, raw HTML or justified text.
- PDF streams are uncompressed.
- The parser follows CommonMark closely but is not a certified implementation; edge cases
  (e.g. complex lazy-continuation and mixed-indent lists) may differ.

## Test

```bash
cargo test -p mdpdf-core
```
