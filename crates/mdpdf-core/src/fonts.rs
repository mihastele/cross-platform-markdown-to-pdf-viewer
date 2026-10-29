//! The 12 PDF base fonts we use (Helvetica/Times/Courier x regular/bold/italic),
//! their exact advance widths, and Unicode -> WinAnsi mapping.
//!
//! Using base fonts means PDFs need no embedded font programs and the layout
//! engine knows glyph widths exactly, so the viewer and the PDF agree.

use crate::metrics_data as m;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Family {
    Sans,
    Serif,
    Mono,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Face {
    pub family: Family,
    pub bold: bool,
    pub italic: bool,
}

impl Face {
    pub const fn new(family: Family, bold: bool, italic: bool) -> Self {
        Face { family, bold, italic }
    }

    /// Stable index 0..12, also used to name the PDF resource (`F0`..`F11`).
    pub fn index(self) -> usize {
        let f = match self.family {
            Family::Sans => 0,
            Family::Serif => 1,
            Family::Mono => 2,
        };
        f * 4 + (self.bold as usize) * 2 + self.italic as usize
    }

    /// Inverse of [`Face::index`].
    pub fn from_index(i: usize) -> Face {
        let family = match i / 4 {
            0 => Family::Sans,
            1 => Family::Serif,
            _ => Family::Mono,
        };
        Face { family, bold: i % 4 >= 2, italic: i % 2 == 1 }
    }

    pub fn pdf_name(self) -> &'static str {
        const NAMES: [&str; 12] = [
            "Helvetica", "Helvetica-Oblique", "Helvetica-Bold", "Helvetica-BoldOblique",
            "Times-Roman", "Times-Italic", "Times-Bold", "Times-BoldItalic",
            "Courier", "Courier-Oblique", "Courier-Bold", "Courier-BoldOblique",
        ];
        NAMES[self.index()]
    }

    fn table(self) -> &'static [u16; 256] {
        match self.index() {
            0 => &m::HELVETICA,
            1 => &m::HELVETICA_OBLIQUE,
            2 => &m::HELVETICA_BOLD,
            3 => &m::HELVETICA_BOLDOBLIQUE,
            4 => &m::TIMES_ROMAN,
            5 => &m::TIMES_ITALIC,
            6 => &m::TIMES_BOLD,
            7 => &m::TIMES_BOLDITALIC,
            8 => &m::COURIER,
            9 => &m::COURIER_OBLIQUE,
            10 => &m::COURIER_BOLD,
            _ => &m::COURIER_BOLDOBLIQUE,
        }
    }

    pub fn with(self, bold: bool, italic: bool) -> Face {
        Face { bold: self.bold || bold, italic: self.italic || italic, ..self }
    }

    /// Width of `text` in points at `size`.
    pub fn width(self, text: &str, size: f32) -> f32 {
        let t = self.table();
        let units: u32 = text.chars().map(|c| t[winansi(c).unwrap_or(b'?') as usize] as u32).sum();
        units as f32 * size / 1000.0
    }

    pub fn char_width(self, c: char, size: f32) -> f32 {
        self.table()[winansi(c).unwrap_or(b'?') as usize] as f32 * size / 1000.0
    }
}

/// Maps a Unicode scalar to its WinAnsi (CP1252) byte.
pub fn winansi(c: char) -> Option<u8> {
    let u = c as u32;
    match u {
        0x20..=0x7e | 0xa0..=0xff => Some(u as u8),
        0x20ac => Some(0x80),
        0x201a => Some(0x82),
        0x0192 => Some(0x83),
        0x201e => Some(0x84),
        0x2026 => Some(0x85),
        0x2020 => Some(0x86),
        0x2021 => Some(0x87),
        0x02c6 => Some(0x88),
        0x2030 => Some(0x89),
        0x0160 => Some(0x8a),
        0x2039 => Some(0x8b),
        0x0152 => Some(0x8c),
        0x017d => Some(0x8e),
        0x2018 => Some(0x91),
        0x2019 => Some(0x92),
        0x201c => Some(0x93),
        0x201d => Some(0x94),
        0x2022 => Some(0x95),
        0x2013 => Some(0x96),
        0x2014 => Some(0x97),
        0x02dc => Some(0x98),
        0x2122 => Some(0x99),
        0x0161 => Some(0x9a),
        0x203a => Some(0x9b),
        0x0153 => Some(0x9c),
        0x017e => Some(0x9e),
        0x0178 => Some(0x9f),
        _ => None,
    }
}

/// Rewrites text so every char is representable in WinAnsi: common symbols get
/// ASCII look-alikes, anything else becomes `?`.
pub fn sanitize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if winansi(c).is_some() || c == '\n' {
            out.push(c);
            continue;
        }
        match c {
            '\u{2192}' | '\u{27f6}' => out.push_str("->"),
            '\u{2190}' | '\u{27f5}' => out.push_str("<-"),
            '\u{2194}' => out.push_str("<->"),
            '\u{21d2}' => out.push_str("=>"),
            '\u{21d0}' => out.push_str("<="),
            '\u{2264}' => out.push_str("<="),
            '\u{2265}' => out.push_str(">="),
            '\u{2260}' => out.push_str("!="),
            '\u{2212}' => out.push('-'),
            '\u{2009}' | '\u{2002}' | '\u{2003}' | '\u{202f}' => out.push(' '),
            '\u{200b}' | '\u{200c}' | '\u{200d}' | '\u{feff}' | '\u{fe0f}' => {}
            '\u{2713}' | '\u{2714}' => out.push('v'),
            '\u{2717}' | '\u{2718}' => out.push('x'),
            '\u{2023}' | '\u{25cf}' | '\u{25aa}' => out.push('\u{2022}'),
            '\u{2010}' | '\u{2011}' => out.push('-'),
            _ if c.is_control() => {}
            _ => out.push('?'),
        }
    }
    out
}
