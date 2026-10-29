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
//! Conversion modes, chosen through [`Config`]:
//! * **Standard** (`Config::a4()`): plain Markdown, built-in engine.
//! * **Designed** (`Config::designed_a4()`): opt-in. Markdown *and* HTML are
//!   rendered by an installed Chromium-based browser (full HTML/CSS) with print
//!   rules that never cut headings, tables, images or code across pages.
//! * **Designed, built-in** (`Config::designed_builtin_a4()`): the same look
//!   from our own HTML/CSS-subset engine and smart pagination; works offline.

pub mod ast;
pub mod block;
pub mod browser;
pub mod css;
pub mod deflate;
pub mod fonts;
pub mod html;
pub mod html_out;
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

/// The conversion mode. HTML input is only available in the Designed modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Standard,
    /// Opt-in: rendered by a Chromium-based browser.
    Designed,
    /// Opt-in: our own HTML/CSS-subset engine (offline).
    DesignedBuiltin,
}

impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Mode::Standard => "Standard (Markdown)",
            Mode::Designed => "Designed (HTML engine)",
            Mode::DesignedBuiltin => "Designed (built-in, offline)",
        }
    }

    /// A4 or Letter configuration for this mode.
    pub fn config(self, letter: bool) -> Config {
        match (self, letter) {
            (Mode::Standard, false) => Config::a4(),
            (Mode::Standard, true) => Config::letter(),
            (Mode::Designed, false) => Config::designed_a4(),
            (Mode::Designed, true) => Config::designed_letter(),
            (Mode::DesignedBuiltin, false) => Config::designed_builtin_a4(),
            (Mode::DesignedBuiltin, true) => Config::designed_builtin_letter(),
        }
    }
}

const HTML_NEEDS_DESIGNED: &str = "HTML input needs Designed mode (opt in with --mode designed)";

/// Parses `src` for the built-in engine according to `cfg.designed`.
pub fn parse_source(src: &str, kind: Source, cfg: &Config) -> Result<Document, String> {
    match (kind, cfg.designed) {
        (Source::Markdown, false) => Ok(parse(src)),
        (Source::Markdown, true) => Ok(parse_with(src, &ParseOptions { html: true, callouts: true, raw_html: false })),
        (Source::Html, true) => Ok(html::parse_document(src)),
        (Source::Html, false) => Err(HTML_NEEDS_DESIGNED.into()),
    }
}

/// Source -> layout using the built-in engine (also what the viewer previews).
pub fn render(src: &str, kind: Source, cfg: &Config) -> Result<Layout, String> {
    Ok(layout(&parse_source(src, kind, cfg)?, cfg))
}

/// Source -> PDF through the external browser (Designed mode).
fn browser_convert(src: &str, kind: Source, cfg: &Config) -> Result<Vec<u8>, String> {
    let base = cfg.base_dir.as_deref().and_then(|d| d.canonicalize().ok()).map(|d| browser::dir_url(&d));
    let page = match kind {
        Source::Markdown => {
            let doc = parse_with(src, &ParseOptions { html: false, callouts: true, raw_html: true });
            html_out::markdown_page(&doc, cfg, base.as_deref())
        }
        Source::Html => html_out::html_page(src, cfg, base.as_deref()),
    };
    browser::print_pdf(&page, cfg)
}

/// Source -> PDF bytes. Designed configs with `browser: true` print through
/// the browser; everything else uses the built-in engine.
pub fn convert(src: &str, kind: Source, cfg: &Config) -> Result<Vec<u8>, String> {
    if cfg.designed && cfg.browser {
        return browser_convert(src, kind, cfg);
    }
    render(src, kind, cfg).map(|l| pdf::write_pdf(&l))
}

/// Converts Markdown source straight to PDF bytes.
pub fn markdown_to_pdf(src: &str, cfg: &Config) -> Vec<u8> {
    convert(src, Source::Markdown, cfg).expect("Markdown converts in every built-in mode")
}
