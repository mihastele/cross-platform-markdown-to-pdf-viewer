//! HTML subset -> document tree. A lenient tokenizer and tree builder (implicit
//! `p`/`li`/`td` closing, void elements, raw-text elements), then a converter
//! that resolves the CSS subset from [`crate::css`]. No browser engine: the
//! result is the same AST the Markdown parser produces, so it shares layout.

use crate::ast::*;
use crate::css::*;
use crate::inline::entity;

// ------------------------------------------------------------ tokenizer

#[derive(Debug, Clone)]
pub enum Tag {
    Open { name: String, attrs: Vec<(String, String)>, self_close: bool },
    Close(String),
    Comment,
}

pub fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let c: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < c.len() {
        if c[i] == '&' {
            if let Some((r, n)) = entity(&c[i..]) {
                out.push_str(&r);
                i += n;
                continue;
            }
        }
        out.push(c[i]);
        i += 1;
    }
    out
}

/// Parses the tag starting at `c[0] == '<'`. Returns the tag and its length.
pub fn parse_tag(c: &[char]) -> Option<(Tag, usize)> {
    if c.first() != Some(&'<') {
        return None;
    }
    let find = |from: usize, pat: &str| -> Option<usize> {
        let p: Vec<char> = pat.chars().collect();
        (from..c.len().saturating_sub(p.len() - 1)).find(|&i| c[i..i + p.len()] == p[..])
    };
    if c.starts_with(&['<', '!', '-', '-']) {
        let e = find(4, "-->").map(|e| e + 3).unwrap_or(c.len());
        return Some((Tag::Comment, e));
    }
    match c.get(1)? {
        '!' | '?' => {
            let e = c.iter().position(|&x| x == '>').map_or(c.len(), |p| p + 1);
            return Some((Tag::Comment, e));
        }
        '/' => {
            let mut i = 2;
            while i < c.len() && (c[i].is_ascii_alphanumeric() || c[i] == '-') {
                i += 1;
            }
            if i == 2 {
                return None;
            }
            let name: String = c[2..i].iter().collect::<String>().to_ascii_lowercase();
            while i < c.len() && c[i] != '>' {
                i += 1;
            }
            return (i < c.len()).then_some((Tag::Close(name), i + 1));
        }
        ch if ch.is_ascii_alphabetic() => {}
        _ => return None,
    }
    let mut i = 1;
    while i < c.len() && (c[i].is_ascii_alphanumeric() || c[i] == '-') {
        i += 1;
    }
    let name: String = c[1..i].iter().collect::<String>().to_ascii_lowercase();
    if !matches!(c.get(i), Some(x) if x.is_whitespace() || *x == '>' || *x == '/') {
        return None;
    }
    let mut attrs = Vec::new();
    loop {
        while i < c.len() && c[i].is_whitespace() {
            i += 1;
        }
        match c.get(i) {
            None => return None,
            Some('>') => return Some((Tag::Open { name, attrs, self_close: false }, i + 1)),
            Some('/') => {
                if c.get(i + 1) == Some(&'>') {
                    return Some((Tag::Open { name, attrs, self_close: true }, i + 2));
                }
                i += 1;
            }
            Some(_) => {
                let s = i;
                while i < c.len() && !c[i].is_whitespace() && !matches!(c[i], '=' | '>' | '/') {
                    i += 1;
                }
                let an: String = c[s..i].iter().collect::<String>().to_ascii_lowercase();
                while i < c.len() && c[i].is_whitespace() {
                    i += 1;
                }
                let mut val = String::new();
                if c.get(i) == Some(&'=') {
                    i += 1;
                    while i < c.len() && c[i].is_whitespace() {
                        i += 1;
                    }
                    match c.get(i) {
                        Some(&q) if q == '"' || q == '\'' => {
                            i += 1;
                            let s = i;
                            while i < c.len() && c[i] != q {
                                i += 1;
                            }
                            if i >= c.len() {
                                return None;
                            }
                            val = c[s..i].iter().collect();
                            i += 1;
                        }
                        _ => {
                            let s = i;
                            while i < c.len() && !c[i].is_whitespace() && c[i] != '>' {
                                i += 1;
                            }
                            val = c[s..i].iter().collect();
                        }
                    }
                }
                if !an.is_empty() {
                    attrs.push((an, decode_entities(&val)));
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
enum Tok {
    Text(String),
    Tag(Tag),
}

fn tokenize(src: &str) -> Vec<Tok> {
    let c: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut text = String::new();
    let mut i = 0;
    let flush = |text: &mut String, out: &mut Vec<Tok>| {
        if !text.is_empty() {
            out.push(Tok::Text(decode_entities(&std::mem::take(text))));
        }
    };
    while i < c.len() {
        if c[i] == '<' {
            if let Some((tag, n)) = parse_tag(&c[i..]) {
                flush(&mut text, &mut out);
                let raw = match &tag {
                    Tag::Open { name, self_close: false, .. } if matches!(name.as_str(), "script" | "style" | "textarea" | "title") => {
                        Some(name.clone())
                    }
                    _ => None,
                };
                if !matches!(tag, Tag::Comment) {
                    out.push(Tok::Tag(tag));
                }
                i += n;
                if let Some(name) = raw {
                    let close: Vec<char> = format!("</{name}").chars().collect();
                    let end = (i..c.len())
                        .find(|&k| c.len() >= k + close.len() && c[k..k + close.len()].iter().zip(&close).all(|(a, b)| a.to_ascii_lowercase() == *b))
                        .unwrap_or(c.len());
                    let body: String = c[i..end].iter().collect();
                    out.push(Tok::Text(if name == "title" || name == "textarea" { decode_entities(&body) } else { body }));
                    i = end;
                }
                continue;
            }
        }
        text.push(c[i]);
        i += 1;
    }
    flush(&mut text, &mut out);
    out
}

// ------------------------------------------------------------------ DOM

#[derive(Debug, Clone)]
pub enum Node {
    Text(String),
    El(Element),
}

#[derive(Debug, Clone)]
pub struct Element {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Node>,
    /// Still open at end of input (used for Markdown HTML fragments).
    pub unclosed: bool,
}

impl Element {
    pub fn attr(&self, k: &str) -> Option<&str> {
        self.attrs.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
    }
}

const VOID: &[&str] = &["area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "source", "track", "wbr"];
const BLOCK: &[&str] = &[
    "address", "article", "aside", "blockquote", "body", "center", "dd", "details", "dialog", "dir", "div", "dl", "dt",
    "fieldset", "figcaption", "figure", "footer", "form", "h1", "h2", "h3", "h4", "h5", "h6", "header", "hgroup", "hr",
    "html", "li", "main", "menu", "nav", "ol", "p", "pre", "section", "summary", "table", "ul", "tbody", "thead", "tfoot",
    "tr", "td", "th", "caption", "legend",
];
/// Containers that become Markdown-level markers when left unclosed.
const MARKER: &[&str] = &["div", "section", "article", "center", "details", "figure", "header", "footer", "main", "aside", "nav", "blockquote"];

fn is_block(n: &str) -> bool {
    BLOCK.contains(&n)
}

fn close_to(stack: &mut Vec<Element>, len: usize) {
    while stack.len() > len {
        let e = stack.pop().unwrap();
        stack.last_mut().unwrap().children.push(Node::El(e));
    }
}

fn implicit_close(stack: &mut Vec<Element>, opening: &str) {
    let find = |stack: &[Element], targets: &[&str], stops: &[&str]| -> Option<usize> {
        for (i, e) in stack.iter().enumerate().skip(1).rev() {
            if targets.contains(&e.name.as_str()) {
                return Some(i);
            }
            if stops.contains(&e.name.as_str()) {
                return None;
            }
        }
        None
    };
    let target = match opening {
        "li" => find(stack, &["li"], &["ul", "ol"]),
        "dt" | "dd" => find(stack, &["dt", "dd"], &["dl"]),
        "tr" => find(stack, &["tr"], &["table"]),
        "td" | "th" => find(stack, &["td", "th"], &["tr", "table"]),
        "thead" | "tbody" | "tfoot" => find(stack, &["thead", "tbody", "tfoot"], &["table"]),
        n if is_block(n) => find(stack, &["p"], &["td", "th", "li", "table", "caption", "button", "body", "html"]),
        _ => None,
    };
    if let Some(i) = target {
        close_to(stack, i);
    }
}

pub fn build(src: &str) -> Vec<Node> {
    let mut stack = vec![Element { name: "#root".into(), attrs: vec![], children: vec![], unclosed: false }];
    for tok in tokenize(src) {
        match tok {
            Tok::Text(t) => stack.last_mut().unwrap().children.push(Node::Text(t)),
            Tok::Tag(Tag::Comment) => {}
            Tok::Tag(Tag::Open { name, attrs, self_close }) => {
                implicit_close(&mut stack, &name);
                let el = Element { name: name.clone(), attrs, children: vec![], unclosed: false };
                if self_close || VOID.contains(&name.as_str()) {
                    stack.last_mut().unwrap().children.push(Node::El(el));
                } else {
                    stack.push(el);
                }
            }
            Tok::Tag(Tag::Close(name)) => {
                if VOID.contains(&name.as_str()) {
                    continue;
                }
                match stack.iter().rposition(|e| e.name == name) {
                    Some(i) if i > 0 => close_to(&mut stack, i + 1),
                    _ => {
                        let marker = Element { name: format!("/{name}"), attrs: vec![], children: vec![], unclosed: false };
                        stack.last_mut().unwrap().children.push(Node::El(marker));
                        continue;
                    }
                }
                // `close_to` left the matching element on top; attach it.
                let e = stack.pop().unwrap();
                stack.last_mut().unwrap().children.push(Node::El(e));
            }
        }
    }
    while stack.len() > 1 {
        let mut e = stack.pop().unwrap();
        e.unclosed = true;
        stack.last_mut().unwrap().children.push(Node::El(e));
    }
    stack.pop().unwrap().children
}

// ------------------------------------------------------------ conversion

fn elem_ref(e: &Element) -> ElemRef {
    ElemRef {
        tag: e.name.clone(),
        id: e.attr("id").unwrap_or("").to_string(),
        classes: e.attr("class").unwrap_or("").split_whitespace().map(str::to_string).collect(),
    }
}

fn text_of(nodes: &[Node], out: &mut String) {
    for n in nodes {
        match n {
            Node::Text(t) => out.push_str(t),
            Node::El(e) if e.name == "br" => out.push('\n'),
            Node::El(e) => text_of(&e.children, out),
        }
    }
}

fn find_el<'a>(nodes: &'a [Node], name: &str) -> Option<&'a Element> {
    for n in nodes {
        if let Node::El(e) = n {
            if e.name == name {
                return Some(e);
            }
            if let Some(f) = find_el(&e.children, name) {
                return Some(f);
            }
        }
    }
    None
}

fn collect_styles(nodes: &[Node], out: &mut String) {
    for n in nodes {
        if let Node::El(e) = n {
            if e.name == "style" {
                text_of(&e.children, out);
                out.push('\n');
            } else {
                collect_styles(&e.children, out);
            }
        }
    }
}

/// Concatenated contents of every `<style>` element in `src`.
pub fn extract_styles(src: &str) -> String {
    let mut s = String::new();
    collect_styles(&build(src), &mut s);
    s
}

fn has_content(inl: &[Inline]) -> bool {
    inl.iter().any(|i| match i {
        Inline::Text(t) => !t.trim().is_empty(),
        Inline::Code(_) | Inline::Image { .. } => true,
        Inline::Emph(c) | Inline::Strong(c) | Inline::Strike(c) => has_content(c),
        Inline::Styled { content, .. } | Inline::Link { content, .. } => has_content(content),
        _ => false,
    })
}

fn dim_attr(v: &str) -> Option<Dim> {
    let v = v.trim();
    if let Some(p) = v.strip_suffix('%') {
        return p.trim().parse().ok().map(Dim::Pct);
    }
    parse_pt(v).map(Dim::Pt)
}

struct Conv<'a> {
    sheet: &'a Sheet,
    anc: Vec<ElemRef>,
    fragment: bool,
    scale: f32,
}

impl Conv<'_> {
    fn css(&self, e: &Element) -> Css {
        let mut c = self.sheet.compute(&elem_ref(e), &self.anc, e.attr("style"));
        // Legacy presentational attributes (lower priority than CSS).
        if c.align.is_none() {
            c.align = match e.attr("align").map(str::to_ascii_lowercase).as_deref() {
                Some("center") | Some("middle") => Some(Align::Center),
                Some("right") => Some(Align::Right),
                _ => None,
            };
        }
        if c.bg.is_none() {
            c.bg = e.attr("bgcolor").and_then(parse_color);
        }
        if e.name == "center" && c.align.is_none() {
            c.align = Some(Align::Center);
        }
        c
    }

    /// Relative font scale of this element, updating the running absolute scale.
    fn rel_scale(&self, css: &Css) -> Option<f32> {
        match css.font_size? {
            FontSize::Abs(a) => Some(a / self.scale),
            FontSize::Rel(r) => Some(r),
        }
    }

    fn box_style(&self, css: &Css) -> BoxStyle {
        BoxStyle {
            color: css.color,
            bg: css.bg,
            align: css.align,
            size_scale: self.rel_scale(css),
            bold: css.bold == Some(true),
            italic: css.italic == Some(true),
            pad_x: css.pad_x.unwrap_or(0.0),
            pad_y: css.pad_y.unwrap_or(0.0),
            indent: css.indent.unwrap_or(0.0).max(0.0),
            margin_top: css.margin_top.unwrap_or(0.0).max(0.0),
            margin_bottom: css.margin_bottom.unwrap_or(0.0).max(0.0),
            border: css.border,
            border_left: css.border_left,
            break_before: css.break_before,
            break_after: css.break_after,
            avoid_break: css.avoid_break,
        }
    }

    fn wrap(&self, css: &Css, blocks: Vec<Block>) -> Vec<Block> {
        let style = self.box_style(css);
        if style.is_plain() || blocks.is_empty() {
            blocks
        } else {
            vec![Block::Div { style, blocks }]
        }
    }

    fn children_blocks(&mut self, e: &Element, css: &Css) -> Vec<Block> {
        self.anc.push(elem_ref(e));
        let saved = self.scale;
        if let Some(r) = self.rel_scale(css) {
            self.scale *= r;
        }
        let out = self.blocks(&e.children, false);
        self.scale = saved;
        self.anc.pop();
        out
    }

    fn children_inline(&mut self, e: &Element) -> Vec<Inline> {
        self.anc.push(elem_ref(e));
        let out = self.inline_nodes(&e.children);
        self.anc.pop();
        out
    }

    fn blocks(&mut self, nodes: &[Node], pre: bool) -> Vec<Block> {
        let mut out = Vec::new();
        let mut inl: Vec<Inline> = Vec::new();
        for n in nodes {
            match n {
                Node::Text(t) => push_text(&mut inl, t, pre),
                Node::El(e) if e.name.starts_with('/') => {
                    flush(&mut inl, &mut out);
                    if self.fragment {
                        out.push(Block::HtmlClose { tag: e.name[1..].to_string() });
                    }
                }
                Node::El(e) if is_block(&e.name) || is_block_only(&e.name) => {
                    flush(&mut inl, &mut out);
                    self.block_el(e, &mut out);
                }
                Node::El(e) => self.inline_el(e, &mut inl),
            }
        }
        flush(&mut inl, &mut out);
        out
    }

    fn block_el(&mut self, e: &Element, out: &mut Vec<Block>) {
        let css = self.css(e);
        if css.hidden {
            return;
        }
        let name = e.name.as_str();
        match name {
            "script" | "style" | "head" | "title" | "meta" | "link" | "template" | "noscript" => {}
            "hr" => out.push(Block::Rule),
            "p" | "summary" | "figcaption" | "caption" | "legend" => {
                let inl = self.children_inline(e);
                let inl = match name {
                    "summary" => vec![Inline::Strong(inl)],
                    "figcaption" => vec![Inline::Styled {
                        style: InlineStyle { italic: true, size_scale: Some(0.9), ..Default::default() },
                        content: inl,
                    }],
                    _ => inl,
                };
                let mut b = Vec::new();
                if has_content(&inl) {
                    b.push(Block::Paragraph(trim_owned(inl)));
                }
                let mut css = css.clone();
                if matches!(name, "figcaption") && css.align.is_none() {
                    css.align = Some(Align::Center);
                }
                out.extend(self.wrap(&css, b));
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                let inl = trim_owned(self.children_inline(e));
                if has_content(&inl) {
                    let mut css = css.clone();
                    css.font_size = None;
                    out.extend(self.wrap(&css, vec![Block::Heading { level: name.as_bytes()[1] - b'0', content: inl }]));
                }
            }
            "ul" | "ol" => {
                let list = self.list(e, name == "ol");
                out.extend(self.wrap(&css, vec![list]));
            }
            "li" => {
                let b = self.children_blocks(e, &css);
                out.extend(self.wrap(&css, b));
            }
            "dl" => {
                self.anc.push(elem_ref(e));
                let mut blocks = Vec::new();
                for n in &e.children {
                    if let Node::El(c) = n {
                        match c.name.as_str() {
                            "dt" => {
                                let inl = trim_owned(self.children_inline(c));
                                if has_content(&inl) {
                                    blocks.push(Block::Paragraph(vec![Inline::Strong(inl)]));
                                }
                            }
                            "dd" => {
                                let cc = self.css(c);
                                let inner = self.children_blocks(c, &cc);
                                blocks.push(Block::Div { style: BoxStyle { indent: 18.0, ..Default::default() }, blocks: inner });
                            }
                            _ => {}
                        }
                    }
                }
                self.anc.pop();
                out.extend(self.wrap(&css, blocks));
            }
            "pre" => {
                let mut text = String::new();
                text_of(&e.children, &mut text);
                let lang = find_el(&e.children, "code")
                    .and_then(|c| c.attr("class"))
                    .and_then(|cl| cl.split_whitespace().find_map(|w| w.strip_prefix("language-")))
                    .unwrap_or("")
                    .to_string();
                let text = text.trim_matches('\n').to_string();
                out.extend(self.wrap(&css, vec![Block::CodeBlock { lang, text }]));
            }
            "blockquote" => {
                if self.fragment && e.unclosed {
                    out.push(Block::HtmlOpen { tag: name.into(), style: self.box_style(&css) });
                    out.extend(self.children_blocks(e, &css));
                } else {
                    let b = self.children_blocks(e, &css);
                    out.extend(self.wrap(&css, vec![Block::Quote(b)]));
                }
            }
            "table" => {
                if let Some(t) = self.table(e) {
                    out.extend(self.wrap(&css, vec![t]));
                }
            }
            "html" | "body" => {
                out.extend(self.children_blocks(e, &css));
            }
            "svg" | "video" | "audio" | "iframe" | "canvas" | "object" | "embed" => out.push(placeholder(name)),
            _ => {
                // Generic container (div, section, center, details, figure, ...).
                if self.fragment && e.unclosed && MARKER.contains(&name) {
                    let mut st = self.box_style(&css);
                    if name == "details" {
                        st.margin_bottom = st.margin_bottom.max(2.0);
                    }
                    out.push(Block::HtmlOpen { tag: name.into(), style: st });
                    out.extend(self.children_blocks(e, &css));
                } else {
                    let mut css = css.clone();
                    if name == "details" {
                        css.avoid_break = true;
                    }
                    let b = self.children_blocks(e, &css);
                    out.extend(self.wrap(&css, b));
                }
            }
        }
    }

    fn list(&mut self, e: &Element, ordered: bool) -> Block {
        self.anc.push(elem_ref(e));
        let mut items = Vec::new();
        let mut tight = true;
        for n in &e.children {
            let Node::El(li) = n else { continue };
            if li.name != "li" {
                continue;
            }
            let css = self.css(li);
            if css.hidden {
                continue;
            }
            if li.children.iter().any(|c| matches!(c, Node::El(x) if x.name == "p")) {
                tight = false;
            }
            let checked = li
                .children
                .iter()
                .find_map(|c| match c {
                    Node::El(x) if x.name == "input" && x.attr("type") == Some("checkbox") => Some(x.attr("checked").is_some()),
                    _ => None,
                });
            let kids: Vec<Node> = li
                .children
                .iter()
                .filter(|c| !matches!(c, Node::El(x) if x.name == "input"))
                .cloned()
                .collect();
            self.anc.push(elem_ref(li));
            let mut blocks = self.blocks(&kids, false);
            self.anc.pop();
            if blocks.is_empty() {
                blocks.push(Block::Paragraph(vec![]));
            }
            items.push(ListItem { checked, blocks });
        }
        self.anc.pop();
        let start = e.attr("start").and_then(|s| s.parse().ok()).unwrap_or(1);
        Block::List { ordered, start, tight, items }
    }

    fn table(&mut self, e: &Element) -> Option<Block> {
        self.anc.push(elem_ref(e));
        let mut caption = None;
        let mut head_rows: Vec<&Element> = Vec::new();
        let mut body_rows: Vec<&Element> = Vec::new();
        for n in &e.children {
            let Node::El(c) = n else { continue };
            match c.name.as_str() {
                "caption" => {
                    let inl = trim_owned(self.children_inline(c));
                    if has_content(&inl) {
                        caption = Some(inl);
                    }
                }
                "thead" => head_rows.extend(c.children.iter().filter_map(|n| match n {
                    Node::El(r) if r.name == "tr" => Some(r),
                    _ => None,
                })),
                "tbody" | "tfoot" => body_rows.extend(c.children.iter().filter_map(|n| match n {
                    Node::El(r) if r.name == "tr" => Some(r),
                    _ => None,
                })),
                "tr" => body_rows.push(c),
                _ => {}
            }
        }
        let mut all: Vec<&Element> = head_rows.clone();
        all.extend(body_rows.iter().copied());
        let mut header_idx: Option<usize> = None;
        if !head_rows.is_empty() {
            header_idx = Some(0);
        } else if let Some(first) = all.first() {
            let cells: Vec<&Element> = first.children.iter().filter_map(|n| match n {
                Node::El(c) if c.name == "td" || c.name == "th" => Some(c),
                _ => None,
            }).collect();
            if !cells.is_empty() && cells.iter().all(|c| c.name == "th") {
                header_idx = Some(0);
            }
        }
        let mut grid: Vec<Vec<Vec<Inline>>> = Vec::new();
        let mut aligns: Vec<Align> = Vec::new();
        for (ri, r) in all.iter().enumerate() {
            self.anc.push(elem_ref(r));
            let mut row = Vec::new();
            for n in &r.children {
                let Node::El(c) = n else { continue };
                if c.name != "td" && c.name != "th" {
                    continue;
                }
                let css = self.css(c);
                let mut inl = trim_owned(self.children_inline(c));
                let style = InlineStyle {
                    color: css.color,
                    bold: css.bold == Some(true),
                    italic: css.italic == Some(true),
                    ..Default::default()
                };
                if style != InlineStyle::default() && !inl.is_empty() {
                    inl = vec![Inline::Styled { style, content: inl }];
                }
                if ri == 0 {
                    aligns.push(match css.align {
                        Some(a) => a,
                        None => Align::Left,
                    });
                }
                let span = c.attr("colspan").and_then(|s| s.parse::<usize>().ok()).unwrap_or(1).clamp(1, 50);
                row.push(inl);
                for _ in 1..span {
                    row.push(vec![]);
                    if ri == 0 {
                        aligns.push(Align::Left);
                    }
                }
            }
            self.anc.pop();
            if !row.is_empty() {
                grid.push(row);
            }
        }
        self.anc.pop();
        if grid.is_empty() {
            return None;
        }
        let ncols = grid.iter().map(|r| r.len()).max().unwrap();
        aligns.resize(ncols, Align::Left);
        for r in &mut grid {
            r.resize(ncols, vec![]);
        }
        let header = if header_idx.is_some() { grid.remove(0) } else { vec![] };
        Some(Block::Table { aligns, header, rows: grid, caption })
    }

    fn inline_nodes(&mut self, nodes: &[Node]) -> Vec<Inline> {
        let mut inl = Vec::new();
        for n in nodes {
            match n {
                Node::Text(t) => push_text(&mut inl, t, false),
                Node::El(e) if e.name.starts_with('/') => {}
                Node::El(e) if is_block(&e.name) && e.name != "li" || is_block_only(&e.name) => {
                    let css = self.css(e);
                    if css.hidden || matches!(e.name.as_str(), "script" | "style" | "head" | "title") {
                        continue;
                    }
                    if !inl.is_empty() {
                        inl.push(Inline::HardBreak);
                    }
                    let inner = self.children_inline(e);
                    inl.extend(inner);
                    inl.push(Inline::HardBreak);
                }
                Node::El(e) => self.inline_el(e, &mut inl),
            }
        }
        inl
    }

    fn inline_el(&mut self, e: &Element, inl: &mut Vec<Inline>) {
        let css = self.css(e);
        if css.hidden {
            return;
        }
        let name = e.name.as_str();
        match name {
            "br" => inl.push(Inline::HardBreak),
            "wbr" | "script" | "style" | "meta" | "link" | "input" | "source" | "track" | "template" | "noscript" => {}
            "img" => inl.push(img_inline(e, &css)),
            "svg" | "video" | "audio" | "iframe" | "canvas" | "object" | "embed" => {
                inl.push(Inline::Styled {
                    style: InlineStyle { italic: true, ..Default::default() },
                    content: vec![Inline::Text(format!("[{name} omitted]"))],
                });
            }
            _ => {
                let content = self.children_inline(e);
                let wrapped = inline_wrap(name, &e.attrs, &css, content, self.scale);
                inl.extend(wrapped);
            }
        }
    }
}

/// Non-rendered elements, routed through `block_el` so they are skipped.
fn is_block_only(n: &str) -> bool {
    matches!(n, "script" | "style" | "head" | "title" | "meta" | "link" | "template" | "noscript")
}

fn placeholder(name: &str) -> Block {
    Block::Paragraph(vec![Inline::Styled {
        style: InlineStyle { italic: true, ..Default::default() },
        content: vec![Inline::Text(format!("[{name} content omitted]"))],
    }])
}

pub fn img_inline(e: &Element, css: &Css) -> Inline {
    let url = e.attr("src").or_else(|| e.attr("data-src")).unwrap_or("").to_string();
    let w = css.width.or_else(|| e.attr("width").and_then(dim_attr));
    let h = css.height.or_else(|| e.attr("height").and_then(dim_attr));
    Inline::Image {
        url,
        alt: e.attr("alt").unwrap_or("").to_string(),
        title: e.attr("title").map(str::to_string),
        width: w,
        height: h,
    }
}

/// Wraps already-converted inline `content` according to the tag `name` and CSS.
pub fn inline_wrap(name: &str, attrs: &[(String, String)], css: &Css, content: Vec<Inline>, scale: f32) -> Vec<Inline> {
    if content.is_empty() {
        return content;
    }
    let attr = |k: &str| attrs.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
    let mut st = InlineStyle {
        color: css.color,
        bg: css.bg,
        size_scale: match css.font_size {
            Some(FontSize::Abs(a)) => Some(a / scale),
            Some(FontSize::Rel(r)) => Some(r),
            None => None,
        },
        bold: css.bold == Some(true),
        italic: css.italic == Some(true),
        underline: css.underline == Some(true),
        strike: css.strike == Some(true),
        mono: css.mono == Some(true),
        shift: 0,
    };
    match name {
        "b" | "strong" => st.bold = true,
        "i" | "em" | "cite" | "dfn" | "var" | "address" => st.italic = true,
        "u" | "ins" => st.underline = true,
        "s" | "strike" | "del" => st.strike = true,
        "code" | "tt" | "samp" => st.mono = true,
        "kbd" => {
            st.mono = true;
            st.bg = st.bg.or(Some(Color::rgb(236, 239, 243)));
        }
        "mark" => st.bg = st.bg.or(Some(Color::rgb(255, 238, 140))),
        "small" => st.size_scale = st.size_scale.or(Some(0.85)),
        "big" => st.size_scale = st.size_scale.or(Some(1.2)),
        "sub" => {
            st.shift = -1;
            st.size_scale = Some(0.75);
        }
        "sup" => {
            st.shift = 1;
            st.size_scale = Some(0.75);
        }
        "font" => {
            if let Some(c) = attr("color").and_then(parse_color) {
                st.color = st.color.or(Some(c));
            }
            if let Some(sz) = attr("size").and_then(|s| s.trim().parse::<usize>().ok()) {
                st.size_scale = st.size_scale.or(Some([0.63, 0.82, 1.0, 1.13, 1.5, 2.0, 3.0][sz.clamp(1, 7) - 1]));
            }
        }
        "q" => {
            let mut c = vec![Inline::Text("\u{201c}".into())];
            c.extend(content);
            c.push(Inline::Text("\u{201d}".into()));
            return inline_wrap("span", attrs, css, c, scale);
        }
        _ => {}
    }
    let inner = if st == InlineStyle::default() { content } else { vec![Inline::Styled { style: st, content }] };
    if name == "a" {
        if let Some(href) = attr("href") {
            return vec![Inline::Link { url: href.to_string(), content: inner }];
        }
    }
    inner
}

fn push_text(inl: &mut Vec<Inline>, t: &str, pre: bool) {
    if pre {
        inl.push(Inline::Text(t.to_string()));
        return;
    }
    let mut s = String::with_capacity(t.len());
    let mut last_ws = false;
    for ch in t.chars() {
        if ch == ' ' || ch == '\n' || ch == '\t' || ch == '\r' {
            if !last_ws {
                s.push(' ');
            }
            last_ws = true;
        } else {
            s.push(ch);
            last_ws = false;
        }
    }
    let prev_space = match inl.last() {
        None | Some(Inline::HardBreak) | Some(Inline::SoftBreak) => true,
        Some(Inline::Text(p)) => p.ends_with(' '),
        _ => false,
    };
    if prev_space && s.starts_with(' ') {
        s.remove(0);
    }
    if !s.is_empty() {
        inl.push(Inline::Text(s));
    }
}

fn trim_inline(inl: &mut Vec<Inline>) {
    while matches!(inl.first(), Some(Inline::HardBreak | Inline::SoftBreak)) {
        inl.remove(0);
    }
    while matches!(inl.last(), Some(Inline::HardBreak | Inline::SoftBreak)) {
        inl.pop();
    }
    if let Some(Inline::Text(t)) = inl.first_mut() {
        *t = t.trim_start().to_string();
    }
    if let Some(Inline::Text(t)) = inl.last_mut() {
        *t = t.trim_end().to_string();
    }
}

fn trim_owned(mut inl: Vec<Inline>) -> Vec<Inline> {
    trim_inline(&mut inl);
    inl
}

fn flush(inl: &mut Vec<Inline>, out: &mut Vec<Block>) {
    let mut v = std::mem::take(inl);
    trim_inline(&mut v);
    if has_content(&v) {
        out.push(Block::Paragraph(v));
    }
}

// ------------------------------------------------------------- entry points

/// Parses a complete HTML document.
pub fn parse_document(src: &str) -> Document {
    let nodes = build(src);
    let mut css = String::new();
    collect_styles(&nodes, &mut css);
    let sheet = Sheet::parse(&css);
    let mut title = None;
    if let Some(t) = find_el(&nodes, "title") {
        let mut s = String::new();
        text_of(&t.children, &mut s);
        let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
        if !s.is_empty() {
            title = Some(s);
        }
    }
    let mut conv = Conv { sheet: &sheet, anc: vec![], fragment: false, scale: 1.0 };
    let mut page_bg = None;
    for tag in ["html", "body"] {
        if let Some(e) = find_el(&nodes, tag) {
            let c = conv.css(e);
            page_bg = c.bg.or(page_bg);
        }
    }
    let blocks = conv.blocks(&nodes, false);
    Document { blocks, title, page_bg }
}

/// Parses an HTML fragment embedded in Markdown. Unbalanced container tags
/// come back as `HtmlOpen`/`HtmlClose` markers for the block parser to fold.
pub fn parse_fragment(src: &str, sheet: &Sheet) -> Vec<Block> {
    let nodes = build(src);
    let mut conv = Conv { sheet, anc: vec![], fragment: true, scale: 1.0 };
    conv.blocks(&nodes, false)
}

/// Builds the inline replacement for a tag found inside Markdown text.
pub fn inline_tag(name: &str, attrs: &[(String, String)], content: Vec<Inline>, sheet: &Sheet) -> Vec<Inline> {
    let el = Element { name: name.to_string(), attrs: attrs.to_vec(), children: vec![], unclosed: false };
    let css = sheet.compute(&elem_ref(&el), &[], el.attr("style"));
    inline_wrap(name, attrs, &css, content, 1.0)
}

pub fn inline_img(attrs: &[(String, String)], sheet: &Sheet) -> Inline {
    let el = Element { name: "img".into(), attrs: attrs.to_vec(), children: vec![], unclosed: false };
    let css = sheet.compute(&elem_ref(&el), &[], el.attr("style"));
    img_inline(&el, &css)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_and_entities() {
        let c: Vec<char> = "<a href=\"x?a=1&amp;b=2\" data-k=v>".chars().collect();
        let (Tag::Open { name, attrs, .. }, n) = parse_tag(&c).unwrap() else { panic!() };
        assert_eq!(name, "a");
        assert_eq!(attrs[0].1, "x?a=1&b=2");
        assert_eq!(n, c.len());
        assert!(parse_tag(&"<https://x.y>".chars().collect::<Vec<_>>()).is_none());
    }

    #[test]
    fn implicit_closing() {
        let d = parse_document("<ul><li>a<li>b</ul><p>one<p>two");
        let Block::List { items, .. } = &d.blocks[0] else { panic!("{:?}", d.blocks) };
        assert_eq!(items.len(), 2);
        assert_eq!(d.blocks.len(), 3);
    }

    #[test]
    fn table_with_header_and_colspan() {
        let d = parse_document("<table><thead><tr><th>A<th>B</tr></thead><tr><td colspan=2>x</td></tr></table>");
        let Block::Table { header, rows, .. } = &d.blocks[0] else { panic!() };
        assert_eq!(header.len(), 2);
        assert_eq!(rows[0].len(), 2);
    }

    #[test]
    fn css_applies_to_blocks() {
        let d = parse_document("<style>.hero{color:#ff0000;text-align:center}</style><div class=hero><p>hi</p></div>");
        let Block::Div { style, .. } = &d.blocks[0] else { panic!("{:?}", d.blocks) };
        assert_eq!(style.color, Some(Color::rgb(255, 0, 0)));
        assert_eq!(style.align, Some(Align::Center));
    }

    #[test]
    fn fragment_markers() {
        let b = parse_fragment("<div align=\"center\">", &Sheet::default());
        assert!(matches!(&b[0], Block::HtmlOpen { tag, style } if tag == "div" && style.align == Some(Align::Center)));
        let b = parse_fragment("</div>", &Sheet::default());
        assert!(matches!(&b[0], Block::HtmlClose { tag } if tag == "div"));
    }
}
