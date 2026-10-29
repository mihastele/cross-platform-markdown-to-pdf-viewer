//! Block-level parser: headings, paragraphs, fenced/indented code, quotes,
//! lists (nested, task lists), GFM tables, rules, reference definitions.

use crate::ast::*;
use crate::inline::{normalize_label, parse_inlines, Refs};

/// Parses Markdown source into a [`Document`].
pub fn parse(src: &str) -> Document {
    let text = src.replace("\r\n", "\n").replace('\r', "\n").replace('\t', "    ");
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text).to_string();
    let lines: Vec<String> = text.split('\n').map(str::to_string).collect();
    let refs = collect_refs(&lines);
    Document { blocks: parse_blocks(&lines, &refs) }
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
fn interrupts_paragraph(l: &str) -> bool {
    atx(l).is_some()
        || fence_open(l).is_some()
        || is_hr(l)
        || (indent(l) <= 3 && l.trim_start().starts_with('>'))
        || list_marker(l).is_some_and(|m| !m.empty && (!m.ordered || m.start == 1))
}

fn parse_blocks(lines: &[String], refs: &Refs) -> Vec<Block> {
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
        if let Some((level, body)) = atx(line) {
            out.push(Block::Heading { level, content: parse_inlines(&body, refs) });
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
                    && !interrupts_paragraph(l)
                {
                    inner.push(l.clone());
                } else {
                    break;
                }
                i += 1;
            }
            out.push(Block::Quote(parse_blocks(&inner, refs)));
            continue;
        }
        if let Some(m) = list_marker(line) {
            let (b, ni) = parse_list(lines, i, m, refs);
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
                    while i < lines.len() && !blank(&lines[i]) && !interrupts_paragraph(&lines[i]) {
                        let mut cells = split_row(&lines[i]);
                        cells.resize(cols, String::new());
                        rows.push(cells.iter().map(|c| parse_inlines(c, refs)).collect());
                        i += 1;
                    }
                    out.push(Block::Table {
                        aligns,
                        header: header.iter().map(|c| parse_inlines(c, refs)).collect(),
                        rows,
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
            if interrupts_paragraph(l) {
                break;
            }
            para.push(l.trim_start());
            i += 1;
        }
        let text = para.join("\n").trim_end().to_string();
        let content = parse_inlines(&text, refs);
        out.push(match setext {
            Some(level) => Block::Heading { level, content },
            None => Block::Paragraph(content),
        });
    }
    out
}

fn parse_list(lines: &[String], mut i: usize, first: Marker, refs: &Refs) -> (Block, usize) {
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
                && !interrupts_paragraph(l)
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
        let mut blocks = parse_blocks(&body, refs);
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
