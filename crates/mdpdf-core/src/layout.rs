//! Layout engine: turns a [`Document`] into pages of positioned drawing ops.
//!
//! Coordinates are in points with the origin at the top-left and y growing
//! downward. The PDF writer flips y; the GUI viewer paints the ops as-is.
//! Both consume the same output, so what you see is what the PDF contains.
//!
//! With `Config::designed` the engine also applies "smart pagination":
//! headings stay with the content that follows, paragraphs and code blocks
//! never leave a lone line at the top or bottom of a page, small blocks are
//! kept whole, tables and images are never cut, and tables that must span
//! pages split only between rows (header repeated, no one-row stragglers).

use crate::ast::*;
pub use crate::ast::Color;
use crate::fonts::{sanitize, Face, Family};
use crate::image::{self, ImageData};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

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
    pub accent: Color,
    pub zebra: Color,
    pub quote_bg: Color,
    pub on_accent: Color,
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
            accent: Color::rgb(9, 105, 218),
            zebra: Color::rgb(249, 250, 251),
            quote_bg: Color::rgb(247, 248, 250),
            on_accent: Color::rgb(255, 255, 255),
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
            accent: Color::rgb(88, 166, 255),
            zebra: Color::rgb(29, 32, 38),
            quote_bg: Color::rgb(30, 33, 39),
            on_accent: Color::rgb(15, 17, 21),
        }
    }
    /// Refined print palette for Designed mode.
    pub fn designed() -> Self {
        let accent = Color::rgb(37, 84, 199);
        Theme {
            page_bg: Color::rgb(255, 255, 255),
            text: Color::rgb(38, 44, 54),
            heading: Color::rgb(15, 23, 42),
            muted: Color::rgb(100, 110, 125),
            link: accent,
            code_text: Color::rgb(30, 41, 59),
            code_bg: Color::rgb(244, 246, 250),
            border: Color::rgb(222, 228, 236),
            table_header_bg: accent,
            accent,
            zebra: Color::rgb(248, 250, 253),
            quote_bg: Color::rgb(243, 246, 251),
            on_accent: Color::rgb(255, 255, 255),
        }
    }
    pub fn designed_dark() -> Self {
        let accent = Color::rgb(110, 168, 254);
        Theme {
            page_bg: Color::rgb(20, 23, 29),
            text: Color::rgb(210, 216, 226),
            heading: Color::rgb(241, 245, 250),
            muted: Color::rgb(139, 150, 166),
            link: accent,
            code_text: Color::rgb(222, 228, 238),
            code_bg: Color::rgb(30, 35, 44),
            border: Color::rgb(52, 60, 74),
            table_header_bg: Color::rgb(41, 82, 160),
            accent,
            zebra: Color::rgb(25, 29, 37),
            quote_bg: Color::rgb(27, 32, 41),
            on_accent: Color::rgb(255, 255, 255),
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
    /// Designed mode: richer styling plus smart pagination.
    pub designed: bool,
    /// Line height as a multiple of the font size.
    pub leading: f32,
    /// Running header/footer (paginated Designed output).
    pub header: bool,
    /// Directory that relative image paths resolve against.
    pub base_dir: Option<PathBuf>,
    /// Designed mode only: render through an external Chromium-based browser
    /// (full HTML/CSS, never-cut page breaks) instead of the built-in engine.
    pub browser: bool,
    /// Browser executable; `None` auto-detects (see `browser::find_browser`).
    pub browser_path: Option<PathBuf>,
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
            designed: false,
            leading: 1.4,
            header: false,
            base_dir: None,
            browser: false,
            browser_path: None,
        }
    }
    pub fn letter() -> Self {
        Config { page_width: 612.0, page_height: 792.0, ..Config::a4() }
    }
    /// A4, Designed mode through the browser engine (the default Designed backend).
    pub fn designed_a4() -> Self {
        Config { browser: true, ..Config::designed_builtin_a4() }
    }
    pub fn designed_letter() -> Self {
        Config { page_width: 612.0, page_height: 792.0, ..Config::designed_a4() }
    }
    /// A4, Designed look with the built-in engine and smart pagination (offline, no browser).
    pub fn designed_builtin_a4() -> Self {
        Config {
            margin_top: 76.0,
            margin_bottom: 72.0,
            margin_left: 66.0,
            margin_right: 66.0,
            base_size: 10.5,
            theme: Theme::designed(),
            designed: true,
            leading: 1.55,
            header: true,
            ..Config::a4()
        }
    }
    pub fn designed_builtin_letter() -> Self {
        Config { page_width: 612.0, page_height: 792.0, ..Config::designed_builtin_a4() }
    }
}

#[derive(Clone, Debug)]
pub enum Op {
    Text { x: f32, y: f32, face: Face, size: f32, color: Color, text: String },
    Rect { x: f32, y: f32, w: f32, h: f32, fill: Option<Color>, stroke: Option<(Color, f32)> },
    Line { x1: f32, y1: f32, x2: f32, y2: f32, color: Color, width: f32 },
    Circle { cx: f32, cy: f32, r: f32, fill: bool, color: Color, width: f32 },
    /// `img` indexes [`Layout::images`].
    Image { x: f32, y: f32, w: f32, h: f32, img: usize },
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
    pub images: Vec<Arc<ImageData>>,
}

/// Picks a readable text colour for an author-chosen background: keeps `fg`
/// when it already contrasts, otherwise switches to dark or light text.
fn contrast_on(bg: Color, fg: Color) -> Color {
    let lum = |c: Color| 0.299 * c.r as f32 + 0.587 * c.g as f32 + 0.114 * c.b as f32;
    if (lum(bg) - lum(fg)).abs() >= 110.0 {
        fg
    } else if lum(bg) > 140.0 {
        Color::rgb(33, 37, 41)
    } else {
        Color::rgb(236, 239, 244)
    }
}

// ---------------------------------------------------------------- atoms

#[derive(Clone)]
struct Style {
    face: Face,
    size: f32,
    color: Color,
    link: Option<Rc<str>>,
    strike: bool,
    underline: bool,
    bg: Option<Color>,
    /// Baseline shift as a fraction of size (positive = up).
    shift: f32,
}

#[derive(Clone)]
struct Atom {
    text: String,
    style: Style,
    width: f32,
    space_before: bool,
    space_w: f32,
    /// Inline image: (slot in `Layout::images`, height).
    img: Option<(usize, f32)>,
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
        img: None,
    }
}

/// Resolves image URLs while flattening inline content.
struct Res<'a> {
    cfg: &'a Config,
    cache: &'a mut HashMap<String, Option<Arc<ImageData>>>,
    images: &'a mut Vec<Arc<ImageData>>,
}

fn slot_of(images: &mut Vec<Arc<ImageData>>, d: &Arc<ImageData>) -> usize {
    if let Some(i) = images.iter().position(|x| x.uid == d.uid) {
        return i;
    }
    images.push(d.clone());
    images.len() - 1
}

fn load_cached(
    cache: &mut HashMap<String, Option<Arc<ImageData>>>,
    cfg: &Config,
    url: &str,
) -> Option<Arc<ImageData>> {
    if let Some(c) = cache.get(url) {
        return c.clone();
    }
    let img = image::load(url, cfg.base_dir.as_deref());
    cache.insert(url.to_string(), img.clone());
    img
}

impl Res<'_> {
    /// Size an inline image: natural size at 96 dpi unless given, capped to stay line-like.
    fn inline_image(&mut self, url: &str, w: Option<Dim>, h: Option<Dim>) -> Option<(usize, f32, f32)> {
        let d = load_cached(self.cache, self.cfg, url)?;
        let (nw, nh) = (d.width as f32 * 0.75, d.height as f32 * 0.75);
        let pt = |x: Dim| match x {
            Dim::Pt(p) => p,
            Dim::Pct(p) => 400.0 * p / 100.0,
        };
        let (mut w, mut h) = match (w, h) {
            (Some(w), Some(h)) => (pt(w), pt(h)),
            (Some(w), None) => (pt(w), pt(w) * nh / nw),
            (None, Some(h)) => (pt(h) * nw / nh, pt(h)),
            (None, None) => (nw, nh),
        };
        let k = (360.0 / w).min(160.0 / h).min(1.0);
        w = (w * k).max(2.0);
        h = (h * k).max(2.0);
        Some((slot_of(self.images, &d), w, h))
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

fn flatten(inl: &[Inline], st: &Style, out: &mut Vec<Item>, pending: &mut bool, theme: &Theme, res: &mut Res) {
    for i in inl {
        match i {
            Inline::Text(t) => text_atoms(t, st, out, pending),
            Inline::Code(t) => {
                let s = Style {
                    face: Face::new(Family::Mono, st.face.bold, st.face.italic),
                    size: st.size * 0.9,
                    color: if st.link.is_some() { st.color } else { theme.code_text },
                    bg: Some(theme.code_bg),
                    ..st.clone()
                };
                text_atoms(t, &s, out, pending);
            }
            Inline::Emph(c) => {
                let s = Style { face: st.face.with(false, true), ..st.clone() };
                flatten(c, &s, out, pending, theme, res);
            }
            Inline::Strong(c) => {
                let s = Style { face: st.face.with(true, false), ..st.clone() };
                flatten(c, &s, out, pending, theme, res);
            }
            Inline::Strike(c) => {
                let s = Style { strike: true, ..st.clone() };
                flatten(c, &s, out, pending, theme, res);
            }
            Inline::Styled { style, content } => {
                let mut s = st.clone();
                if let Some(c) = style.color {
                    s.color = c;
                }
                if let Some(bg) = style.bg {
                    s.bg = Some(bg);
                    if style.color.is_none() {
                        s.color = contrast_on(bg, s.color);
                    }
                }
                if let Some(k) = style.size_scale {
                    s.size *= k;
                }
                if style.mono {
                    s.face = Face::new(Family::Mono, s.face.bold, s.face.italic);
                    if style.size_scale.is_none() {
                        s.size *= 0.9;
                    }
                }
                s.face = s.face.with(style.bold, style.italic);
                s.underline |= style.underline;
                s.strike |= style.strike;
                if style.shift != 0 {
                    s.shift = style.shift as f32 * 0.33;
                }
                flatten(content, &s, out, pending, theme, res);
            }
            Inline::Link { url, content } => {
                let s = Style { color: theme.link, link: Some(url.as_str().into()), underline: true, ..st.clone() };
                flatten(content, &s, out, pending, theme, res);
            }
            Inline::Image { url, alt, width, height, .. } => {
                if let Some((slot, w, h)) = res.inline_image(url, *width, *height) {
                    let mut a = make_atom("", st, *pending);
                    a.width = w;
                    a.img = Some((slot, h));
                    *pending = false;
                    out.push(Item::Atom(a));
                    continue;
                }
                let s = Style {
                    face: st.face.with(false, true),
                    color: theme.link,
                    link: Some(url.as_str().into()),
                    underline: true,
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
            Inline::RawHtml(_) => {}
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
        let mut a = match it {
            Item::Hard => {
                lines.push(std::mem::take(&mut cur));
                cur_w = 0.0;
                continue;
            }
            Item::Atom(a) => a,
        };
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
    atoms
        .iter()
        .filter(|a| a.img.is_none())
        .map(|a| a.style.size)
        .fold(0.0, f32::max)
        .max(if atoms.iter().all(|a| a.img.is_some()) { default } else { 0.0 })
}

fn img_height(atoms: &[Atom]) -> f32 {
    atoms.iter().filter_map(|a| a.img.map(|i| i.1)).fold(0.0, f32::max)
}

/// Height of one line: text leading, or taller if an inline image needs it.
fn line_h(atoms: &[Atom], default: f32, lead: f32) -> f32 {
    let size = line_size(atoms, default);
    (size * lead).max(if img_height(atoms) > 0.0 { img_height(atoms) + size * 0.5 } else { 0.0 })
}

// --------------------------------------------------------------- engine

#[derive(Clone, Copy)]
struct Frame {
    x: f32,
    w: f32,
}

#[derive(Clone, Copy)]
enum DecoKind {
    Fill(Color),
    Box { fill: Option<Color>, stroke: Option<(Color, f32)>, left: Option<(f32, Color)> },
}

#[derive(Clone)]
struct Deco {
    kind: DecoKind,
    x: f32,
    w: f32,
    op_idx: usize,
    start_y: f32,
}

struct Snapshot {
    pages: usize,
    last_ops: Vec<Op>,
    y: f32,
    decos: Vec<Deco>,
    headings: usize,
    first_content: Option<usize>,
}

struct ImgRef {
    data: Arc<ImageData>,
    title: Option<String>,
    width: Option<Dim>,
    height: Option<Dim>,
    link: Option<String>,
}

enum Seg {
    Text(Vec<Inline>),
    Images(Vec<ImgRef>),
}

struct Engine<'a> {
    cfg: &'a Config,
    pages: Vec<Page>,
    y: f32,
    decos: Vec<Deco>,
    headings: Vec<HeadingMark>,
    images: Vec<Arc<ImageData>>,
    image_cache: HashMap<String, Option<Arc<ImageData>>>,
    first_content: Option<usize>,
    // Inherited context (from HTML/CSS boxes and block quotes).
    color: Color,
    align: Align,
    scale: f32,
    bold: bool,
    italic: bool,
}

pub fn layout(doc: &Document, cfg: &Config) -> Layout {
    let mut e = Engine {
        cfg,
        pages: vec![Page::default()],
        y: cfg.margin_top,
        decos: Vec::new(),
        headings: Vec::new(),
        images: Vec::new(),
        image_cache: HashMap::new(),
        first_content: None,
        color: cfg.theme.text,
        align: Align::Left,
        scale: 1.0,
        bold: false,
        italic: false,
    };
    let fr = Frame { x: cfg.margin_left, w: (cfg.page_width - cfg.margin_left - cfg.margin_right).max(40.0) };
    e.blocks(&doc.blocks, fr, 0, false);
    e.flush_open_decos();
    if e.pages.len() > 1 && e.pages.last().is_some_and(|p| p.ops.is_empty()) {
        e.pages.pop();
    }
    let title = doc.title.clone().or_else(|| {
        doc.blocks.iter().find_map(|b| match b {
            Block::Heading { level: 1, content } => Some(plain_text(content)),
            _ => None,
        })
    });
    let height = if cfg.paginate { cfg.page_height } else { e.y + cfg.margin_bottom };
    let background = doc.page_bg.unwrap_or(cfg.theme.page_bg);
    if cfg.paginate {
        e.decorate_pages(title.as_deref());
    }
    Layout { width: cfg.page_width, height, background, pages: e.pages, headings: e.headings, title, images: e.images }
}

fn deco_ops(kind: DecoKind, x: f32, w: f32, y0: f32, y1: f32) -> Vec<Op> {
    let h = y1 - y0;
    match kind {
        DecoKind::Fill(c) => vec![Op::Rect { x, y: y0, w, h, fill: Some(c), stroke: None }],
        DecoKind::Box { fill, stroke, left } => {
            let mut v = Vec::new();
            if fill.is_some() || stroke.is_some() {
                v.push(Op::Rect { x, y: y0, w, h, fill, stroke });
            }
            if let Some((bw, c)) = left {
                v.push(Op::Rect { x, y: y0, w: bw, h, fill: Some(c), stroke: None });
            }
            v
        }
    }
}

impl<'a> Engine<'a> {
    fn smart(&self) -> bool {
        self.cfg.designed && self.cfg.paginate
    }

    fn base(&self) -> f32 {
        self.cfg.base_size * self.scale
    }

    fn lead(&self) -> f32 {
        self.cfg.leading
    }

    fn base_style(&self) -> Style {
        Style {
            face: Face::new(self.cfg.family, false, false).with(self.bold, self.italic),
            size: self.base(),
            color: self.color,
            link: None,
            strike: false,
            underline: false,
            bg: None,
            shift: 0.0,
        }
    }

    fn limit(&self) -> f32 {
        self.cfg.page_height - self.cfg.margin_bottom
    }

    fn at_top(&self) -> bool {
        self.y <= self.cfg.margin_top + 0.5
    }

    fn push(&mut self, op: Op) {
        if self.first_content.is_none() {
            self.first_content = Some(self.pages.len() - 1);
        }
        self.pages.last_mut().unwrap().ops.push(op);
    }

    fn ensure(&mut self, h: f32) {
        if self.cfg.paginate && self.y + h > self.limit() && !self.at_top() {
            self.new_page();
        }
    }

    fn page_break(&mut self) {
        if self.cfg.paginate && !self.at_top() {
            self.new_page();
        }
    }

    fn new_page(&mut self) {
        self.flush_decos();
        self.pages.push(Page::default());
        self.y = self.cfg.margin_top;
        for d in &mut self.decos {
            d.op_idx = 0;
            d.start_y = self.y;
        }
    }

    /// Emits the decorations that are open on the current page, ending at `self.y`.
    fn flush_decos(&mut self) {
        let end = self.y;
        for k in (0..self.decos.len()).rev() {
            let d = self.decos[k].clone();
            if end > d.start_y {
                let ops = &mut self.pages.last_mut().unwrap().ops;
                let at = d.op_idx.min(ops.len());
                for (n, op) in deco_ops(d.kind, d.x, d.w, d.start_y, end).into_iter().enumerate() {
                    ops.insert(at + n, op);
                }
            }
        }
    }

    fn flush_open_decos(&mut self) {
        self.flush_decos();
        self.decos.clear();
    }

    fn begin_deco(&mut self, kind: DecoKind, x: f32, w: f32) {
        let op_idx = self.pages.last().unwrap().ops.len();
        self.decos.push(Deco { kind, x, w, op_idx, start_y: self.y });
    }

    fn end_deco(&mut self) {
        let end = self.y;
        let d = self.decos.pop().unwrap();
        if end > d.start_y {
            let ops = &mut self.pages.last_mut().unwrap().ops;
            let at = d.op_idx.min(ops.len());
            for (n, op) in deco_ops(d.kind, d.x, d.w, d.start_y, end).into_iter().enumerate() {
                ops.insert(at + n, op);
            }
        }
    }

    // ---- snapshots (used to try a layout and roll it back) ----

    fn snap(&self) -> Snapshot {
        Snapshot {
            pages: self.pages.len(),
            last_ops: self.pages.last().unwrap().ops.clone(),
            y: self.y,
            decos: self.decos.clone(),
            headings: self.headings.len(),
            first_content: self.first_content,
        }
    }

    fn restore(&mut self, s: &Snapshot) {
        self.pages.truncate(s.pages);
        self.pages.last_mut().unwrap().ops = s.last_ops.clone();
        self.y = s.y;
        self.decos = s.decos.clone();
        self.headings.truncate(s.headings);
        self.first_content = s.first_content;
    }

    /// Lays out `f`; if it would straddle a page boundary but fits on a fresh
    /// page, starts it on a fresh page instead. `frac` limits how big a block
    /// may be (as a fraction of the page body) before we prefer natural flow
    /// over leaving a large gap; use `f32::INFINITY` to always keep it whole.
    fn keep(&mut self, frac: f32, f: &mut dyn FnMut(&mut Self)) {
        if !self.smart() {
            f(self);
            return;
        }
        let s = self.snap();
        f(self);
        if self.pages.len() == s.pages || s.y <= self.cfg.margin_top + 0.5 {
            return;
        }
        let body = self.limit() - self.cfg.margin_top;
        let gap = self.limit() - s.y;
        self.restore(&s);
        self.new_page();
        let y0 = self.y;
        f(self);
        if self.pages.len() > s.pages + 1 {
            // Taller than a page: accept the fresh start only if little is wasted.
            if gap <= 0.2 * body {
                return;
            }
            self.restore(&s);
            f(self);
            return;
        }
        let h = self.y - y0;
        if h <= frac * body || gap <= 0.3 * body {
            return;
        }
        self.restore(&s);
        f(self);
    }

    // ---- block flow ----

    fn blocks(&mut self, blocks: &[Block], fr: Frame, depth: usize, tight: bool) {
        let gap = self.base() * if tight { 0.3 } else if self.cfg.designed { 0.95 } else { 0.85 };
        let mut k = 0;
        while k < blocks.len() {
            if k > 0 && !(self.cfg.paginate && self.at_top()) {
                self.y += gap;
            }
            if self.smart() && matches!(blocks[k], Block::Heading { .. }) && k + 1 < blocks.len() {
                k = self.heading_unit(blocks, k, fr, depth, tight, gap);
            } else {
                self.block(&blocks[k], fr, depth, k == 0);
                k += 1;
            }
        }
    }

    /// Lays out a heading together with what follows it, and moves both to the
    /// next page if the heading would otherwise be stranded at the bottom.
    fn heading_unit(&mut self, blocks: &[Block], k: usize, fr: Frame, depth: usize, tight: bool, gap: f32) -> usize {
        let s0 = self.snap();
        let mut attempt = 0;
        loop {
            self.block(&blocks[k], fr, depth, k == 0);
            let heading_page = self.pages.len() - 1;
            self.y += gap;
            self.first_content = None;
            let next = if matches!(blocks[k + 1], Block::Heading { .. }) && k + 2 < blocks.len() {
                self.heading_unit(blocks, k + 1, fr, depth, tight, gap)
            } else {
                self.block(&blocks[k + 1], fr, depth, false);
                k + 2
            };
            let stranded = self.first_content.is_some_and(|p| p > heading_page);
            if stranded && attempt == 0 && s0.y > self.cfg.margin_top + 0.5 {
                self.restore(&s0);
                self.new_page();
                attempt += 1;
                continue;
            }
            return next;
        }
    }

    fn block(&mut self, b: &Block, fr: Frame, depth: usize, first: bool) {
        let base = self.base();
        match b {
            Block::Paragraph(inl) => self.paragraph(inl, fr),
            Block::Heading { level, content } => self.heading(*level, content, fr, first),
            Block::CodeBlock { lang, text } => {
                let (lang, text) = (lang.clone(), text.clone());
                self.keep(0.45, &mut |e| e.code_block(&lang, &text, fr));
            }
            Block::Quote(inner) => {
                let designed = self.cfg.designed;
                let theme = self.cfg.theme.clone();
                self.keep(0.5, &mut |e| {
                    let (bar, pad_x, pad_y) = (3.0, 10.0, if designed { 6.0 } else { 0.0 });
                    let kind = if designed {
                        DecoKind::Box { fill: Some(theme.quote_bg), stroke: None, left: Some((bar, theme.accent)) }
                    } else {
                        DecoKind::Fill(theme.border)
                    };
                    let w = if designed { fr.w } else { bar };
                    e.begin_deco(kind, fr.x, w);
                    e.y += pad_y;
                    let saved = e.color;
                    e.color = theme.muted;
                    let inset = if designed { bar + pad_x } else { 14.0 };
                    e.blocks(inner, Frame { x: fr.x + inset, w: fr.w - inset - if designed { pad_x } else { 0.0 } }, depth + 1, false);
                    e.color = saved;
                    e.y += pad_y;
                    e.end_deco();
                });
            }
            Block::Callout { kind, blocks } => self.callout(*kind, blocks, fr, depth),
            Block::List { ordered, start, tight, items } => self.list(*ordered, *start, *tight, items, fr, depth),
            Block::Table { aligns, header, rows, caption } => self.table(aligns, header, rows, caption.as_deref(), fr),
            Block::Div { style, blocks } => self.div(style, blocks, fr, depth),
            Block::PageBreak => self.page_break(),
            Block::Rule => {
                if self.smart() && self.at_top() && self.pages.len() > 1 {
                    return;
                }
                self.ensure(base * 0.9);
                let y = self.y + base * 0.45;
                let (c, x1, x2) = (self.cfg.theme.border, fr.x, fr.x + fr.w);
                self.push(Op::Line { x1, y1: y, x2, y2: y, color: c, width: 1.0 });
                self.y += base * 0.9;
            }
            Block::HtmlOpen { .. } | Block::HtmlClose { .. } | Block::RawHtml(_) => {}
        }
    }

    fn div(&mut self, st: &BoxStyle, blocks: &[Block], fr: Frame, depth: usize) {
        if st.break_before {
            self.page_break();
        }
        if st.margin_top > 0.0 && !self.at_top() {
            self.y += st.margin_top;
        }
        let saved = (self.color, self.align, self.scale, self.bold, self.italic);
        if let Some(c) = st.color {
            self.color = c;
        } else if let Some(bg) = st.bg {
            self.color = contrast_on(bg, self.color);
        }
        if let Some(a) = st.align {
            self.align = a;
        }
        if let Some(k) = st.size_scale {
            self.scale *= k;
        }
        self.bold |= st.bold;
        self.italic |= st.italic;
        let boxed = st.bg.is_some() || st.border.is_some() || st.border_left.is_some();
        let (mut pad_x, mut pad_y) = (st.pad_x, st.pad_y);
        if boxed {
            pad_x = pad_x.max(8.0);
            pad_y = pad_y.max(5.0);
        }
        let bl = st.border_left.map_or(0.0, |b| b.0);
        let (ox, ow) = (fr.x + st.indent, fr.w - st.indent);
        let inner = Frame { x: ox + pad_x + bl, w: (ow - 2.0 * pad_x - bl).max(30.0) };
        let kind = DecoKind::Box { fill: st.bg, stroke: st.border.map(|(w, c)| (c, w)), left: st.border_left };
        let mut run = |e: &mut Self| {
            if boxed {
                e.begin_deco(kind, ox, ow);
                e.y += pad_y;
            }
            e.blocks(blocks, inner, depth + 1, false);
            if boxed {
                e.y += pad_y;
                e.end_deco();
            }
        };
        if st.avoid_break {
            self.keep(f32::INFINITY, &mut run);
        } else if boxed {
            self.keep(0.5, &mut run);
        } else {
            run(self);
        }
        (self.color, self.align, self.scale, self.bold, self.italic) = saved;
        self.y += st.margin_bottom;
        if st.break_after {
            self.page_break();
        }
    }

    fn callout(&mut self, kind: CalloutKind, blocks: &[Block], fr: Frame, depth: usize) {
        let theme = self.cfg.theme.clone();
        let (name, c) = match kind {
            CalloutKind::Note => ("Note", Color::rgb(37, 99, 235)),
            CalloutKind::Tip => ("Tip", Color::rgb(22, 150, 74)),
            CalloutKind::Important => ("Important", Color::rgb(147, 51, 234)),
            CalloutKind::Warning => ("Warning", Color::rgb(200, 110, 6)),
            CalloutKind::Caution => ("Caution", Color::rgb(220, 38, 38)),
        };
        let dark = theme.page_bg.r < 128;
        let c = if dark { c.mix(Color::rgb(255, 255, 255), 0.35) } else { c };
        let fill = c.mix(theme.page_bg, if dark { 0.88 } else { 0.9 });
        let base = self.base();
        self.keep(f32::INFINITY, &mut |e| {
            let (bar, pad) = (3.5, 10.0);
            e.begin_deco(DecoKind::Box { fill: Some(fill), stroke: None, left: Some((bar, c)) }, fr.x, fr.w);
            e.y += pad * 0.8;
            let inner = Frame { x: fr.x + bar + pad, w: fr.w - bar - 2.0 * pad };
            let lh = base * e.lead();
            e.ensure(lh * 3.0);
            let face = Face::new(e.cfg.family, true, false);
            let y = e.y + lh / 2.0 + 0.36 * base;
            e.push(Op::Text { x: inner.x, y, face, size: base, color: c, text: name.to_string() });
            e.y += lh + base * 0.15;
            let saved = e.color;
            e.color = theme.text;
            e.blocks(blocks, inner, depth + 1, true);
            e.color = saved;
            e.y += pad * 0.8;
            e.end_deco();
        });
    }

    // ---- text ----

    fn paragraph(&mut self, inl: &[Inline], fr: Frame) {
        let segs = self.split_media(inl);
        let gap = self.base() * 0.5;
        for (n, seg) in segs.into_iter().enumerate() {
            if n > 0 {
                self.y += gap;
            }
            match seg {
                Seg::Text(v) => {
                    let st = self.base_style();
                    let items = self.items(&v, &st);
                    let lines = break_lines(items, fr.w);
                    let (align, base) = (self.align, self.base());
                    self.put_lines(&lines, fr, align, base);
                }
                Seg::Images(v) => self.image_row(v, fr),
            }
        }
    }

    fn load_image(&mut self, url: &str) -> Option<Arc<ImageData>> {
        if let Some(c) = self.image_cache.get(url) {
            return c.clone();
        }
        let img = image::load(url, self.cfg.base_dir.as_deref());
        self.image_cache.insert(url.to_string(), img.clone());
        img
    }

    /// Flattens inline content, resolving images against the document.
    fn items(&mut self, inl: &[Inline], st: &Style) -> Vec<Item> {
        let cfg = self.cfg;
        let mut res = Res { cfg, cache: &mut self.image_cache, images: &mut self.images };
        let mut out = Vec::new();
        flatten(inl, st, &mut out, &mut false, &cfg.theme, &mut res);
        out
    }

    /// A paragraph made only of loadable images becomes block-level image rows;
    /// anything else flows as text with the images inline.
    fn split_media(&mut self, inl: &[Inline]) -> Vec<Seg> {
        let is_blank = |i: &Inline| match i {
            Inline::Text(t) => t.trim().is_empty(),
            Inline::SoftBreak => true,
            _ => false,
        };
        let mut imgs = Vec::new();
        for i in inl.iter().filter(|i| !is_blank(i)) {
            let found = match i {
                Inline::Image { url, title, width, height, .. } => Some((url, title, width, height, None)),
                Inline::Link { url: lu, content } => {
                    let non_blank: Vec<&Inline> = content.iter().filter(|c| !is_blank(c)).collect();
                    match non_blank.as_slice() {
                        [Inline::Image { url, title, width, height, .. }] => Some((url, title, width, height, Some(lu.clone()))),
                        _ => None,
                    }
                }
                _ => None,
            };
            let Some((url, title, w, h, link)) = found else { return vec![Seg::Text(inl.to_vec())] };
            let Some(data) = self.load_image(url) else { return vec![Seg::Text(inl.to_vec())] };
            imgs.push(ImgRef { data, title: title.clone(), width: *w, height: *h, link });
        }
        if imgs.is_empty() {
            vec![Seg::Text(inl.to_vec())]
        } else {
            vec![Seg::Images(imgs)]
        }
    }

    fn image_row(&mut self, imgs: Vec<ImgRef>, fr: Frame) {
        let max_h = if self.cfg.paginate { (self.limit() - self.cfg.margin_top) * 0.8 } else { 1200.0 };
        let gap = 6.0;
        let single = imgs.len() == 1;
        // Resolve display sizes.
        let sized: Vec<(ImgRef, f32, f32)> = imgs
            .into_iter()
            .map(|im| {
                let (nw, nh) = (im.data.width as f32 * 0.75, im.data.height as f32 * 0.75);
                let dim = |d: Dim, whole: f32| match d {
                    Dim::Pt(p) => p,
                    Dim::Pct(p) => whole * p / 100.0,
                };
                let (mut w, mut h) = match (im.width, im.height) {
                    (Some(w), Some(h)) => (dim(w, fr.w), dim(h, max_h)),
                    (Some(w), None) => {
                        let w = dim(w, fr.w);
                        (w, w * nh / nw)
                    }
                    (None, Some(h)) => {
                        let h = dim(h, max_h);
                        (h * nw / nh, h)
                    }
                    (None, None) => (nw, nh),
                };
                let k = (fr.w / w).min(max_h / h).min(1.0);
                if k < 1.0 {
                    w *= k;
                    h *= k;
                }
                (im, w.max(2.0), h.max(2.0))
            })
            .collect();
        // Pack into rows.
        let mut rows: Vec<Vec<(ImgRef, f32, f32)>> = vec![Vec::new()];
        let mut cur_w = 0.0;
        for it in sized {
            let add = if rows.last().unwrap().is_empty() { it.1 } else { gap + it.1 };
            if !rows.last().unwrap().is_empty() && cur_w + add > fr.w + 0.01 {
                rows.push(Vec::new());
                cur_w = it.1;
            } else {
                cur_w += add;
            }
            rows.last_mut().unwrap().push(it);
        }
        let center_single = single && self.cfg.designed;
        let align = if center_single { Align::Center } else { self.align };
        let base = self.base();
        for (ri, row) in rows.into_iter().enumerate() {
            if ri > 0 {
                self.y += gap;
            }
            let total: f32 = row.iter().map(|r| r.1).sum::<f32>() + gap * (row.len() as f32 - 1.0);
            let rh = row.iter().map(|r| r.2).fold(0.0, f32::max);
            let caption = if single { row[0].0.title.clone().filter(|t| !t.trim().is_empty()) } else { None };
            let mut place = |e: &mut Self| {
                e.ensure(rh);
                let mut x = fr.x
                    + match align {
                        Align::Left => 0.0,
                        Align::Center => (fr.w - total) / 2.0,
                        Align::Right => fr.w - total,
                    };
                for (im, w, h) in &row {
                    let slot = e.image_slot(&im.data);
                    let y = e.y + (rh - h) / 2.0;
                    e.push(Op::Image { x, y, w: *w, h: *h, img: slot });
                    if let Some(l) = &im.link {
                        e.push(Op::Link { x, y, w: *w, h: *h, url: l.clone() });
                    }
                    x += w + gap;
                }
                e.y += rh;
                if let Some(c) = &caption {
                    e.y += base * 0.35;
                    let st = Style {
                        face: Face::new(e.cfg.family, false, true),
                        size: base * 0.88,
                        color: e.cfg.theme.muted,
                        link: None,
                        strike: false,
                        underline: false,
                        bg: None,
                        shift: 0.0,
                    };
                    let mut items = Vec::new();
                    text_atoms(c, &st, &mut items, &mut false);
                    let lines = break_lines(items, fr.w);
                    e.put_lines(&lines, fr, Align::Center, base * 0.88);
                }
            };
            self.keep(f32::INFINITY, &mut place);
        }
    }

    fn image_slot(&mut self, d: &Arc<ImageData>) -> usize {
        if let Some(i) = self.images.iter().position(|x| x.uid == d.uid) {
            return i;
        }
        self.images.push(d.clone());
        self.images.len() - 1
    }

    /// Places wrapped lines, honouring page boundaries. In smart mode no
    /// single line is left alone at the end or start of a page.
    fn put_lines(&mut self, lines: &[Vec<Atom>], fr: Frame, align: Align, base: f32) {
        let hs: Vec<f32> = lines.iter().map(|l| line_h(l, base, self.lead())).collect();
        self.put_lines_min(lines, &hs, fr, align, base, 2);
    }

    fn put_lines_min(&mut self, lines: &[Vec<Atom>], hs: &[f32], fr: Frame, align: Align, base: f32, min: usize) {
        let n = lines.len();
        let mut i = 0;
        while i < n {
            let take = if self.cfg.paginate { self.fit_count(&hs[i..], min, n - i) } else { n - i };
            if take == 0 {
                self.new_page();
                continue;
            }
            for k in i..i + take {
                self.emit_line(&lines[k], fr.x, self.y, fr.w, align, base);
                self.y += hs[k];
            }
            i += take;
            if i < n {
                self.new_page();
            }
        }
    }

    /// How many of the remaining lines (heights `hs`) to put on this page.
    /// 0 means "start a new page first". `min` is the widow/orphan minimum.
    fn fit_count(&self, hs: &[f32], min: usize, remaining: usize) -> usize {
        if !self.cfg.paginate {
            return hs.len();
        }
        let avail = self.limit() - self.y;
        let mut acc = 0.0;
        let mut k = 0;
        while k < hs.len() && acc + hs[k] <= avail + 0.01 {
            acc += hs[k];
            k += 1;
        }
        if k == hs.len() {
            return k;
        }
        if !self.smart() {
            return if k == 0 && self.at_top() { 1 } else { k };
        }
        let mut take = k;
        if remaining - take < min {
            take = take.saturating_sub(min - (remaining - take));
        }
        if take < min {
            take = 0;
        }
        if take == 0 && self.at_top() {
            return k.max(1);
        }
        take
    }

    /// Draws one line of atoms with its top at `y`; returns the line height.
    fn emit_line(&mut self, atoms: &[Atom], x: f32, y: f32, avail: f32, align: Align, base: f32) -> f32 {
        let size = line_size(atoms, base);
        let lh = line_h(atoms, base, self.lead());
        let ih = img_height(atoms);
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
        let baseline = if ih > 0.0 { y + lh - 0.3 * size } else { y + lh / 2.0 + 0.36 * size };
        let mut prev: Option<&Atom> = None;
        for a in atoms {
            let gap = if prev.is_some() && a.space_before { a.space_w } else { 0.0 };
            let x0 = cx + gap;
            let st = &a.style;
            let same = |f: &dyn Fn(&Atom) -> bool| prev.is_some_and(|p| f(p) && f(a));
            let by = baseline - st.shift * st.size;
            if let Some((slot, h)) = a.img {
                self.push(Op::Image { x: x0, y: baseline - h, w: a.width, h, img: slot });
                if let Some(url) = &st.link {
                    self.push(Op::Link { x: x0, y: baseline - h, w: a.width, h, url: url.to_string() });
                }
                cx = x0 + a.width;
                prev = Some(a);
                continue;
            }
            if let Some(bg) = st.bg {
                let start = if same(&|q| q.style.bg == Some(bg)) { cx } else { x0 - 1.5 };
                self.push(Op::Rect {
                    x: start,
                    y: by - st.size * 0.88,
                    w: x0 + a.width + 1.5 - start,
                    h: st.size * 1.2,
                    fill: Some(bg),
                    stroke: None,
                });
            }
            self.push(Op::Text { x: x0, y: by, face: st.face, size: st.size, color: st.color, text: a.text.clone() });
            if st.underline {
                let link = st.link.clone();
                let start = if same(&|q| q.style.underline && q.style.link == link) { cx } else { x0 };
                let uy = by + st.size * 0.13;
                self.push(Op::Line { x1: start, y1: uy, x2: x0 + a.width, y2: uy, color: st.color, width: 0.5 + st.size * 0.03 });
            }
            if let Some(url) = &st.link {
                let start = if same(&|q| q.style.link.as_deref() == Some(url)) { cx } else { x0 };
                self.push(Op::Link { x: start, y: by - st.size * 0.9, w: x0 + a.width - start, h: st.size * 1.2, url: url.to_string() });
            }
            if st.strike {
                let start = if same(&|q| q.style.strike) { cx } else { x0 };
                let sy = by - st.size * 0.28;
                self.push(Op::Line { x1: start, y1: sy, x2: x0 + a.width, y2: sy, color: st.color, width: 0.5 + st.size * 0.03 });
            }
            cx = x0 + a.width;
            prev = Some(a);
        }
        lh
    }

    fn heading(&mut self, level: u8, content: &[Inline], fr: Frame, first: bool) {
        let designed = self.cfg.designed;
        let mult = if designed { [2.15, 1.6, 1.3, 1.14, 1.0, 0.92] } else { [2.0, 1.55, 1.3, 1.12, 1.0, 0.92] }
            [(level.clamp(1, 6) - 1) as usize];
        let size = self.base() * mult;
        let theme = self.cfg.theme.clone();
        let st = Style {
            face: Face::new(self.cfg.family, true, false),
            size,
            color: if level == 6 { theme.muted } else { theme.heading },
            link: None,
            strike: false,
            underline: false,
            bg: None,
            shift: 0.0,
        };
        let items = self.items(content, &st);
        let lines = break_lines(items, fr.w);
        if !first {
            self.y += size * if designed { 0.6 } else { 0.45 };
        }
        let lh = size * self.lead().min(1.35);
        self.ensure(lh * lines.len().max(1) as f32 + self.base() * self.lead() * 2.0);
        self.headings.push(HeadingMark { level, title: plain_text(content), page: self.pages.len() - 1, y: self.y });
        let hs: Vec<f32> = lines.iter().map(|l| line_h(l, size, self.lead().min(1.35))).collect();
        let align = self.align;
        self.put_lines_min(&lines, &hs, fr, align, size, 1);
        if designed && level == 1 {
            self.y += 4.0;
            let y = self.y;
            let bx = if self.align == Align::Center { fr.x + (fr.w - 42.0) / 2.0 } else { fr.x };
            self.push(Op::Rect { x: bx, y, w: 42.0, h: 3.0, fill: Some(theme.accent), stroke: None });
            self.y += 7.0;
        } else if level <= 2 {
            self.y += 3.0;
            let y = self.y;
            self.push(Op::Line { x1: fr.x, y1: y, x2: fr.x + fr.w, y2: y, color: theme.border, width: 0.8 });
            self.y += 3.0;
        }
    }

    fn code_block(&mut self, lang: &str, text: &str, fr: Frame) {
        let designed = self.cfg.designed;
        let size = self.base() * 0.85;
        let face = Face::new(Family::Mono, false, false);
        let pad = 8.0;
        let left = if designed { 3.0 } else { 0.0 };
        let cw = face.char_width('m', size);
        let max_chars = (((fr.w - 2.0 * pad - left) / cw).floor() as usize).max(1);
        let lh = size * 1.45;
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
        let label = designed && !lang.is_empty();
        let top_pad = pad + if label { 5.0 } else { 0.0 };
        self.ensure(lh + top_pad);
        let kind = if designed {
            DecoKind::Box { fill: Some(theme.code_bg), stroke: Some((theme.border, 0.5)), left: Some((left, theme.accent)) }
        } else {
            DecoKind::Fill(theme.code_bg)
        };
        self.begin_deco(kind, fr.x, fr.w);
        if label {
            let lf = Face::new(Family::Sans, true, false);
            let lsz = 7.0;
            let t = sanitize(lang).to_uppercase();
            let w = lf.width(&t, lsz);
            let y = self.y + 11.0;
            self.push(Op::Text { x: fr.x + fr.w - pad - w, y, face: lf, size: lsz, color: theme.muted, text: t });
        }
        self.y += top_pad;
        // Code is split with at least 3 lines on each side.
        let hs = vec![lh; vis.len()];
        let n = vis.len();
        let mut i = 0;
        while i < n {
            let take = self.fit_count(&hs[i..], 3, n - i);
            if take == 0 {
                self.new_page();
                continue;
            }
            for l in &vis[i..i + take] {
                if !l.is_empty() {
                    let y = self.y + lh / 2.0 + 0.36 * size;
                    self.push(Op::Text { x: fr.x + pad + left, y, face, size, color: theme.code_text, text: l.clone() });
                }
                self.y += lh;
            }
            i += take;
            if i < n {
                self.new_page();
            }
        }
        self.y += pad;
        self.end_deco();
    }

    fn list(&mut self, ordered: bool, start: u32, tight: bool, items: &[ListItem], fr: Frame, depth: usize) {
        let base = self.base();
        let face = Face::new(self.cfg.family, false, false).with(self.bold, self.italic);
        let label = |n: u32| format!("{n}.");
        let widest = if ordered {
            (0..items.len()).map(|k| face.width(&label(start + k as u32), base)).fold(0.0, f32::max)
        } else {
            0.0
        };
        let indent = (widest + 0.8 * base).max(1.7 * base);
        let lh = base * self.lead();
        let gap = base * if tight { 0.3 } else { 0.85 };
        for (k, item) in items.iter().enumerate() {
            if k > 0 {
                self.y += gap;
            }
            let mut place = |e: &mut Self| {
                e.ensure(lh * 1.5);
                let top = e.y;
                let color = e.color;
                let theme = e.cfg.theme.clone();
                let marker_color = if e.cfg.designed { theme.accent } else { color };
                let mx = fr.x + indent;
                if let Some(done) = item.checked {
                    let s = base * 0.8;
                    let (bx, by) = (mx - 0.62 * base - s / 2.0, top + (lh - s) / 2.0);
                    e.push(Op::Rect { x: bx, y: by, w: s, h: s, fill: None, stroke: Some((theme.muted, 0.9)) });
                    if done {
                        let l = |x1: f32, y1: f32, x2: f32, y2: f32| Op::Line { x1, y1, x2, y2, color: theme.link, width: 1.4 };
                        e.push(l(bx + s * 0.2, by + s * 0.55, bx + s * 0.42, by + s * 0.78));
                        e.push(l(bx + s * 0.42, by + s * 0.78, bx + s * 0.82, by + s * 0.22));
                    }
                } else if ordered {
                    let text = label(start + k as u32);
                    let w = face.width(&text, base);
                    let y = top + lh / 2.0 + 0.36 * base;
                    e.push(Op::Text { x: mx - 0.4 * base - w, y, face, size: base, color: marker_color, text });
                } else {
                    let r = base * 0.17;
                    let (cx, cy) = (mx - 0.62 * base, top + lh / 2.0 + 0.03 * base);
                    match depth % 3 {
                        0 => e.push(Op::Circle { cx, cy, r, fill: true, color: marker_color, width: 1.0 }),
                        1 => e.push(Op::Circle { cx, cy, r, fill: false, color: marker_color, width: 0.8 }),
                        _ => e.push(Op::Rect { x: cx - r, y: cy - r, w: 2.0 * r, h: 2.0 * r, fill: Some(marker_color), stroke: None }),
                    }
                }
                if item.checked == Some(true) {
                    e.color = theme.muted;
                }
                e.blocks(&item.blocks, Frame { x: mx, w: fr.w - indent }, depth + 1, tight);
                e.color = color;
            };
            self.keep(0.2, &mut place);
        }
    }

    // ---- tables ----

    fn table(&mut self, aligns: &[Align], header: &[Vec<Inline>], rows: &[Vec<Vec<Inline>>], caption: Option<&[Inline]>, fr: Frame) {
        let designed = self.cfg.designed;
        let ncol = aligns.len().max(1);
        let theme = self.cfg.theme.clone();
        let base = self.base();
        let (px, py) = if designed { (8.0f32, 5.0f32) } else { (6.0, 4.0) };
        let has_header = !header.is_empty();
        let mut st = self.base_style();
        let mut hst = Style { face: st.face.with(true, false), ..st.clone() };
        if designed {
            hst.color = theme.on_accent;
        }
        let avail = (fr.w - ncol as f32 * 2.0 * px).max(ncol as f32 * 10.0);
        // Shrink the font a little before resorting to breaking words.
        let mut shrink = 1.0f32;
        let (items_all, min, max) = loop {
            st.size = base * shrink;
            hst.size = base * shrink;
            let mut all: Vec<Vec<Vec<Item>>> = Vec::new();
            if has_header {
                let row: Vec<Vec<Item>> = (0..ncol).map(|c| header.get(c).map_or(Vec::new(), |cell| self.items(cell, &hst))).collect();
                all.push(row);
            }
            for r in rows {
                let row: Vec<Vec<Item>> = (0..ncol).map(|c| r.get(c).map_or(Vec::new(), |cell| self.items(cell, &st))).collect();
                all.push(row);
            }
            let mut min = vec![0f32; ncol];
            let mut max = vec![0f32; ncol];
            for r in &all {
                for (c, cell) in r.iter().enumerate() {
                    let (mn, mx) = measure(cell);
                    min[c] = min[c].max(mn);
                    max[c] = max[c].max(mx);
                }
            }
            let smin: f32 = min.iter().sum();
            if smin > avail && shrink > 0.78 && designed {
                shrink = (shrink - 0.07).max(0.78);
                continue;
            }
            break (all, min, max);
        };
        let (smin, smax): (f32, f32) = (min.iter().sum(), max.iter().sum());
        let widths: Vec<f32> = if smax <= avail {
            max.clone()
        } else if smin >= avail {
            min.iter().map(|m| m / smin * avail).collect()
        } else {
            // Short columns keep their natural width so labels like "Team C"
            // never wrap; the rest share what is left between min and max.
            let short: Vec<bool> = (0..ncol).map(|c| max[c] <= 0.16 * avail).collect();
            let fixed: f32 = (0..ncol).filter(|&c| short[c]).map(|c| max[c]).sum();
            let rest_min: f32 = (0..ncol).filter(|&c| !short[c]).map(|c| min[c]).sum();
            if fixed + rest_min <= avail && short.iter().any(|&b| !b) {
                let extra = avail - fixed - rest_min;
                let wsum: f32 = (0..ncol).filter(|&c| !short[c]).map(|c| max[c] - min[c]).sum::<f32>().max(0.001);
                (0..ncol)
                    .map(|c| if short[c] { max[c] } else { min[c] + extra * (max[c] - min[c]) / wsum })
                    .collect()
            } else {
                let extra = avail - smin;
                let wsum: f32 = (0..ncol).map(|c| max[c] - min[c]).sum();
                (0..ncol).map(|c| min[c] + extra * (max[c] - min[c]) / wsum).collect()
            }
        };
        // In Designed mode, stretch narrow tables to the full width.
        let widths: Vec<f32> = if designed && smax < avail {
            let extra = (avail - smax) / ncol as f32;
            widths.iter().map(|w| w + extra).collect()
        } else {
            widths
        };
        struct Row {
            cells: Vec<Vec<Vec<Atom>>>,
            h: f32,
        }
        let lead = self.lead();
        let laid: Vec<Row> = items_all
            .into_iter()
            .map(|r| {
                let cells: Vec<Vec<Vec<Atom>>> = r.into_iter().enumerate().map(|(c, items)| break_lines(items, widths[c])).collect();
                let h = cells
                    .iter()
                    .map(|ls| ls.iter().map(|l| line_h(l, base, lead)).sum::<f32>().max(base * shrink * lead))
                    .fold(0.0, f32::max)
                    + 2.0 * py;
                Row { cells, h }
            })
            .collect();
        let table_w: f32 = widths.iter().map(|w| w + 2.0 * px).sum();
        let tx = fr.x;
        let draw = |e: &mut Self, row: &Row, idx: usize, head: bool| {
            let mut x = tx;
            let y = e.y;
            if designed {
                let fill = if head {
                    Some(theme.table_header_bg)
                } else if idx % 2 == 0 {
                    Some(theme.zebra)
                } else {
                    None
                };
                if fill.is_some() {
                    e.push(Op::Rect { x, y, w: table_w, h: row.h, fill, stroke: None });
                }
                let ly = y + row.h;
                e.push(Op::Line { x1: x, y1: ly, x2: x + table_w, y2: ly, color: theme.border, width: 0.6 });
            }
            for c in 0..ncol {
                let cw = widths[c] + 2.0 * px;
                if !designed {
                    e.push(Op::Rect {
                        x,
                        y,
                        w: cw,
                        h: row.h,
                        fill: head.then_some(theme.table_header_bg),
                        stroke: Some((theme.border, 0.6)),
                    });
                }
                let mut ly = y + py;
                for line in &row.cells[c] {
                    ly += e.emit_line(line, x + px, ly, widths[c], aligns.get(c).copied().unwrap_or(Align::Left), base);
                }
                x += cw;
            }
            e.y += row.h;
        };
        if let Some(cap) = caption {
            let cst = Style { face: st.face.with(false, true), color: theme.muted, size: base * 0.92, ..st.clone() };
            let items = self.items(cap, &cst);
            let lines = break_lines(items, fr.w);
            let hs: Vec<f32> = lines.iter().map(|l| line_h(l, base, lead)).collect();
            self.ensure(hs.iter().sum::<f32>() + laid.first().map_or(0.0, |r| r.h) + laid.get(1).map_or(0.0, |r| r.h));
            self.put_lines_min(&lines, &hs, fr, Align::Left, base, 1);
            self.y += base * 0.3;
        }
        let hdr = has_header as usize;
        let hh = if has_header { laid[0].h } else { 0.0 };
        let mut place = |e: &mut Self| {
            let body = e.limit() - e.cfg.margin_top;
            let pack = |first_avail: f32| -> Vec<Vec<usize>> {
                let mut groups: Vec<Vec<usize>> = vec![Vec::new()];
                let mut avail = first_avail - hh;
                for r in hdr..laid.len() {
                    let h = laid[r].h;
                    if h <= avail + 0.01 || (groups.last().unwrap().is_empty() && avail >= body - hh - 0.01) {
                        groups.last_mut().unwrap().push(r);
                        avail -= h;
                    } else {
                        groups.push(vec![r]);
                        avail = body - hh - h;
                    }
                }
                groups
            };
            let mut groups = if e.cfg.paginate { pack(e.limit() - e.y) } else { vec![(hdr..laid.len()).collect()] };
            let mut break_first = false;
            if e.cfg.paginate {
                if groups[0].is_empty() && groups.len() > 1 {
                    groups.remove(0);
                    break_first = !e.at_top();
                }
                if e.smart() {
                    // No stranded first row: start the table on a fresh page instead.
                    if groups.len() > 1 && groups[0].len() < 2 && !e.at_top() && !break_first {
                        groups = pack(body);
                        break_first = true;
                    }
                    // No stranded last row: pull one down from the previous page.
                    let n = groups.len();
                    if n > 1 && groups[n - 1].len() == 1 && groups[n - 2].len() >= 3 {
                        let moved = groups[n - 2].pop().unwrap();
                        groups[n - 1].insert(0, moved);
                    }
                }
            }
            if break_first {
                e.new_page();
            }
            if groups.iter().all(|g| g.is_empty()) && has_header {
                e.ensure(hh);
                draw(e, &laid[0], 0, true);
                return;
            }
            for (gi, g) in groups.iter().enumerate() {
                if gi > 0 {
                    e.new_page();
                }
                if has_header {
                    e.ensure(hh + g.first().map_or(0.0, |&r| laid[r].h));
                    draw(e, &laid[0], 0, true);
                }
                for &r in g {
                    e.ensure(laid[r].h);
                    draw(e, &laid[r], r - hdr, false);
                }
            }
        };
        self.keep(f32::INFINITY, &mut place);
    }

    // ---- page furniture ----

    fn decorate_pages(&mut self, title: Option<&str>) {
        let cfg = self.cfg;
        let n = self.pages.len();
        let face = Face::new(cfg.family, false, false);
        let size = 8.5;
        for (i, p) in self.pages.iter_mut().enumerate() {
            if cfg.designed && cfg.header {
                if i > 0 {
                    if let Some(t) = title {
                        let t = sanitize(t);
                        p.ops.push(Op::Text { x: cfg.margin_left, y: cfg.margin_top / 2.0 + 2.0, face, size, color: cfg.theme.muted, text: t });
                        let y = cfg.margin_top / 2.0 + 9.0;
                        p.ops.push(Op::Line {
                            x1: cfg.margin_left,
                            y1: y,
                            x2: cfg.page_width - cfg.margin_right,
                            y2: y,
                            color: cfg.theme.border,
                            width: 0.6,
                        });
                    }
                }
                if cfg.page_numbers {
                    let y = cfg.page_height - cfg.margin_bottom / 2.0 + 3.0;
                    let label = format!("Page {} of {}", i + 1, n);
                    let w = face.width(&label, size);
                    p.ops.push(Op::Line {
                        x1: cfg.margin_left,
                        y1: y - 11.0,
                        x2: cfg.page_width - cfg.margin_right,
                        y2: y - 11.0,
                        color: cfg.theme.border,
                        width: 0.6,
                    });
                    p.ops.push(Op::Text { x: cfg.page_width - cfg.margin_right - w, y, face, size, color: cfg.theme.muted, text: label });
                }
            } else if cfg.page_numbers {
                let label = format!("{} / {}", i + 1, n);
                let w = face.width(&label, 9.0);
                p.ops.push(Op::Text {
                    x: (cfg.page_width - w) / 2.0,
                    y: cfg.page_height - cfg.margin_bottom / 2.0 + 3.0,
                    face,
                    size: 9.0,
                    color: cfg.theme.muted,
                    text: label,
                });
            }
        }
    }
}
