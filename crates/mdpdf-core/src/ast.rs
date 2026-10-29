//! Document tree produced by the Markdown and HTML parsers.

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
    /// Linear blend: `t` = 0 gives `self`, 1 gives `other`.
    pub fn mix(self, other: Color, t: f32) -> Color {
        let m = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round().clamp(0.0, 255.0) as u8;
        Color::rgb(m(self.r, other.r), m(self.g, other.g), m(self.b, other.b))
    }
}

/// A length that may be absolute (points) or relative to the available width.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Dim {
    Pt(f32),
    Pct(f32),
}

/// Style deltas applied to inline content (from HTML tags / CSS).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct InlineStyle {
    pub color: Option<Color>,
    pub bg: Option<Color>,
    /// Multiplier on the surrounding font size.
    pub size_scale: Option<f32>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub mono: bool,
    /// +1 superscript, -1 subscript.
    pub shift: i8,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Inline {
    Text(String),
    Code(String),
    Emph(Vec<Inline>),
    Strong(Vec<Inline>),
    Strike(Vec<Inline>),
    Styled { style: InlineStyle, content: Vec<Inline> },
    Link { url: String, content: Vec<Inline> },
    Image {
        url: String,
        alt: String,
        title: Option<String>,
        width: Option<Dim>,
        height: Option<Dim>,
    },
    SoftBreak,
    HardBreak,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ListItem {
    /// `Some(done)` for GFM task list items (`- [x] ...`).
    pub checked: Option<bool>,
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalloutKind {
    Note,
    Tip,
    Important,
    Warning,
    Caution,
}

/// Box-level style (HTML `div`/`section`/... plus the CSS subset we honour).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BoxStyle {
    pub color: Option<Color>,
    pub bg: Option<Color>,
    pub align: Option<Align>,
    pub size_scale: Option<f32>,
    pub bold: bool,
    pub italic: bool,
    pub pad_x: f32,
    pub pad_y: f32,
    pub indent: f32,
    pub margin_top: f32,
    pub margin_bottom: f32,
    pub border: Option<(f32, Color)>,
    pub border_left: Option<(f32, Color)>,
    pub break_before: bool,
    pub break_after: bool,
    pub avoid_break: bool,
}

impl BoxStyle {
    pub fn is_plain(&self) -> bool {
        *self == BoxStyle::default()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    Heading { level: u8, content: Vec<Inline> },
    Paragraph(Vec<Inline>),
    CodeBlock { lang: String, text: String },
    Quote(Vec<Block>),
    Callout { kind: CalloutKind, blocks: Vec<Block> },
    List {
        ordered: bool,
        start: u32,
        tight: bool,
        items: Vec<ListItem>,
    },
    Table {
        aligns: Vec<Align>,
        /// Empty when the table has no header row.
        header: Vec<Vec<Inline>>,
        rows: Vec<Vec<Vec<Inline>>>,
        caption: Option<Vec<Inline>>,
    },
    Div { style: BoxStyle, blocks: Vec<Block> },
    PageBreak,
    Rule,
    /// Parser-internal: an unbalanced HTML container tag seen in a Markdown
    /// HTML block. Folded into `Div` before parsing returns.
    HtmlOpen { tag: String, style: BoxStyle },
    HtmlClose { tag: String },
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Document {
    pub blocks: Vec<Block>,
    pub title: Option<String>,
    /// Page background requested by an HTML `<body>`.
    pub page_bg: Option<Color>,
}

/// Flattens inline content to plain text (used for outline titles, image alt text).
pub fn plain_text(inlines: &[Inline]) -> String {
    let mut out = String::new();
    for i in inlines {
        match i {
            Inline::Text(t) | Inline::Code(t) => out.push_str(t),
            Inline::Emph(c) | Inline::Strong(c) | Inline::Strike(c) => out.push_str(&plain_text(c)),
            Inline::Styled { content, .. } | Inline::Link { content, .. } => out.push_str(&plain_text(content)),
            Inline::Image { alt, .. } => out.push_str(alt),
            Inline::SoftBreak | Inline::HardBreak => out.push(' '),
        }
    }
    out
}
