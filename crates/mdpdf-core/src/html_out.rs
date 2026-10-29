//! Document tree -> HTML for the browser engine, plus the stylesheet that
//! makes the browser paginate well ("never cut" rules).
//!
//! Markdown is emitted as semantic HTML; raw HTML found in the Markdown (or a
//! whole `.html` input) is passed through verbatim so the browser's own CSS
//! engine handles it. We only add (1) a page setup + a refined default theme
//! for Markdown documents, and (2) print rules that forbid bad page breaks.

use crate::ast::*;
use crate::fonts::Family;
use crate::layout::Config;
use std::collections::HashMap;

pub fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            c => o.push(c),
        }
    }
    o
}

/// GitHub-style heading anchor.
pub fn slug(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .filter_map(|c| match c {
            ' ' => Some('-'),
            c if c.is_alphanumeric() || c == '-' || c == '_' => Some(c),
            _ => None,
        })
        .collect()
}

struct Emit {
    out: String,
    ids: HashMap<String, usize>,
    /// Body width/height in points and font size: used to guess if a table fits a page.
    geo: Option<(f32, f32, f32)>,
}

impl Emit {
    fn blocks(&mut self, blocks: &[Block], tight: bool) {
        for b in blocks {
            self.block(b, tight);
        }
    }

    fn block(&mut self, b: &Block, tight: bool) {
        match b {
            Block::Heading { level, content } => {
                let base = slug(&plain_text(content));
                let n = self.ids.entry(base.clone()).or_insert(0);
                let id = if *n == 0 { base } else { format!("{base}-{n}") };
                *n += 1;
                let l = (*level).clamp(1, 6);
                self.out.push_str(&format!("<h{l} id=\"{}\">", esc(&id)));
                self.inlines(content);
                self.out.push_str(&format!("</h{l}>\n"));
            }
            Block::Paragraph(inl) => {
                if tight {
                    self.inlines(inl);
                    self.out.push('\n');
                } else if let [Inline::Image { title: Some(t), .. }] = inl.as_slice() {
                    self.out.push_str("<figure>");
                    self.inlines(inl);
                    self.out.push_str(&format!("<figcaption>{}</figcaption></figure>\n", esc(t)));
                } else {
                    self.out.push_str("<p>");
                    self.inlines(inl);
                    self.out.push_str("</p>\n");
                }
            }
            Block::CodeBlock { lang, text } => {
                let cls = if lang.is_empty() { String::new() } else { format!(" class=\"language-{}\"", esc(lang)) };
                self.out.push_str(&format!("<pre><code{cls}>{}</code></pre>\n", esc(text)));
            }
            Block::Quote(inner) => {
                self.out.push_str("<blockquote>\n");
                self.blocks(inner, false);
                self.out.push_str("</blockquote>\n");
            }
            Block::Callout { kind, blocks } => {
                let (cls, name) = match kind {
                    CalloutKind::Note => ("note", "Note"),
                    CalloutKind::Tip => ("tip", "Tip"),
                    CalloutKind::Important => ("important", "Important"),
                    CalloutKind::Warning => ("warning", "Warning"),
                    CalloutKind::Caution => ("caution", "Caution"),
                };
                self.out.push_str(&format!("<div class=\"callout {cls}\"><p class=\"callout-title\">{name}</p>\n"));
                self.blocks(blocks, false);
                self.out.push_str("</div>\n");
            }
            Block::List { ordered, start, tight, items } => {
                let tag = if *ordered { "ol" } else { "ul" };
                let mut attrs = String::new();
                if *ordered && *start != 1 {
                    attrs.push_str(&format!(" start=\"{start}\""));
                }
                self.out.push_str(&format!("<{tag}{attrs}>\n"));
                for it in items {
                    self.out.push_str(if it.checked.is_some() { "<li class=\"task\">" } else { "<li>" });
                    match it.checked {
                        Some(done) => self.out.push_str(&format!("<input type=\"checkbox\" disabled{}> ", if done { " checked" } else { "" })),
                        None => {}
                    }
                    self.blocks(&it.blocks, *tight);
                    self.out.push_str("</li>\n");
                }
                self.out.push_str(&format!("</{tag}>\n"));
            }
            Block::Table { aligns, header, rows, caption } => {
                let tall = self.geo.is_some_and(|g| estimate_table_height(aligns.len(), header, rows, g) > 0.9 * g.1);
                self.out.push_str(if tall { "<table data-tall>\n" } else { "<table>\n" });
                if let Some(c) = caption {
                    self.out.push_str("<caption>");
                    self.inlines(c);
                    self.out.push_str("</caption>\n");
                }
                let cell = |e: &mut Emit, tag: &str, i: usize, c: &[Inline]| {
                    let a = match aligns.get(i) {
                        Some(Align::Center) => " style=\"text-align:center\"",
                        Some(Align::Right) => " style=\"text-align:right\"",
                        _ => "",
                    };
                    e.out.push_str(&format!("<{tag}{a}>"));
                    e.inlines(c);
                    e.out.push_str(&format!("</{tag}>"));
                };
                if !header.is_empty() {
                    self.out.push_str("<thead><tr>");
                    for (i, c) in header.iter().enumerate() {
                        cell(self, "th", i, c);
                    }
                    self.out.push_str("</tr></thead>\n");
                }
                self.out.push_str("<tbody>\n");
                for r in rows {
                    self.out.push_str("<tr>");
                    for (i, c) in r.iter().enumerate() {
                        cell(self, "td", i, c);
                    }
                    self.out.push_str("</tr>\n");
                }
                self.out.push_str("</tbody></table>\n");
            }
            Block::Div { blocks, .. } => {
                self.out.push_str("<div>\n");
                self.blocks(blocks, false);
                self.out.push_str("</div>\n");
            }
            Block::PageBreak => self.out.push_str("<div class=\"pagebreak\"></div>\n"),
            Block::Rule => self.out.push_str("<hr>\n"),
            Block::RawHtml(h) => {
                self.out.push_str(h);
                self.out.push('\n');
            }
            Block::HtmlOpen { .. } | Block::HtmlClose { .. } => {}
        }
    }

    fn inlines(&mut self, inl: &[Inline]) {
        for i in inl {
            match i {
                Inline::Text(t) => self.out.push_str(&esc(t)),
                Inline::Code(t) => self.out.push_str(&format!("<code>{}</code>", esc(t))),
                Inline::Emph(c) => self.wrap("em", c),
                Inline::Strong(c) => self.wrap("strong", c),
                Inline::Strike(c) => self.wrap("del", c),
                Inline::Styled { content, .. } => self.inlines(content),
                Inline::Link { url, content } => {
                    self.out.push_str(&format!("<a href=\"{}\">", esc(url)));
                    self.inlines(content);
                    self.out.push_str("</a>");
                }
                Inline::Image { url, alt, title, .. } => {
                    let t = title.as_ref().map(|t| format!(" title=\"{}\"", esc(t))).unwrap_or_default();
                    self.out.push_str(&format!("<img src=\"{}\" alt=\"{}\"{t}>", esc(url), esc(alt)));
                }
                Inline::SoftBreak => self.out.push('\n'),
                Inline::HardBreak => self.out.push_str("<br>\n"),
                Inline::RawHtml(h) => self.out.push_str(h),
            }
        }
    }

    fn wrap(&mut self, tag: &str, c: &[Inline]) {
        self.out.push_str(&format!("<{tag}>"));
        self.inlines(c);
        self.out.push_str(&format!("</{tag}>"));
    }
}

fn estimate_table_height(ncols: usize, header: &[Vec<Inline>], rows: &[Vec<Vec<Inline>>], geo: (f32, f32, f32)) -> f32 {
    let (w, _, size) = geo;
    let col_w = w / ncols.max(1) as f32;
    let mut total = 0.0;
    let row_h = |cells: &[Vec<Inline>]| -> f32 {
        let lines = cells
            .iter()
            .map(|c| ((plain_text(c).chars().count() as f32 * 0.52 * size) / (col_w - 16.0).max(20.0)).ceil().max(1.0))
            .fold(1.0, f32::max);
        lines * size * 1.55 + 11.0
    };
    if !header.is_empty() {
        total += row_h(header);
    }
    for r in rows {
        total += row_h(r);
    }
    total
}

pub fn body_html(doc: &Document) -> String {
    body_html_geo(doc, None)
}

fn body_html_geo(doc: &Document, geo: Option<(f32, f32, f32)>) -> String {
    let mut e = Emit { out: String::new(), ids: HashMap::new(), geo };
    e.blocks(&doc.blocks, false);
    e.out
}

// ---------------------------------------------------------------- CSS

fn font_stack(f: Family) -> &'static str {
    match f {
        Family::Sans => "-apple-system, \"Segoe UI\", \"Helvetica Neue\", Helvetica, Arial, \"Liberation Sans\", \"Noto Sans\", sans-serif",
        Family::Serif => "Georgia, \"Times New Roman\", Times, \"Liberation Serif\", \"Noto Serif\", serif",
        Family::Mono => "\"SF Mono\", Menlo, Consolas, \"Liberation Mono\", \"DejaVu Sans Mono\", monospace",
    }
}

fn css_str(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"").replace(['\n', '\r'], " ")
}

/// `@page` setup: size, margins, running header and page numbers.
pub fn page_css(cfg: &Config, title: Option<&str>) -> String {
    let stack = font_stack(cfg.family);
    let boxstyle = format!("font-family: {stack}; font-size: 8.5pt; color: #64707e;");
    let mut css = format!(
        "@page {{ size: {}pt {}pt; margin: {}pt {}pt {}pt {}pt;\n",
        cfg.page_width, cfg.page_height, cfg.margin_top, cfg.margin_right, cfg.margin_bottom, cfg.margin_left
    );
    if cfg.page_numbers {
        css += &format!("  @bottom-right {{ content: \"Page \" counter(page) \" of \" counter(pages); {boxstyle} }}\n");
    }
    if cfg.header {
        if let Some(t) = title {
            css += &format!("  @top-left {{ content: \"{}\"; {boxstyle} }}\n", css_str(t));
        }
    }
    css += "}\n";
    if cfg.header && title.is_some() {
        css += "@page :first { @top-left { content: none; } }\n";
    }
    css
}

/// Refined default look for Markdown documents.
pub fn designed_css(cfg: &Config) -> String {
    let stack = font_stack(cfg.family);
    let mono = font_stack(Family::Mono);
    format!(
        r#"
:root {{ --accent: #2554c7; --ink: #262c36; --heading: #0f172a; --muted: #646e7d; --rule: #dee4ec; --code-bg: #f4f6fa; }}
html {{ -webkit-print-color-adjust: exact; print-color-adjust: exact; }}
body.mdpdf {{ margin: 0; color: var(--ink); font-family: {stack}; font-size: {size}pt; line-height: {lead}; }}
.mdpdf > :first-child {{ margin-top: 0; }}
.mdpdf h1, .mdpdf h2, .mdpdf h3, .mdpdf h4, .mdpdf h5, .mdpdf h6 {{ color: var(--heading); line-height: 1.25; }}
.mdpdf h1 {{ font-size: 2.15em; margin: 0 0 .7em; }}
.mdpdf h1::after {{ content: ""; display: block; width: 42pt; height: 3pt; background: var(--accent); margin-top: .35em; }}
.mdpdf h2 {{ font-size: 1.6em; margin: 1.6em 0 .6em; padding-bottom: .2em; border-bottom: .8pt solid var(--rule); }}
.mdpdf h3 {{ font-size: 1.3em; margin: 1.4em 0 .5em; }}
.mdpdf h4 {{ font-size: 1.14em; margin: 1.3em 0 .4em; }}
.mdpdf h5, .mdpdf h6 {{ font-size: 1em; margin: 1.2em 0 .4em; }}
.mdpdf h6 {{ color: var(--muted); }}
.mdpdf p {{ margin: 0 0 .9em; }}
.mdpdf a {{ color: var(--accent); }}
.mdpdf code, .mdpdf kbd, .mdpdf samp {{ font-family: {mono}; font-size: .9em; background: var(--code-bg); padding: .08em .3em; border-radius: 3pt; }}
.mdpdf pre {{ margin: 0 0 1em; padding: 8pt 11pt; background: var(--code-bg); border: .5pt solid var(--rule); border-left: 3pt solid var(--accent); border-radius: 3pt; font-size: .85em; line-height: 1.45; white-space: pre-wrap; word-break: break-word; }}
.mdpdf pre code {{ background: none; padding: 0; font-size: 1em; }}
.mdpdf blockquote {{ margin: 0 0 1em; padding: 6pt 12pt; background: #f3f6fb; border-left: 3pt solid var(--accent); color: var(--muted); }}
.mdpdf blockquote > :last-child {{ margin-bottom: 0; }}
.mdpdf table {{ width: 100%; margin: 0 0 1.1em; }}
.mdpdf caption {{ caption-side: top; text-align: left; font-style: italic; color: var(--muted); font-size: .92em; padding-bottom: 4pt; }}
.mdpdf th {{ background: var(--accent); color: #fff; text-align: left; }}
.mdpdf th, .mdpdf td {{ padding: 5pt 8pt; border-bottom: .6pt solid var(--rule); vertical-align: top; }}
.mdpdf tbody tr:nth-child(odd) {{ background: #f8fafd; }}
.mdpdf ul, .mdpdf ol {{ margin: 0 0 .9em; padding-left: 1.7em; }}
.mdpdf li {{ margin: .22em 0; }}
.mdpdf li > ul, .mdpdf li > ol {{ margin: .2em 0 0; }}
.mdpdf li.task {{ list-style: none; margin-left: -1.35em; }}
.mdpdf li.task > input {{ margin: 0 .45em 0 0; }}
.mdpdf hr {{ border: 0; border-top: 1pt solid var(--rule); margin: 1.5em 0; }}
.mdpdf img {{ max-width: 100%; height: auto; }}
.mdpdf figure {{ margin: 1.1em 0; text-align: center; }}
.mdpdf figcaption {{ margin-top: 5pt; font-style: italic; color: var(--muted); font-size: .88em; }}
.mdpdf mark {{ background: #ffee8c; }}
.mdpdf del {{ color: var(--muted); }}
.mdpdf .callout {{ margin: 0 0 1em; padding: 8pt 12pt; border-left: 3.5pt solid var(--c); background: color-mix(in srgb, var(--c) 9%, white); }}
.mdpdf .callout > :last-child {{ margin-bottom: 0; }}
.mdpdf .callout-title {{ margin: 0 0 .3em; font-weight: 700; color: var(--c); }}
.mdpdf .note {{ --c: #2563eb; }} .mdpdf .tip {{ --c: #16964a; }} .mdpdf .important {{ --c: #9333ea; }}
.mdpdf .warning {{ --c: #c86e06; }} .mdpdf .caution {{ --c: #dc2626; }}
"#,
        size = cfg.base_size,
        lead = cfg.leading,
    )
}

/// The "never cut" rules. Appended last so they win over document styles.
pub fn print_rules(cfg: &Config) -> String {
    let max_h = (cfg.page_height - cfg.margin_top - cfg.margin_bottom - 24.0).max(100.0);
    format!(
        r#"
/* mdpdf page-break rules */
h1, h2, h3, h4, h5, h6 {{ break-after: avoid; break-inside: avoid; }}
h1 + *, h2 + *, h3 + *, h4 + *, h5 + *, h6 + * {{ break-before: avoid; }}
caption, figcaption, summary, dt {{ break-after: avoid; }}
p, li, dd, blockquote {{ orphans: 3; widows: 3; }}
pre, code {{ orphans: 3; widows: 3; }}
img, svg, video, canvas, figure, picture {{ break-inside: avoid; max-width: 100%; }}
img, svg, video, canvas {{ max-height: {max_h}pt; object-fit: contain; }}
pre, blockquote, details, figure, .callout, li, dl > div {{ break-inside: avoid; }}
table {{ border-collapse: collapse; }}
table:not([data-tall]) {{ break-inside: avoid; }}
thead {{ display: table-header-group; }}
tfoot {{ display: table-footer-group; }}
tr, th, td {{ break-inside: avoid; }}
/* A split table never strands the header, or a single row, at either end. */
tbody > tr:first-child, tbody > tr:nth-child(2), tbody > tr:last-child {{ break-before: avoid; }}
details > summary {{ display: block; }}
details {{ display: block; }}
.pagebreak {{ break-after: page; height: 0; margin: 0; padding: 0; border: 0; }}
"#
    )
}

fn find_ci(hay: &str, needle: &str) -> Option<usize> {
    hay.to_ascii_lowercase().find(needle)
}

/// Content-Security-Policy: documents may not run scripts or embed frames.
const CSP: &str = "<meta http-equiv=\"Content-Security-Policy\" content=\"script-src 'none'; object-src 'none'; frame-src 'none'; form-action 'none'\">";

fn head_snippet(base_url: Option<&str>) -> String {
    let base = base_url.map(|u| format!("<base href=\"{}\">", esc(u))).unwrap_or_default();
    format!("<meta charset=\"utf-8\">{CSP}{base}")
}

/// Browsers print closed `<details>` collapsed, hiding their content; open them.
pub fn open_details(html: &str) -> String {
    let lower = html.to_ascii_lowercase();
    let mut out = String::with_capacity(html.len() + 32);
    let mut last = 0;
    let mut from = 0;
    while let Some(rel) = lower[from..].find("<details") {
        let at = from + rel;
        let after = at + "<details".len();
        let boundary = lower[after..].chars().next().is_some_and(|c| c.is_whitespace() || c == '>' || c == '/');
        let Some(end) = lower[after..].find('>').map(|e| after + e) else { break };
        if boundary && !lower[after..end].split_whitespace().any(|a| a == "open" || a.starts_with("open=")) {
            out.push_str(&html[last..after]);
            out.push_str(" open");
            last = after;
        }
        from = end;
    }
    out.push_str(&html[last..]);
    out
}

/// Marks tables in user HTML that are surely taller than a page (`data-tall`),
/// so only the others are held together.
pub fn mark_tall_tables(html: &str, cfg: &Config) -> String {
    let body_h = cfg.page_height - cfg.margin_top - cfg.margin_bottom;
    let row_h = cfg.base_size * cfg.leading + 11.0;
    let lower = html.to_ascii_lowercase();
    let mut out = String::with_capacity(html.len() + 64);
    let (mut last, mut from) = (0, 0);
    while let Some(rel) = lower[from..].find("<table") {
        let at = from + rel;
        let after = at + "<table".len();
        let Some(close) = lower[after..].find("</table").map(|c| after + c) else { break };
        let boundary = lower[after..].chars().next().is_some_and(|c| c.is_whitespace() || c == '>');
        let rows = lower[after..close].matches("<tr").count() as f32;
        if boundary && rows * row_h > 0.9 * body_h && !lower[after..lower[after..].find('>').map_or(after, |e| after + e)].contains("data-tall") {
            out.push_str(&html[last..after]);
            out.push_str(" data-tall");
            last = after;
        }
        from = after;
    }
    out.push_str(&html[last..]);
    out
}

/// Complete HTML page for a Markdown document.
pub fn markdown_page(doc: &Document, cfg: &Config, base_url: Option<&str>) -> String {
    let title = doc.title.as_deref();
    format!(
        "<!doctype html>\n<html lang=\"en\"><head>{}<title>{}</title><style>{}{}</style></head>\n<body class=\"mdpdf\">\n{}<style>{}</style>\n</body></html>\n",
        head_snippet(base_url),
        esc(title.unwrap_or("Document")),
        page_css(cfg, title),
        designed_css(cfg),
        open_details(&body_html_geo(doc, Some((cfg.page_width - cfg.margin_left - cfg.margin_right, cfg.page_height - cfg.margin_top - cfg.margin_bottom, cfg.base_size)))),
        print_rules(cfg),
    )
}

/// A user-supplied HTML document: kept as is, with page setup before it and
/// the print rules after it.
pub fn html_page(src: &str, cfg: &Config, base_url: Option<&str>) -> String {
    let title = None::<&str>;
    let head_add = format!("{}<style>{}</style>", head_snippet(base_url), page_css(cfg, title));
    let tail_add = format!("<style>{}</style>", print_rules(cfg));
    let mut out = mark_tall_tables(&open_details(src), cfg);
    // Head: after the opening <head ...>, else after <html ...>, else prepend.
    let inserted = ["<head", "<html"].iter().find_map(|tag| {
        let at = find_ci(&out, tag)?;
        let close = out[at..].find('>')? + at + 1;
        Some((close, *tag == "<html"))
    });
    match inserted {
        Some((at, is_html)) if !is_html => out.insert_str(at, &head_add),
        Some((at, _)) => out.insert_str(at, &format!("<head>{head_add}</head>")),
        None => out = format!("<!doctype html><html><head>{head_add}</head><body>{out}</body></html>"),
    }
    match find_ci(&out, "</body>").and_then(|_| out.to_ascii_lowercase().rfind("</body>")) {
        Some(at) => out.insert_str(at, &tail_add),
        None => out.push_str(&tail_add),
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::{parse_with, ParseOptions};

    fn md(src: &str) -> String {
        body_html(&parse_with(src, &ParseOptions { html: false, callouts: true, raw_html: true }))
    }

    #[test]
    fn markdown_to_semantic_html() {
        let h = md("# Hi *there*\n\n- [x] done\n- [ ] todo\n\n| A | B |\n|:-:|--:|\n| 1 | 2 |\n\n```rust\nlet a = 1 < 2;\n```\n");
        assert!(h.contains("<h1 id=\"hi-there\">Hi <em>there</em></h1>"), "{h}");
        assert!(h.contains("<li class=\"task\"><input type=\"checkbox\" disabled checked>") && h.contains("<li class=\"task\"><input type=\"checkbox\" disabled>"), "{h}");
        assert!(h.contains("<th style=\"text-align:center\">A</th>") && h.contains("<td style=\"text-align:right\">2</td>"), "{h}");
        assert!(h.contains("class=\"language-rust\">let a = 1 &lt; 2;"), "{h}");
    }

    #[test]
    fn raw_html_passes_through_verbatim_and_text_is_escaped() {
        let h = md("<div align=\"center\">\n\n**x** & y <b style=\"color:red\">z</b>\n\n</div>\n\n1 < 2 <https://a.b>");
        assert!(h.contains("<div align=\"center\">"), "{h}");
        assert!(h.contains("<strong>x</strong> &amp; y <b style=\"color:red\">z</b>"), "{h}");
        assert!(h.contains("1 &lt; 2 <a href=\"https://a.b\">"), "{h}");
    }

    #[test]
    fn callouts_figures_and_page_breaks() {
        let h = md("> [!WARNING]\n> careful\n\n![c](a.png \"Cap\")\n\n<!-- pagebreak -->\n");
        assert!(h.contains("callout warning") && h.contains("Warning"), "{h}");
        assert!(h.contains("<figure><img src=\"a.png\" alt=\"c\" title=\"Cap\"><figcaption>Cap</figcaption></figure>"), "{h}");
        assert!(h.contains("class=\"pagebreak\""), "{h}");
    }

    #[test]
    fn details_are_opened_and_tall_tables_marked() {
        assert_eq!(open_details("<details><summary>x</summary></details>"), "<details open><summary>x</summary></details>");
        assert_eq!(open_details("<DETAILS class=a>"), "<DETAILS open class=a>");
        assert_eq!(open_details("<details open>"), "<details open>");
        assert_eq!(open_details("<detailsx>"), "<detailsx>");
        let cfg = Config::designed_a4();
        let rows: String = (0..60).map(|i| format!("<tr><td>{i}</td></tr>")).collect();
        let t = mark_tall_tables(&format!("<table>{rows}</table><table><tr><td>x</td></tr></table>"), &cfg);
        assert!(t.starts_with("<table data-tall>") && t.contains("</table><table><tr>"), "{t}");
    }

    #[test]
    fn tall_markdown_tables_are_marked() {
        let cfg = Config::designed_a4();
        let mut s = String::from("| a | b |\n|---|---|\n");
        for i in 0..80 {
            s += &format!("| {i} | x |\n");
        }
        let doc = parse_with(&s, &ParseOptions { html: false, callouts: true, raw_html: true });
        assert!(markdown_page(&doc, &cfg, None).contains("<table data-tall>"));
        let small = parse_with("| a |\n|---|\n| 1 |\n", &ParseOptions { html: false, callouts: true, raw_html: true });
        assert!(!markdown_page(&small, &cfg, None).contains("data-tall>"));
    }

    #[test]
    fn duplicate_headings_get_unique_ids() {
        let h = md("# A\n\n## A\n\n## A\n");
        assert!(h.contains("id=\"a\"") && h.contains("id=\"a-1\"") && h.contains("id=\"a-2\""), "{h}");
    }

    #[test]
    fn user_html_gets_csp_page_css_and_print_rules() {
        let cfg = Config::designed_a4();
        let out = html_page("<html><head><title>t</title></head><body><p>x</p></body></html>", &cfg, Some("file:///d/"));
        let head = out.find("<head>").unwrap();
        assert!(out[head..].starts_with("<head><meta charset"));
        assert!(out.contains("script-src 'none'") && out.contains("<base href=\"file:///d/\">"));
        assert!(out.contains("@page") && out.rfind("break-inside").unwrap() > out.find("<p>x</p>").unwrap(), "{out}");
        // Fragments without html/head still become a page.
        let frag = html_page("<h1>x</h1>", &cfg, None);
        assert!(frag.starts_with("<!doctype html>") && frag.contains("<h1>x</h1>"));
    }
}
