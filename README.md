# Markdown viewer + Markdown→PDF (Rust, no HTML in the middle)

Three crates, one pipeline:

```
Markdown ──parse──▶ AST ──layout──▶ positioned draw ops ──┬─▶ PDF bytes   (mdpdf CLI, mdview "Export PDF")
                                                          └─▶ screen      (mdview, painted with egui)
```

| Crate | What it is | Dependencies |
|---|---|---|
| `crates/mdpdf-core` | Markdown + HTML/CSS parsers, PNG/inflate/deflate, layout engine, PDF 1.4 writer — all hand-written | **none** |
| `crates/mdpdf-cli` (`mdpdf`) | `mdpdf notes.md -o notes.pdf` | core only |
| `crates/mdview` (`mdview`) | Native desktop viewer (Linux / macOS / Windows) | core + `eframe`/`egui` (window, GPU painting) + `rfd` (file dialogs) + `image` (JPEG preview only) |

There is no HTML, CSS, browser engine or web view anywhere. The viewer and the
PDF share one layout engine and the same font metrics, so what you see is what
you export.

## Use

```bash
cargo run --release -p mdpdf -- examples/showcase.md -o showcase.pdf
cargo run --release -p mdview -- examples/showcase.md
```

`mdpdf` options: `--mode standard|designed`, `--html`, `--base-dir`, `--page a4|letter`, `--font sans|serif`,
`--size`, `--margin`, `--no-page-numbers`, `-` for stdin/stdout.

`mdview`: Ctrl+O open · Ctrl+E export PDF · Ctrl+D dark mode · Ctrl +/−/0 zoom (or Ctrl+scroll) ·
outline panel (headings) · clickable links, `#anchors` and relative `.md` links · drag a file onto the window ·
**auto-reloads when the file changes on disk.**

As a library:

```rust
let pdf = mdpdf_core::markdown_to_pdf(&markdown, &mdpdf_core::Config::a4());
```

## Two conversion modes (you opt in)

| | **Standard** (default) | **Designed** (opt in) |
|---|---|---|
| Select | default · `mdpdf --mode standard` | viewer **Mode** dropdown · `mdpdf --mode designed` |
| Input | Markdown | Markdown **with embedded HTML**, and whole `.html` files |
| Styling | clean, simple | refined theme, accent colours, styled tables/code/quotes, GitHub `> [!NOTE]` callouts, running header + "Page n of N" footer |
| CSS | – | supported subset (below) |
| Page breaks | fills pages top to bottom | **smart pagination** (below) |

HTML is never interpreted in Standard mode: `mdpdf page.html` fails with a message telling you to opt in,
and the viewer shows the same notice until you pick *Designed* in the dropdown.

```bash
mdpdf --mode designed examples/designed.md -o report.pdf     # Markdown + HTML + callouts
mdpdf --mode designed examples/report.html -o report.pdf     # a styled HTML document
mdview --mode designed examples/designed.md
```

### Smart pagination (Designed mode)

The layout engine tries a block, and if it would land badly it rolls back and re-lays it out:

- **Headings never end a page.** A heading moves to the next page together with the content after it
  (paragraph, table, code, list or image), including chains like `h2` followed by `h3`.
- **No widows or orphans.** Paragraphs never leave a single line alone at the bottom or top of a page (code blocks: 3 lines).
- **Tables are never cut when they fit on one page.** Tables taller than a page break *only between rows*, repeat
  the header row on every page, and never leave a lone row on either side. Rows are never split.
- **Images stay whole** with their caption, and are scaled to fit the page. Callouts, quotes, `<details>` and
  short list items are kept together when they fit.
- Explicit control: `<!-- pagebreak -->`, CSS `page-break-before/after: always`, `break-inside: avoid`.
- No empty trailing page, no horizontal rule stranded at the top of a page.

These guarantees are enforced by property-style tests that sweep content across every page-bottom alignment
(`crates/mdpdf-core/tests/designed.rs`); each was verified to fail when smart pagination is switched off.

### HTML and CSS subset

No browser engine is involved; HTML and CSS are parsed by `html.rs`/`css.rs` into the same tree Markdown uses.
Supported: headings, `p`/`div`/`section`/`center`/`details`/`figure`/`blockquote`, lists (nested, checkboxes), `dl`,
tables (`thead`/`tbody`/`caption`/`colspan`/`align`), `pre`/`code`, `img` (with `width`/`height`), links, `br`/`hr`,
inline `b i u s sub sup mark kbd small code span font q`. Entities and `<style>` blocks (also inside Markdown),
`style=""` attributes, classes/ids, descendant selectors, specificity and `var(--custom)` properties.
CSS properties: `color`, `background`, `text-align`, `font-size/weight/style/family(mono)`, `text-decoration`,
`margin(-top/-bottom/-left)`, `padding`, `border`, `border-left`, `width`/`height`, `display:none`, page-break properties.
Not supported: flex/grid/float/positioning, pseudo-classes and `::before`, attribute selectors, `calc()`, web fonts,
media queries, `rowspan`, forms, SVG/video (shown as an "omitted" note). Markdown inside HTML works for block
containers such as `<div align="center">` … blank line … `</div>`.

## Images

PNG (all colour types and bit depths, Adam7 interlacing, transparency → PDF soft mask) is decoded by our own
inflate/PNG code; JPEG (baseline and progressive, gray/RGB/CMYK) is embedded into the PDF untouched. Local paths
(relative to the file, or `--base-dir`) and `data:` URIs work; remote `http(s)` images are **not fetched** and
GIF/WebP/SVG are not supported — those render as a linked `[image: alt]` placeholder. Image-only paragraphs become
centred figures (the Markdown title is the caption); images inside text flow inline; consecutive images (badges) share a row.
PDF streams are Flate-compressed by a small built-in compressor.

## Supported Markdown

Headings (ATX + setext), paragraphs, hard/soft breaks, **bold**, *italic*, ~~strikethrough~~,
`code`, fenced and indented code blocks, block quotes (nested), ordered/unordered/nested lists,
task lists, GFM tables with alignment, horizontal rules, inline/reference/auto links and bare URLs,
backslash escapes, common entities, images.

PDF output includes clickable link annotations, a nested bookmark outline built from the
headings, document title, and page numbers.

## Design notes and known limits

- **Fonts:** PDFs use the 14 standard base fonts (Helvetica/Times/Courier), so no font files are
  embedded. Widths come from the real AFM metrics (`tools/gen_font_metrics.py`). The viewer loads the
  metric-compatible system fonts (Liberation / Arial / Times New Roman / Courier New) so on-screen wrapping matches
  the PDF; without them it falls back to egui's built-in fonts and wrapping may differ slightly.
- **Characters:** base fonts cover WinAnsi (Western European). A few symbols map to ASCII look-alikes
  (`→` becomes `->`); anything else, e.g. emoji, CJK, becomes `?`. Embedding TrueType fonts is the next step.
- A single table row or code line taller than a whole page cannot be split and may overflow the margin.
- The viewer decodes JPEG for display with the `image` crate (viewer only; the PDF pipeline stays dependency-free);
  CMYK JPEGs show as an empty frame on screen but are correct in the PDF.
- No syntax highlighting, footnotes, math, justified text or hyphenation.
- The parser follows CommonMark closely but is not a certified implementation.

## Test

```bash
cargo test -p mdpdf-core
```
