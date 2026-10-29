//! `mdpdf-core`: a dependency-free Markdown/HTML -> layout -> PDF pipeline.
//!
//! ```
//! let cfg = mdpdf_core::Config::a4();
//! let pdf = mdpdf_core::markdown_to_pdf("# Hello\n\nSome *text*.", &cfg);
//! assert!(pdf.starts_with(b"%PDF-"));
//! ```
//!
//! Pipeline: parse -> [`layout::layout`] -> [`pdf::write_pdf`]. The GUI
//! viewer stops after `layout` and paints the same ops on screen.
//!
//! Two conversion modes, chosen through [`Config`]:
//! * **Standard** (`Config::a4()`): plain Markdown.
//! * **Designed** (`Config::designed_a4()`): additionally understands HTML
//!   (embedded in Markdown or as a whole document) and a CSS subset, GitHub
//!   callouts, applies a refined theme and smart pagination.

pub mod ast;
pub mod block;
pub mod css;
pub mod deflate;
pub mod fonts;
pub mod html;
pub mod image;
pub mod inline;
pub mod layout;
mod metrics_data;
pub mod pdf;

pub use ast::{Block, Document, Inline};
pub use block::{parse, parse_with, ParseOptions};
pub use fonts::Family;
pub use layout::{layout, Color, Config, Layout, Op, Theme};

/// What kind of source text we are converting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Markdown,
    Html,
}

/// The conversion mode. HTML input is only available in `Designed`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Standard,
    Designed,
}

impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Mode::Standard => "Standard (Markdown)",
            Mode::Designed => "Designed (Markdown + HTML)",
        }
    }
}

/// Parses `src` according to the mode carried by `cfg.designed`.
pub fn parse_source(src: &str, kind: Source, cfg: &Config) -> Result<Document, String> {
    match (kind, cfg.designed) {
        (Source::Markdown, false) => Ok(parse(src)),
        (Source::Markdown, true) => Ok(parse_with(src, &ParseOptions { html: true, callouts: true })),
        (Source::Html, true) => Ok(html::parse_document(src)),
        (Source::Html, false) => Err("HTML input needs Designed mode (opt in with --mode designed)".into()),
    }
}

/// Source -> layout.
pub fn render(src: &str, kind: Source, cfg: &Config) -> Result<Layout, String> {
    Ok(layout(&parse_source(src, kind, cfg)?, cfg))
}

/// Source -> PDF bytes.
pub fn convert(src: &str, kind: Source, cfg: &Config) -> Result<Vec<u8>, String> {
    render(src, kind, cfg).map(|l| pdf::write_pdf(&l))
}

/// Converts Markdown source straight to PDF bytes.
pub fn markdown_to_pdf(src: &str, cfg: &Config) -> Vec<u8> {
    convert(src, Source::Markdown, cfg).expect("Markdown is accepted in every mode")
}
