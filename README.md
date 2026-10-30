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

## Conversion modes (Designed is opt-in)

| | **Standard** (default) | **Designed** (opt in) | **Designed, built-in** (opt in) |
|---|---|---|---|
| Select | default · `--mode standard` | viewer **Mode** dropdown · `--mode designed` | `--mode designed-builtin` |
| Engine | our own layout engine | **an installed Chromium-based browser** (Chrome, Edge, Chromium, Brave) | our own engine |
| Input | Markdown | Markdown **with raw HTML**, and whole `.html` files — full HTML/CSS | same, but only the HTML/CSS subset below |
| Needs | nothing | a browser (auto-detected) | nothing — works offline |
| Page breaks | simple | **never cut** (below) | smart pagination (below) |

HTML is never interpreted in Standard mode: `mdpdf page.html` fails with a message telling you to opt in,
and the viewer shows the same notice until you pick a *Designed* entry.

```bash
mdpdf --mode designed examples/designed.md -o report.pdf     # Markdown + HTML + callouts, via the browser
mdpdf --mode designed examples/report.html -o report.pdf     # a styled HTML document
mdpdf --mode designed-builtin examples/designed.md           # no browser needed
mdview --mode designed examples/designed.md
```

### How Designed mode works

We don't reinvent an HTML engine here. Markdown is converted to HTML (raw HTML in the Markdown is passed through
untouched), a page setup plus a refined default theme is added, and the page is printed to PDF by your browser
in headless mode. The browser's own CSS engine therefore handles whatever the document's HTML/CSS contains;
what we add is the part browsers get wrong by default: **page transitions**.

- **Headings never end a page**: they move together with the paragraph, table, code block, list or image after them.
- **No widows or orphans**: paragraphs never leave a lone line at the top or bottom of a page.
- **Tables** that fit on a page are never split: the browser *measures* every table in a first pass (a script allowed
  only by a one-time CSP nonce), so this does not rely on guessing. Taller tables start right after the preceding text and break
  *only between rows*, repeat their header on every page, and never strand the header or a single row at either end.
- **Images, figures, code blocks, quotes, callouts, list items and `<details>`** are kept whole when they fit
  (images are also capped to the page height). Closed `<details>` are opened so their content is not lost in print.
- Page numbers ("Page n of N"), a running title and bookmarks from the headings; `<!-- pagebreak -->` and CSS
  `break-before/after` work.

These rules are enforced by tests that print real PDFs with a real browser and sweep content across many
page-bottom alignments (`crates/mdpdf-core/tests/browser.rs`, reads the PDFs with `pdftotext`). They were
verified to fail when the print rules are removed. They skip themselves when no browser/`pdftotext` is present.

The measuring pass costs one extra browser launch (~0.3 s) and only runs for documents that contain tables; if it fails
we fall back to a size estimate.

Browser lookup: `--browser PATH`, then `$MDPDF_BROWSER`, then `PATH` (`google-chrome`, `chromium`, `microsoft-edge`,
`brave-browser`, `chrome-headless-shell`, …), then the usual install locations on macOS/Windows/Linux. `MDPDF_BROWSER_ARGS`
adds extra flags and `MDPDF_BROWSER_TIMEOUT` (seconds, default 90) bounds each run. A private profile in a temp dir is
used each time so a running browser is never disturbed.

**Security and privacy.** Documents are untrusted input: a Content-Security-Policy blocks scripts, frames, plugins
and forms, so JavaScript-generated content is not supported. Remote images and stylesheets *are* fetched by the
browser (this is what makes real web designs print); use `designed-builtin` if a document must not touch the network.
The sandbox is on, except when running as root on Linux (e.g. in containers), where Chromium requires `--no-sandbox`.

The viewer's *preview* always uses the built-in engine (it labels itself "preview approximate"); **Export PDF**
prints with the browser. Use `designed-builtin` when you want preview and PDF to match exactly.

### Built-in Designed engine: smart pagination, HTML and CSS subset

The same guarantees (headings with content, no widows/orphans, tables whole or split between rows with repeated
header and no stragglers, images and callouts whole) are implemented by the built-in engine, which rolls back and
re-lays out a block when it would land badly (`crates/mdpdf-core/tests/designed.rs`).
Its HTML/CSS support is a subset: headings, `p`/`div`/`section`/`center`/`details`/`figure`/`blockquote`, lists, `dl`,
tables (`caption`/`colspan`/`align`), `pre`, `img`, links, and inline `b i u s sub sup mark kbd small code span font q`;
`<style>` blocks, `style=""`, classes/ids, descendant selectors, specificity, `var(--x)`; and the properties `color`,
`background`, `text-align`, `font-*`, `text-decoration`, `margin`, `padding`, `border`, `width`/`height`, `display:none`,
page-break properties. Not supported: flex/grid/float/positioning, pseudo-classes, attribute selectors, `calc()`,
web fonts, media queries, `rowspan`, forms, SVG/video. Unlike the browser path it is limited to Western European text.

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
- **Characters (built-in engines):** base fonts cover WinAnsi (Western European); the browser path supports any script your system fonts cover. A few symbols map to ASCII look-alikes
  (`→` becomes `->`); anything else, e.g. emoji, CJK, becomes `?`. Embedding TrueType fonts is the next step.
- Built-in engine: a single table row or code line taller than a whole page cannot be split and may overflow the margin.
- The built-in engine's viewer decodes JPEG for display with the `image` crate (viewer only; the PDF pipeline stays dependency-free);
  CMYK JPEGs show as an empty frame on screen but are correct in the PDF.
- No syntax highlighting, footnotes, math, justified text or hyphenation.
- The parser follows CommonMark closely but is not a certified implementation.

## Test

```bash
cargo test -p mdpdf-core
```
