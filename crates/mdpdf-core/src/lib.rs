//! `mdpdf-core`: a dependency-free Markdown -> layout -> PDF pipeline.
//!
//! ```
//! let cfg = mdpdf_core::Config::a4();
//! let pdf = mdpdf_core::markdown_to_pdf("# Hello\n\nSome *text*.", &cfg);
//! assert!(pdf.starts_with(b"%PDF-"));
//! ```
//!
//! Pipeline: [`parse`] -> [`layout::layout`] -> [`pdf::write_pdf`]. The GUI
//! viewer stops after `layout` and paints the same ops on screen.

pub mod ast;
pub mod block;
pub mod fonts;
pub mod inline;
pub mod layout;
mod metrics_data;
pub mod pdf;

pub use ast::{Block, Document, Inline};
pub use block::parse;
pub use fonts::Family;
pub use layout::{layout, Color, Config, Layout, Op, Theme};

/// Converts Markdown source straight to PDF bytes.
pub fn markdown_to_pdf(src: &str, cfg: &Config) -> Vec<u8> {
    let doc = parse(src);
    pdf::write_pdf(&layout(&doc, cfg))
}
