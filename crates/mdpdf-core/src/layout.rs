//! Layout engine: turns a [`Document`] into pages of positioned drawing ops.
//!
//! Coordinates are in points with the origin at the top-left and y growing
//! downward. The PDF writer flips y; the GUI viewer paints the ops as-is.
//! Both consume the same output, so what you see is what the PDF contains.

use crate::ast::*;
use crate::fonts::{sanitize, Face, Family};
use std::rc::Rc;

const LEADING: f32 = 1.4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Color { r, g, b }
    }
    pub fn unit(self) -> (f32, f32, f32) {
        (self.r as f32 / 255.0, self.g as f32 / 255.0, self.b as f32 / 255.0)
    }
}

#[derive(Clone, Debug)]
pub struct Theme {
    pub page_bg: Color,
    pub text: Color,
    pub heading: Color,
    pub muted: Color,
    pub link: Color,
    pub code_text: Color,
    pub code_bg: Color,
    pub border: Color,
    pub table_header_bg: Color,
}

impl Theme {
    pub fn light() -> Self {
        Theme {
            page_bg: Color::rgb(255, 255, 255),
            text: Color::rgb(33, 37, 41),
            heading: Color::rgb(17, 24, 39),
            muted: Color::rgb(95, 104, 115),
            link: Color::rgb(9, 105, 218),
            code_text: Color::rgb(36, 41, 47),
            code_bg: Color::rgb(243, 244, 246),
            border: Color::rgb(208, 215, 222),
            table_header_bg: Color::rgb(238, 241, 245),
        }
    }
    pub fn dark() -> Self {
        Theme {
            page_bg: Color::rgb(24, 26, 31),
            text: Color::rgb(215, 218, 224),
            heading: Color::rgb(240, 242, 246),
            muted: Color::rgb(139, 148, 158),
            link: Color::rgb(88, 166, 255),
            code_text: Color::rgb(225, 228, 232),
            code_bg: Color::rgb(36, 40, 47),
            border: Color::rgb(58, 64, 74),
            table_header_bg: Color::rgb(33, 37, 44),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Config {
    pub page_width: f32,
    pub page_height: f32,
    pub margin_top: f32,
    pub margin_right: f32,
    pub margin_bottom: f32,
    pub margin_left: f32,
    pub base_size: f32,
    pub family: Family,
    /// Break into fixed-size pages (PDF). When false, one tall page (viewer).
    pub paginate: bool,
    pub page_numbers: bool,
    pub theme: Theme,
}

impl Config {
    pub fn a4() -> Self {
        Config {
            page_width: 595.28,
            page_height: 841.89,
            margin_top: 64.0,
            margin_right: 60.0,
            margin_bottom: 64.0,
            margin_left: 60.0,
            base_size: 11.0,
            family: Family::Sans,
            paginate: true,
            page_numbers: true,
            theme: Theme::light(),
        }
    }
    pub fn letter() -> Self {
        Config { page_width: 612.0, page_height: 792.0, ..Config::a4() }
    }
}

#[derive(Clone, Debug)]
pub enum Op {
    Text { x: f32, y: f32, face: Face, size: f32, color: Color, text: String },
    Rect { x: f32, y: f32, w: f32, h: f32, fill: Option<Color>, stroke: Option<(Color, f32)> },
    Line { x1: f32, y1: f32, x2: f32, y2: f32, color: Color, width: f32 },
    Circle { cx: f32, cy: f32, r: f32, fill: bool, color: Color, width: f32 },
    Link { x: f32, y: f32, w: f32, h: f32, url: String },
}

#[derive(Clone, Debug, Default)]
pub struct Page {
    pub ops: Vec<Op>,
}

#[derive(Clone, Debug)]
pub struct HeadingMark {
    pub level: u8,
    pub title: String,
    pub page: usize,
    pub y: f32,
}

#[derive(Clone, Debug)]
pub struct Layout {
    pub width: f32,
    pub height: f32,
    pub background: Color,
    pub pages: Vec<Page>,
    pub headings: Vec<HeadingMark>,
    pub title: Option<String>,
}

// ---------------------------------------------------------------- atoms

#[derive(Clone)]
struct Style {
    face: Face,
    size: f32,
    color: Color,
    link: Option<Rc<str>>,
    strike: bool,
    code: bool,
}

#[derive(Clone)]
struct Atom {
    text: String,
    style: Style,
    width: f32,
    space_before: bool,
    space_w: f32,
}

enum Item {
    Atom(Atom),
    Hard,
}

fn make_atom(text: &str, st: &Style, space_before: bool) -> Atom {
    Atom {
        width: st.face.width(text, st.size),
        text: text.to_string(),
        style: st.clone(),
        space_before,
        space_w: st.face.char_width(' ', st.size),
    }
}

fn text_atoms(raw: &str, st: &Style, out: &mut Vec<Item>, pending: &mut bool) {
    let t = sanitize(raw);
    let is_ws = |c: char| c == ' ' || c == '\n' || c == '\t';
    let mut rest = t.as_str();
    while !rest.is_empty() {
        let lead = rest.len() - rest.trim_start_matches(is_ws).len();
        if lead > 0 {
            *pending = true;
            rest = &rest[lead..];
            continue;
        }
        let end = rest.find(is_ws).unwrap_or(rest.len());
        out.push(Item::Atom(make_atom(&rest[..end], st, *pending)));
        *pending = false;
        rest = &rest[end..];
    }
}

fn flatten(inl: &[Inline], st: &Style, out: &mut Vec<Item>, pending: &mut bool, theme: &Theme) {
    for i in inl {
        match i {
            Inline::Text(t) => text_atoms(t, st, out, pending),
            Inline::Code(t) => {
                let s = Style {
                    face: Face::new(Family::Mono, st.face.bold, st.face.italic),
                    size: st.size * 0.9,
                    color: if st.link.is_some() { st.color } else { theme.code_text },
                    code: true,
                    ..st.clone()
                };
                text_atoms(t, &s, out, pending);
            }
            Inline::Emph(c) => {
                let s = Style { face: st.face.with(false, true), ..st.clone() };
                flatten(c, &s, out, pending, theme);
            }
            Inline::Strong(c) => {
                let s = Style { face: st.face.with(true, false), ..st.clone() };
                flatten(c, &s, out, pending, theme);
            }
            Inline::Strike(c) => {
                let s = Style { strike: true, ..st.clone() };
                flatten(c, &s, out, pending, theme);
            }
            Inline::Link { url, content } => {
                let s = Style { color: theme.link, link: Some(url.as_str().into()), ..st.clone() };
                flatten(content, &s, out, pending, theme);
            }
            Inline::Image { url, alt } => {
                let s = Style {
                    face: st.face.with(false, true),
                    color: theme.link,
                    link: Some(url.as_str().into()),
                    ..st.clone()
                };
                let label = if alt.is_empty() { "[image]".to_string() } else { format!("[image: {alt}]") };
                text_atoms(&label, &s, out, pending);
            }
            Inline::SoftBreak => *pending = true,
            Inline::HardBreak => {
                out.push(Item::Hard);
                *pending = false;
            }
        }
    }
}

/// Natural (unwrapped) width and the width of the longest unbreakable word.
fn measure(items: &[Item]) -> (f32, f32) {
    let (mut min, mut max, mut cur, mut first) = (0f32, 0f32, 0f32, true);
    for it in items {
        match it {
            Item::Hard => {
                max = max.max(cur);
                cur = 0.0;
                first = true;
            }
            Item::Atom(a) => {
                cur += if first { 0.0 } else if a.space_before { a.space_w } else { 0.0 } + a.width;
                first = false;
                min = min.max(a.width);
            }
        }
    }
    (min, max.max(cur))
}

fn break_lines(items: Vec<Item>, width: f32) -> Vec<Vec<Atom>> {
    let mut lines: Vec<Vec<Atom>> = Vec::new();
    let mut cur: Vec<Atom> = Vec::new();
    let mut cur_w = 0f32;
    for it in items {
        let a = match it {
            Item::Hard => {
                lines.push(std::mem::take(&mut cur));
                cur_w = 0.0;
                continue;
            }
            Item::Atom(a) => a,
        };
        let mut a = a;
        loop {
            let sp = if !cur.is_empty() && a.space_before { a.space_w } else { 0.0 };
            if !cur.is_empty() && cur_w + sp + a.width > width + 0.01 {
                lines.push(std::mem::take(&mut cur));
                cur_w = 0.0;
                continue;
            }
            if cur.is_empty() && a.width > width + 0.01 {
                // Unbreakable word wider than the line: split by characters.
                let mut head = String::new();
                let mut w = 0f32;
                let mut rest = String::new();
                for ch in a.text.chars() {
                    let cw = a.style.face.char_width(ch, a.style.size);
                    if rest.is_empty() && (w + cw <= width + 0.01 || head.is_empty()) {
                        head.push(ch);
                        w += cw;
                    } else {
                        rest.push(ch);
                    }
                }
                cur.push(make_atom(&head, &a.style, false));
                lines.push(std::mem::take(&mut cur));
                cur_w = 0.0;
                a = make_atom(&rest, &a.style, false);
                continue;
            }
            cur_w += sp + a.width;
            cur.push(a);
            break;
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}

fn line_size(atoms: &[Atom], default: f32) -> f32 {
    atoms.iter().map(|a| a.style.size).fold(0.0, f32::max).max(if atoms.is_empty() { default } else { 0.0 })
}

// --------------------------------------------------------------- engine

#[derive(Clone, Copy)]
struct Frame {
    x: f32,
    w: f32,
}

enum DecoKind {
    Bg(Color),
    Bar(Color),
}

struct Deco {
    kind: DecoKind,
    x: f32,
    w: f32,
    op_idx: usize,
    start_y: f32,
}

struct Engine<'a> {
    cfg: &'a Config,
    pages: Vec<Page>,
    y: f32,
    decos: Vec<Deco>,
    headings: Vec<HeadingMark>,
    color: Color,
}

pub fn layout(doc: &Document, cfg: &Config) -> Layout {
    let mut e = Engine {
        cfg,
        pages: vec![Page::default()],
        y: cfg.margin_top,
        decos: Vec::new(),
        headings: Vec::new(),
        color: cfg.theme.text,
    };
    let fr = Frame { x: cfg.margin_left, w: (cfg.page_width - cfg.margin_left - cfg.margin_right).max(40.0) };
    e.blocks(&doc.blocks, fr, 0, false);
    let title = doc.blocks.iter().find_map(|b| match b {
        Block::Heading { level: 1, content } => Some(plain_text(content)),
        _ => None,
    });
    let height = if cfg.paginate {
        cfg.page_height
    } else {
        e.y + cfg.margin_bottom
    };
    if cfg.paginate && cfg.page_numbers {
        let n = e.pages.len();
        for (i, p) in e.pages.iter_mut().enumerate() {
            let label = format!("{} / {}", i + 1, n);
            let face = Face::new(cfg.family, false, false);
            let size = 9.0;
            let w = face.width(&label, size);
            p.ops.push(Op::Text {
                x: (cfg.page_width - w) / 2.0,
                y: cfg.page_height - cfg.margin_bottom / 2.0 + 3.0,
                face,
                size,
                color: cfg.theme.muted,
                text: label,
            });
        }
    }
    Layout {
        width: cfg.page_width,
        height,
        background: cfg.theme.page_bg,
        pages: e.pages,
        headings: e.headings,
        title,
    }
}

impl<'a> Engine<'a> {
    fn base_style(&self) -> Style {
        Style {
            face: Face::new(self.cfg.family, false, false),
            size: self.cfg.base_size,
            color: self.color,
            link: None,
            strike: false,
            code: false,
        }
    }

    fn limit(&self) -> f32 {
        self.cfg.page_height - self.cfg.margin_bottom
    }

    fn ops(&mut self) -> &mut Vec<Op> {
        &mut self.pages.last_mut().unwrap().ops
    }

    fn ensure(&mut self, h: f32) {
        if self.cfg.paginate && self.y + h > self.limit() && self.y > self.cfg.margin_top + 0.5 {
            self.new_page();
        }
    }

    fn new_page(&mut self) {
        self.flush_decos(true);
        self.pages.push(Page::default());
        self.y = self.cfg.margin_top;
        for d in &mut self.decos {
            d.op_idx = 0;
            d.start_y = self.y;
        }
    }

    fn flush_decos(&mut self, keep: bool) {
        let end = self.y;
        let n = self.decos.len();
        for k in (0..n).rev() {
            let d = &self.decos[k];
            if end > d.start_y {
                let op = match d.kind {
                    DecoKind::Bg(c) => Op::Rect { x: d.x, y: d.start_y, w: d.w, h: end - d.start_y, fill: Some(c), stroke: None },
                    DecoKind::Bar(c) => Op::Rect { x: d.x, y: d.start_y, w: d.w, h: end - d.start_y, fill: Some(c), stroke: None },
                };
                let ops = &mut self.pages.last_mut().unwrap().ops;
                let at = d.op_idx.min(ops.len());
                ops.insert(at, op);
            }
        }
        if !keep {
            self.decos.clear();
        }
    }

    fn begin_deco(&mut self, kind: DecoKind, x: f32, w: f32) {
        let op_idx = self.ops().len();
        self.decos.push(Deco { kind, x, w, op_idx, start_y: self.y });
    }

    fn end_deco(&mut self) {
        let end = self.y;
        let d = self.decos.pop().unwrap();
        if end > d.start_y {
            let op = match d.kind {
                DecoKind::Bg(c) | DecoKind::Bar(c) => {
                    Op::Rect { x: d.x, y: d.start_y, w: d.w, h: end - d.start_y, fill: Some(c), stroke: None }
                }
            };
            let ops = &mut self.pages.last_mut().unwrap().ops;
            let at = d.op_idx.min(ops.len());
            ops.insert(at, op);
        }
    }

    fn blocks(&mut self, blocks: &[Block], fr: Frame, depth: usize, tight: bool) {
        let gap = self.cfg.base_size * if tight { 0.3 } else { 0.85 };
        for (k, b) in blocks.iter().enumerate() {
            if k > 0 {
                self.y += gap;
            }
            self.block(b, fr, depth, k == 0);
        }
    }

    fn block(&mut self, b: &Block, fr: Frame, depth: usize, first: bool) {
        let base = self.cfg.base_size;
        match b {
            Block::Paragraph(inl) => {
                let st = self.base_style();
                let mut items = Vec::new();
                flatten(inl, &st, &mut items, &mut false, &self.cfg.theme);
                let lines = break_lines(items, fr.w);
                self.put_lines(&lines, fr, Align::Left, base);
            }
            Block::Heading { level, content } => self.heading(*level, content, fr, first),
            Block::CodeBlock { text, .. } => self.code_block(text, fr),
            Block::Quote(inner) => {
                let bar = 3.0;
                let indent = 14.0;
                self.begin_deco(DecoKind::Bar(self.cfg.theme.border), fr.x, bar);
                let saved = self.color;
                self.color = self.cfg.theme.muted;
                self.blocks(inner, Frame { x: fr.x + indent, w: fr.w - indent }, depth + 1, false);
                self.color = saved;
                self.end_deco();
            }
            Block::List { ordered, start, tight, items } => self.list(*ordered, *start, *tight, items, fr, depth),
            Block::Table { aligns, header, rows } => self.table(aligns, header, rows, fr),
            Block::Rule => {
                self.ensure(10.0);
                let y = self.y + 5.0;
                let (c, x1, x2) = (self.cfg.theme.border, fr.x, fr.x + fr.w);
                self.ops().push(Op::Line { x1, y1: y, x2, y2: y, color: c, width: 1.0 });
                self.y += 10.0;
            }
        }
    }

    fn put_lines(&mut self, lines: &[Vec<Atom>], fr: Frame, align: Align, base: f32) {
        for line in lines {
            let lh = line_size(line, base) * LEADING;
            self.ensure(lh);
            self.emit_line(line, fr.x, self.y, fr.w, align, base);
            self.y += lh;
        }
    }

    /// Draws one line of atoms with its top at `y`; returns the line height.
    fn emit_line(&mut self, atoms: &[Atom], x: f32, y: f32, avail: f32, align: Align, base: f32) -> f32 {
        let size = line_size(atoms, base);
        let lh = size * LEADING;
        let mut total = 0f32;
        for (k, a) in atoms.iter().enumerate() {
            if k > 0 && a.space_before {
                total += a.space_w;
            }
            total += a.width;
        }
        let mut cx = x + match align {
            Align::Left => 0.0,
            Align::Center => ((avail - total) / 2.0).max(0.0),
            Align::Right => (avail - total).max(0.0),
        };
        let baseline = y + lh / 2.0 + 0.36 * size;
        let theme = self.cfg.theme.clone();
        let mut prev: Option<&Atom> = None;
        for a in atoms {
            let gap = if prev.is_some() && a.space_before { a.space_w } else { 0.0 };
            let x0 = cx + gap;
            let joined = |p: Option<&Atom>, f: &dyn Fn(&Atom) -> bool| p.is_some_and(|p| f(p) && f(a));
            let st = &a.style;
            if st.code {
                let start = if joined(prev, &|q| q.style.code) { cx } else { x0 - 1.5 };
                let h = st.size * 1.2;
                self.ops().push(Op::Rect {
                    x: start,
                    y: baseline - st.size * 0.88,
                    w: x0 + a.width + 1.5 - start,
                    h,
                    fill: Some(theme.code_bg),
                    stroke: None,
                });
            }
            self.ops().push(Op::Text {
                x: x0,
                y: baseline,
                face: st.face,
                size: st.size,
                color: st.color,
                text: a.text.clone(),
            });
            if let Some(url) = &st.link {
                let same = |q: &Atom| q.style.link.as_deref() == Some(url);
                let start = if joined(prev, &same) { cx } else { x0 };
                let uy = baseline + st.size * 0.13;
                self.ops().push(Op::Line {
                    x1: start,
                    y1: uy,
                    x2: x0 + a.width,
                    y2: uy,
                    color: st.color,
                    width: 0.5 + st.size * 0.03,
                });
                self.ops().push(Op::Link {
                    x: start,
                    y: baseline - st.size * 0.9,
                    w: x0 + a.width - start,
                    h: st.size * 1.2,
                    url: url.to_string(),
                });
            }
            if st.strike {
                let start = if joined(prev, &|q| q.style.strike) { cx } else { x0 };
                let sy = baseline - st.size * 0.28;
                self.ops().push(Op::Line {
                    x1: start,
                    y1: sy,
                    x2: x0 + a.width,
                    y2: sy,
                    color: st.color,
                    width: 0.5 + st.size * 0.03,
                });
            }
            cx = x0 + a.width;
            prev = Some(a);
        }
        lh
    }

    fn heading(&mut self, level: u8, content: &[Inline], fr: Frame, first: bool) {
        let mult = [2.0, 1.55, 1.3, 1.12, 1.0, 0.92][(level.clamp(1, 6) - 1) as usize];
        let size = self.cfg.base_size * mult;
        let theme = self.cfg.theme.clone();
        let st = Style {
            face: Face::new(self.cfg.family, true, false),
            size,
            color: if level == 6 { theme.muted } else { theme.heading },
            link: None,
            strike: false,
            code: false,
        };
        let mut items = Vec::new();
        flatten(content, &st, &mut items, &mut false, &theme);
        let lines = break_lines(items, fr.w);
        if !first {
            self.y += size * 0.45;
        }
        let lh = size * LEADING;
        self.ensure(lh * lines.len().max(1) as f32 + self.cfg.base_size * LEADING * 2.0);
        self.headings.push(HeadingMark {
            level,
            title: plain_text(content),
            page: self.pages.len() - 1,
            y: self.y,
        });
        self.put_lines(&lines, fr, Align::Left, size);
        if level <= 2 {
            self.y += 3.0;
            let y = self.y;
            self.ops().push(Op::Line { x1: fr.x, y1: y, x2: fr.x + fr.w, y2: y, color: theme.border, width: 0.8 });
            self.y += 3.0;
        }
    }

    fn code_block(&mut self, text: &str, fr: Frame) {
        let size = self.cfg.base_size * 0.85;
        let face = Face::new(Family::Mono, false, false);
        let pad = 7.0;
        let cw = face.char_width('m', size);
        let max_chars = (((fr.w - 2.0 * pad) / cw).floor() as usize).max(1);
        let lh = size * 1.4;
        let theme = self.cfg.theme.clone();
        let mut vis: Vec<String> = Vec::new();
        for l in sanitize(text).split('\n') {
            let chars: Vec<char> = l.chars().collect();
            if chars.is_empty() {
                vis.push(String::new());
            }
            for ch in chars.chunks(max_chars) {
                vis.push(ch.iter().collect());
            }
        }
        self.ensure(lh + pad);
        self.begin_deco(DecoKind::Bg(theme.code_bg), fr.x, fr.w);
        self.y += pad;
        for l in &vis {
            self.ensure(lh);
            if !l.is_empty() {
                let y = self.y + lh / 2.0 + 0.36 * size;
                self.ops().push(Op::Text { x: fr.x + pad, y, face, size, color: theme.code_text, text: l.clone() });
            }
            self.y += lh;
        }
        self.y += pad;
        self.end_deco();
    }

    fn list(&mut self, ordered: bool, start: u32, tight: bool, items: &[ListItem], fr: Frame, depth: usize) {
        let base = self.cfg.base_size;
        let face = Face::new(self.cfg.family, false, false);
        let label = |n: u32| format!("{n}.");
        let widest = if ordered {
            items
                .iter()
                .enumerate()
                .map(|(k, _)| face.width(&label(start + k as u32), base))
                .fold(0.0, f32::max)
        } else {
            0.0
        };
        let indent = (widest + 0.8 * base).max(1.7 * base);
        let lh = base * LEADING;
        let gap = base * if tight { 0.3 } else { 0.85 };
        for (k, item) in items.iter().enumerate() {
            if k > 0 {
                self.y += gap;
            }
            self.ensure(lh * 1.5);
            let top = self.y;
            let color = self.color;
            let theme = self.cfg.theme.clone();
            let mx = fr.x + indent;
            if let Some(done) = item.checked {
                let s = base * 0.8;
                let (bx, by) = (mx - 0.62 * base - s / 2.0, top + (lh - s) / 2.0);
                self.ops().push(Op::Rect { x: bx, y: by, w: s, h: s, fill: None, stroke: Some((theme.muted, 0.9)) });
                if done {
                    let l = |x1: f32, y1: f32, x2: f32, y2: f32| Op::Line { x1, y1, x2, y2, color: theme.link, width: 1.4 };
                    self.ops().push(l(bx + s * 0.2, by + s * 0.55, bx + s * 0.42, by + s * 0.78));
                    self.ops().push(l(bx + s * 0.42, by + s * 0.78, bx + s * 0.82, by + s * 0.22));
                }
            } else if ordered {
                let text = label(start + k as u32);
                let w = face.width(&text, base);
                let y = top + lh / 2.0 + 0.36 * base;
                self.ops().push(Op::Text { x: mx - 0.4 * base - w, y, face, size: base, color, text });
            } else {
                let r = base * 0.17;
                let (cx, cy) = (mx - 0.62 * base, top + lh / 2.0 + 0.03 * base);
                match depth % 3 {
                    0 => self.ops().push(Op::Circle { cx, cy, r, fill: true, color, width: 1.0 }),
                    1 => self.ops().push(Op::Circle { cx, cy, r, fill: false, color, width: 0.8 }),
                    _ => self.ops().push(Op::Rect {
                        x: cx - r,
                        y: cy - r,
                        w: 2.0 * r,
                        h: 2.0 * r,
                        fill: Some(color),
                        stroke: None,
                    }),
                }
            }
            if item.checked == Some(true) {
                self.color = theme.muted;
            }
            self.blocks(&item.blocks, Frame { x: mx, w: fr.w - indent }, depth + 1, tight);
            self.color = color;
        }
    }

    fn table(&mut self, aligns: &[Align], header: &[Vec<Inline>], rows: &[Vec<Vec<Inline>>], fr: Frame) {
        let ncol = aligns.len().max(1);
        let theme = self.cfg.theme.clone();
        let base = self.cfg.base_size;
        let (px, py) = (6.0f32, 4.0f32);
        let st = self.base_style();
        let hst = Style { face: st.face.with(true, false), ..st.clone() };
        let to_items = |cells: &[Vec<Inline>], s: &Style| -> Vec<Vec<Item>> {
            (0..ncol)
                .map(|c| {
                    let mut v = Vec::new();
                    if let Some(cell) = cells.get(c) {
                        flatten(cell, s, &mut v, &mut false, &theme);
                    }
                    v
                })
                .collect()
        };
        let mut all: Vec<Vec<Vec<Item>>> = vec![to_items(header, &hst)];
        all.extend(rows.iter().map(|r| to_items(r, &st)));
        let mut min = vec![0f32; ncol];
        let mut max = vec![0f32; ncol];
        for r in &all {
            for (c, cell) in r.iter().enumerate() {
                let (mn, mx) = measure(cell);
                min[c] = min[c].max(mn);
                max[c] = max[c].max(mx);
            }
        }
        let avail = (fr.w - ncol as f32 * 2.0 * px).max(ncol as f32 * 10.0);
        let (smin, smax): (f32, f32) = (min.iter().sum(), max.iter().sum());
        let widths: Vec<f32> = if smax <= avail {
            max.clone()
        } else if smin >= avail {
            min.iter().map(|m| m / smin * avail).collect()
        } else {
            let extra = avail - smin;
            let wsum: f32 = (0..ncol).map(|c| max[c] - min[c]).sum();
            (0..ncol).map(|c| min[c] + extra * (max[c] - min[c]) / wsum).collect()
        };
        struct Row {
            cells: Vec<Vec<Vec<Atom>>>,
            h: f32,
        }
        let laid: Vec<Row> = all
            .into_iter()
            .map(|r| {
                let cells: Vec<Vec<Vec<Atom>>> =
                    r.into_iter().enumerate().map(|(c, items)| break_lines(items, widths[c])).collect();
                let h = cells
                    .iter()
                    .map(|ls| ls.iter().map(|l| line_size(l, base) * LEADING).sum::<f32>().max(base * LEADING))
                    .fold(0.0, f32::max)
                    + 2.0 * py;
                Row { cells, h }
            })
            .collect();
        let table_w: f32 = widths.iter().map(|w| w + 2.0 * px).sum();
        let draw = |e: &mut Engine, row: &Row, head: bool| {
            let mut x = fr.x;
            let y = e.y;
            for c in 0..ncol {
                let cw = widths[c] + 2.0 * px;
                e.ops().push(Op::Rect {
                    x,
                    y,
                    w: cw,
                    h: row.h,
                    fill: head.then_some(theme.table_header_bg),
                    stroke: Some((theme.border, 0.6)),
                });
                let mut ly = y + py;
                for line in &row.cells[c] {
                    ly += e.emit_line(line, x + px, ly, widths[c], aligns.get(c).copied().unwrap_or(Align::Left), base);
                }
                x += cw;
            }
            e.y += row.h;
        };
        let _ = table_w;
        for (r, row) in laid.iter().enumerate() {
            let before = self.pages.len();
            self.ensure(row.h + if r == 0 { laid.get(1).map_or(0.0, |n| n.h) } else { 0.0 });
            if self.pages.len() > before && r > 0 {
                draw(self, &laid[0], true);
            }
            draw(self, row, r == 0);
        }
    }
}
