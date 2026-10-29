//! Block-level parser: headings, paragraphs, fenced/indented code, quotes,
//! lists (nested, task lists), GFM tables, rules, reference definitions.

use crate::ast::*;
use crate::css::Sheet;
use crate::html;
use crate::inline::{normalize_label, parse_inlines, InlineCtx, Refs};

/// Extensions beyond plain Markdown ("Designed" mode).
#[derive(Debug, Clone, Copy, Default)]
pub struct ParseOptions {
    /// Embedded HTML blocks/inline tags and `<style>` rules.
    pub html: bool,
    /// GitHub-style `> [!NOTE]` callouts.
    pub callouts: bool,
    /// Keep embedded HTML verbatim (as `RawHtml`) for a real HTML engine to render.
    pub raw_html: bool,
}

struct Ctx<'a> {
    refs: &'a Refs,
    sheet: Option<&'a Sheet>,
    callouts: bool,
    raw_html: bool,
}

impl Ctx<'_> {
    fn html_on(&self) -> bool {
        self.sheet.is_some() || self.raw_html
    }

    fn inl(&self, s: &str) -> Vec<Inline> {
        parse_inlines(s, &InlineCtx { refs: self.refs, html: self.sheet, raw_html: self.raw_html })
    }
}

/// Parses plain Markdown (no HTML, no callouts) into a [`Document`].
pub fn parse(src: &str) -> Document {
    parse_with(src, &ParseOptions::default())
}

pub fn parse_with(src: &str, opts: &ParseOptions) -> Document {
    let text = src.replace("\r\n", "\n").replace('\r', "\n").replace('\t', "    ");
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text).to_string();
    let lines: Vec<String> = text.split('\n').map(str::to_string).collect();
    let refs = collect_refs(&lines);
    let sheet = opts.html.then(|| Sheet::parse(&html::extract_styles(&text)));
    let ctx = Ctx { refs: &refs, sheet: sheet.as_ref(), callouts: opts.callouts, raw_html: opts.raw_html };
    let blocks = parse_blocks(&lines, &ctx);
    let title = blocks.iter().find_map(|b| match b {
        Block::Heading { level: 1, content } => Some(plain_text(content)),
        _ => None,
    });
    Document { blocks, title, page_bg: None }
}

const HTML_BLOCK_TAGS: &[&str] = &[
    "address", "article", "aside", "blockquote", "body", "center", "dd", "details", "dialog", "dir", "div", "dl", "dt",
    "fieldset", "figcaption", "figure", "footer", "form", "h1", "h2", "h3", "h4", "h5", "h6", "header", "hgroup", "hr",
    "html", "li", "main", "menu", "nav", "ol", "p", "section", "summary", "table", "tbody", "thead", "tfoot", "tr", "td",
    "th", "caption", "ul", "picture",
];
const HTML_RAW_TAGS: &[&str] = &["pre", "script", "style", "svg", "video", "iframe"];

enum HtmlEnd {
    Blank,
    Comment,
    Close(String),
}

fn html_start(l: &str) -> Option<HtmlEnd> {
    if indent(l) > 3 {
        return None;
    }
    let t = l.trim_start();
    if t.starts_with("<!--") {
        return Some(HtmlEnd::Comment);
    }
    let rest = t.strip_prefix('<')?;
    let (closing, rest) = match rest.strip_prefix('/') {
        Some(r) => (true, r),
        None => (false, rest),
    };
    let name: String = rest.chars().take_while(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase();
    let after = rest[name.len()..].chars().next();
    if name.is_empty() || !matches!(after, None | Some(' ') | Some('>') | Some('/')) {
        return None;
    }
    if !closing && HTML_RAW_TAGS.contains(&name.as_str()) {
        return Some(HtmlEnd::Close(name));
    }
    HTML_BLOCK_TAGS.contains(&name.as_str()).then_some(HtmlEnd::Blank)
}

/// Matches `HtmlOpen`/`HtmlClose` markers into `Div`s.
fn fold_markers(blocks: Vec<Block>) -> Vec<Block> {
    if !blocks.iter().any(|b| matches!(b, Block::HtmlOpen { .. } | Block::HtmlClose { .. })) {
        return blocks;
    }
    let mut stack: Vec<(String, BoxStyle, Vec<Block>)> = vec![(String::new(), BoxStyle::default(), Vec::new())];
    let pop = |stack: &mut Vec<(String, BoxStyle, Vec<Block>)>| {
        let (_, style, kids) = stack.pop().unwrap();
        stack.last_mut().unwrap().2.push(Block::Div { style, blocks: kids });
    };
    for b in blocks {
        match b {
            Block::HtmlOpen { tag, style } => stack.push((tag, style, Vec::new())),
            Block::HtmlClose { tag } => {
                if let Some(pos) = stack.iter().rposition(|(t, _, _)| *t == tag) {
                    if pos > 0 {
                        while stack.len() > pos + 1 {
                            pop(&mut stack);
                        }
                        pop(&mut stack);
                    }
                }
            }
            other => stack.last_mut().unwrap().2.push(other),
        }
    }
    while stack.len() > 1 {
        pop(&mut stack);
    }
    stack.pop().unwrap().2
}

fn detect_callout(blocks: &mut Vec<Block>) -> Option<CalloutKind> {
    let Some(Block::Paragraph(p)) = blocks.first_mut() else { return None };
    let Some(Inline::Text(t)) = p.first_mut() else { return None };
    let rest = t.strip_prefix("[!")?;
    let end = rest.find(']')?;
    let kind = match rest[..end].to_ascii_uppercase().as_str() {
        "NOTE" => CalloutKind::Note,
        "TIP" => CalloutKind::Tip,
        "IMPORTANT" => CalloutKind::Important,
        "WARNING" => CalloutKind::Warning,
        "CAUTION" => CalloutKind::Caution,
        _ => return None,
    };
    *t = rest[end + 1..].trim_start().to_string();
    if t.is_empty() {
        p.remove(0);
    }
    while matches!(p.first(), Some(Inline::SoftBreak | Inline::HardBreak)) {
        p.remove(0);
    }
    if p.is_empty() {
        blocks.remove(0);
    }
    Some(kind)
}

fn collect_refs(lines: &[String]) -> Refs {
    let mut refs = Refs::new();
    let mut fence: Option<(char, usize)> = None;
    for l in lines {
        if let Some((ch, n)) = fence {
            if closes_fence(l, ch, n) {
                fence = None;
            }
            continue;
        }
        if let Some((c, n, _)) = fence_open(l) {
            fence = Some((c, n));
            continue;
        }
        if let Some((label, url)) = ref_def(l) {
            refs.entry(normalize_label(&label)).or_insert(url);
        }
    }
    refs
}

fn indent(l: &str) -> usize {
    l.len() - l.trim_start_matches(' ').len()
}

fn blank(l: &str) -> bool {
    l.trim().is_empty()
}

fn ref_def(l: &str) -> Option<(String, String)> {
    if indent(l) > 3 {
        return None;
    }
    let t = l.trim();
    let rest = t.strip_prefix('[')?;
    let close = rest.find("]:")?;
    let label = &rest[..close];
    if label.is_empty() || label.starts_with('^') {
        return None;
    }
    let after = rest[close + 2..].trim();
    let url = if let Some(r) = after.strip_prefix('<') {
        r.split('>').next()?.to_string()
    } else {
        after.split_whitespace().next()?.to_string()
    };
    Some((label.to_string(), url))
}

fn fence_open(l: &str) -> Option<(char, usize, String)> {
    if indent(l) > 3 {
        return None;
    }
    let t = l.trim_start();
    let ch = t.chars().next()?;
    if ch != '`' && ch != '~' {
        return None;
    }
    let n = t.chars().take_while(|&c| c == ch).count();
    if n < 3 {
        return None;
    }
    let info = t[n..].trim().to_string();
    if ch == '`' && info.contains('`') {
        return None;
    }
    Some((ch, n, info))
}

fn closes_fence(l: &str, ch: char, n: usize) -> bool {
    if indent(l) > 3 {
        return false;
    }
    let t = l.trim();
    t.len() >= n && t.chars().all(|c| c == ch)
}

fn atx(l: &str) -> Option<(u8, String)> {
    if indent(l) > 3 {
        return None;
    }
    let t = l.trim();
    let n = t.chars().take_while(|&c| c == '#').count();
    if n == 0 || n > 6 {
        return None;
    }
    let rest = &t[n..];
    if !rest.is_empty() && !rest.starts_with(' ') {
        return None;
    }
    let mut body = rest.trim().to_string();
    let stripped = body.trim_end_matches('#');
    if stripped.len() != body.len() && (stripped.is_empty() || stripped.ends_with(' ')) {
        body = stripped.trim().to_string();
    }
    Some((n as u8, body))
}

fn is_hr(l: &str) -> bool {
    if indent(l) > 3 {
        return false;
    }
    let t: String = l.chars().filter(|c| !c.is_whitespace()).collect();
    t.len() >= 3 && ['-', '*', '_'].iter().any(|&m| t.chars().all(|c| c == m))
}

fn setext_level(l: &str) -> Option<u8> {
    if indent(l) > 3 {
        return None;
    }
    let t = l.trim();
    if !t.is_empty() && t.chars().all(|c| c == '=') {
        Some(1)
    } else if !t.is_empty() && t.chars().all(|c| c == '-') {
        Some(2)
    } else {
        None
    }
}

#[derive(Clone, Copy, PartialEq)]
struct Marker {
    ordered: bool,
    start: u32,
    kind: char,
    /// Column where item content starts.
    content: usize,
    empty: bool,
}

fn list_marker(l: &str) -> Option<Marker> {
    let ind = indent(l);
    if ind > 3 || is_hr(l) {
        return None;
    }
    let t = &l[ind..];
    let mut chars = t.char_indices();
    let (_, first) = chars.next()?;
    let (ordered, start, kind, len) = if matches!(first, '-' | '*' | '+') {
        (false, 1, first, 1)
    } else if first.is_ascii_digit() {
        let digits = t.chars().take_while(|c| c.is_ascii_digit()).count();
        if digits > 9 {
            return None;
        }
        let d = t.chars().nth(digits)?;
        if d != '.' && d != ')' {
            return None;
        }
        (true, t[..digits].parse().ok()?, d, digits + 1)
    } else {
        return None;
    };
    let rest = &t[len..];
    if !rest.is_empty() && !rest.starts_with(' ') {
        return None;
    }
    let spaces = rest.len() - rest.trim_start_matches(' ').len();
    let empty = rest.trim().is_empty();
    let pad = if empty || spaces > 4 { 1 } else { spaces.max(1) };
    Some(Marker { ordered, start, kind, content: ind + len + pad, empty })
}

fn table_delim(l: &str) -> Option<Vec<Align>> {
    if !l.contains('-') {
        return None;
    }
    let cells = split_row(l);
    if cells.is_empty() {
        return None;
    }
    let mut out = Vec::new();
    for c in cells {
        let c = c.trim();
        let left = c.starts_with(':');
        let right = c.ends_with(':');
        let core = c.trim_matches(':');
        if core.is_empty() || !core.chars().all(|x| x == '-') {
            return None;
        }
        out.push(match (left, right) {
            (true, true) => Align::Center,
            (false, true) => Align::Right,
            _ => Align::Left,
        });
    }
    Some(out)
}

fn split_row(l: &str) -> Vec<String> {
    let t = l.trim();
    let t = t.strip_prefix('|').unwrap_or(t);
    let t = if t.ends_with('|') && !t.ends_with("\\|") { &t[..t.len() - 1] } else { t };
    let mut cells = Vec::new();
    let mut cur = String::new();
    let mut in_code = false;
    let mut it = t.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '\\' if it.peek() == Some(&'|') => {
                cur.push('|');
                it.next();
            }
            '`' => {
                in_code = !in_code;
                cur.push(c);
            }
            '|' if !in_code => cells.push(std::mem::take(&mut cur).trim().to_string()),
            _ => cur.push(c),
        }
    }
    cells.push(cur.trim().to_string());
    cells
}

/// True if `l` would start a new block that can interrupt a paragraph.
fn interrupts_paragraph(l: &str, html: bool) -> bool {
    (html && html_start(l).is_some_and(|e| !matches!(e, HtmlEnd::Comment)))
        || atx(l).is_some()
        || fence_open(l).is_some()
        || is_hr(l)
        || (indent(l) <= 3 && l.trim_start().starts_with('>'))
        || list_marker(l).is_some_and(|m| !m.empty && (!m.ordered || m.start == 1))
}

fn parse_blocks(lines: &[String], ctx: &Ctx) -> Vec<Block> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = &lines[i];
        if blank(line) {
            i += 1;
            continue;
        }
        // Fenced code.
        if let Some((ch, n, info)) = fence_open(line) {
            let ind = indent(line);
            let mut body = Vec::new();
            i += 1;
            while i < lines.len() && !closes_fence(&lines[i], ch, n) {
                let l = &lines[i];
                body.push(l[indent(l).min(ind)..].to_string());
                i += 1;
            }
            i += 1;
            out.push(Block::CodeBlock {
                lang: info.split_whitespace().next().unwrap_or("").to_string(),
                text: body.join("\n"),
            });
            continue;
        }
        // Indented code.
        if indent(line) >= 4 {
            let mut body = Vec::new();
            while i < lines.len() && (indent(&lines[i]) >= 4 || blank(&lines[i])) {
                body.push(if lines[i].len() >= 4 { lines[i][4..].to_string() } else { String::new() });
                i += 1;
            }
            while body.last().is_some_and(|l| l.trim().is_empty()) {
                body.pop();
            }
            out.push(Block::CodeBlock { lang: String::new(), text: body.join("\n") });
            continue;
        }
        if let (true, Some(end)) = (ctx.html_on(), html_start(line)) {
            let mut raw = Vec::new();
            match end {
                HtmlEnd::Blank => {
                    while i < lines.len() && !blank(&lines[i]) {
                        raw.push(lines[i].as_str());
                        i += 1;
                    }
                }
                HtmlEnd::Comment => {
                    while i < lines.len() {
                        raw.push(lines[i].as_str());
                        i += 1;
                        if raw.last().is_some_and(|l| l.contains("-->")) {
                            break;
                        }
                    }
                }
                HtmlEnd::Close(name) => {
                    let close = format!("</{name}");
                    while i < lines.len() {
                        raw.push(lines[i].as_str());
                        i += 1;
                        if raw.last().is_some_and(|l| l.to_ascii_lowercase().contains(&close)) {
                            break;
                        }
                    }
                }
            }
            let text = raw.join("\n");
            let squashed: String = text.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_ascii_lowercase();
            if squashed == "<!--pagebreak-->" || squashed == "<!--page-break-->" || squashed == "<!--newpage-->" {
                out.push(Block::PageBreak);
            } else {
                if ctx.raw_html {
                    out.push(Block::RawHtml(text));
                } else if let Some(sheet) = ctx.sheet {
                    out.extend(html::parse_fragment(&text, sheet));
                }
            }
            continue;
        }
        if let Some((level, body)) = atx(line) {
            out.push(Block::Heading { level, content: ctx.inl(&body) });
            i += 1;
            continue;
        }
        if is_hr(line) {
            out.push(Block::Rule);
            i += 1;
            continue;
        }
        if line.trim_start().starts_with('>') {
            let mut inner = Vec::new();
            while i < lines.len() {
                let l = &lines[i];
                let t = l.trim_start();
                if indent(l) <= 3 && t.starts_with('>') {
                    let r = &t[1..];
                    inner.push(r.strip_prefix(' ').unwrap_or(r).to_string());
                } else if !blank(l)
                    && inner.last().is_some_and(|p: &String| !p.trim().is_empty())
                    && !interrupts_paragraph(l, ctx.html_on())
                {
                    inner.push(l.clone());
                } else {
                    break;
                }
                i += 1;
            }
            let mut kids = parse_blocks(&inner, ctx);
            match ctx.callouts.then(|| detect_callout(&mut kids)).flatten() {
                Some(kind) => out.push(Block::Callout { kind, blocks: kids }),
                None => out.push(Block::Quote(kids)),
            }
            continue;
        }
        if let Some(m) = list_marker(line) {
            let (b, ni) = parse_list(lines, i, m, ctx);
            out.push(b);
            i = ni;
            continue;
        }
        // Table.
        if line.contains('|') && i + 1 < lines.len() {
            if let Some(aligns) = table_delim(&lines[i + 1]) {
                let header = split_row(line);
                if header.len() == aligns.len() {
                    let cols = aligns.len();
                    let mut rows = Vec::new();
                    i += 2;
                    while i < lines.len() && !blank(&lines[i]) && !interrupts_paragraph(&lines[i], ctx.html_on()) {
                        let mut cells = split_row(&lines[i]);
                        cells.resize(cols, String::new());
                        rows.push(cells.iter().map(|c| ctx.inl(c)).collect());
                        i += 1;
                    }
                    out.push(Block::Table {
                        aligns,
                        header: header.iter().map(|c| ctx.inl(c)).collect(),
                        rows,
                        caption: None,
                    });
                    continue;
                }
            }
        }
        // Reference definition lines produce no output.
        if ref_def(line).is_some() {
            i += 1;
            continue;
        }
        // Paragraph (possibly setext heading).
        let mut para: Vec<&str> = vec![line.trim_start()];
        i += 1;
        let mut setext = None;
        while i < lines.len() {
            let l = &lines[i];
            if blank(l) {
                break;
            }
            if let Some(lv) = setext_level(l) {
                setext = Some(lv);
                i += 1;
                break;
            }
            if interrupts_paragraph(l, ctx.html_on()) {
                break;
            }
            para.push(l.trim_start());
            i += 1;
        }
        let text = para.join("\n").trim_end().to_string();
        let content = ctx.inl(&text);
        out.push(match setext {
            Some(level) => Block::Heading { level, content },
            None => Block::Paragraph(content),
        });
    }
    if ctx.sheet.is_some() {
        return fold_markers(out);
    }
    out
}

fn parse_list(lines: &[String], mut i: usize, first: Marker, ctx: &Ctx) -> (Block, usize) {
    let mut items = Vec::new();
    let mut tight = true;
    loop {
        let Some(m) = list_marker(&lines[i]).filter(|m| m.ordered == first.ordered && m.kind == first.kind) else {
            break;
        };
        let mut body: Vec<String> = vec![lines[i].get(m.content..).unwrap_or("").to_string()];
        i += 1;
        while i < lines.len() {
            let l = &lines[i];
            if blank(l) {
                match lines[i..].iter().position(|x| !blank(x)) {
                    Some(p) if indent(&lines[i + p]) >= m.content => {
                        if body.len() > 1 || !body[0].trim().is_empty() {
                            tight = false;
                        }
                        body.push(String::new());
                        i += 1;
                    }
                    _ => break,
                }
            } else if indent(l) >= m.content {
                body.push(l[m.content..].to_string());
                i += 1;
            } else if body.last().is_some_and(|p| !p.trim().is_empty())
                && list_marker(l).is_none()
                && !interrupts_paragraph(l, ctx.html_on())
                && !is_hr(l)
            {
                body.push(l.trim_start().to_string());
                i += 1;
            } else {
                break;
            }
        }
        while body.last().is_some_and(|l| l.trim().is_empty()) {
            body.pop();
        }
        let mut blocks = parse_blocks(&body, ctx);
        let mut checked = None;
        if let Some(Block::Paragraph(p)) = blocks.first_mut() {
            if let Some(Inline::Text(t)) = p.first_mut() {
                for (pfx, v) in [("[ ] ", false), ("[x] ", true), ("[X] ", true)] {
                    if let Some(r) = t.strip_prefix(pfx) {
                        *t = r.to_string();
                        checked = Some(v);
                        break;
                    }
                }
            }
        }
        items.push(ListItem { checked, blocks });
        // A blank line between items makes the list loose.
        let mut j = i;
        while j < lines.len() && blank(&lines[j]) {
            j += 1;
        }
        if j > i && j < lines.len() && list_marker(&lines[j]).is_some_and(|n| n.ordered == first.ordered && n.kind == first.kind) {
            tight = false;
            i = j;
        } else if j == i && i < lines.len() {
            continue;
        } else {
            break;
        }
    }
    (
        Block::List { ordered: first.ordered, start: first.start, tight, items },
        i,
    )
}
