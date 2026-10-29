//! Loads system fonts that are metric-compatible with the PDF base-14 fonts
//! (Helvetica ~ Arial ~ Liberation Sans, Times ~ Liberation Serif, Courier ~
//! Liberation Mono) so on-screen line breaks match the exported PDF. If none
//! are installed we fall back to egui's built-in fonts (widths then differ
//! slightly) and fake bold by double-striking.

use eframe::egui::{self, FontData, FontDefinitions, FontFamily};
use mdpdf_core::fonts::{Face, Family};
use std::sync::Arc;

pub struct FontSet {
    real: [bool; 12],
}

impl FontSet {
    pub fn family(&self, face: Face) -> FontFamily {
        FontFamily::Name(format!("md{}", face.index()).into())
    }
    pub fn faux_bold(&self, face: Face) -> bool {
        face.bold && !self.real[face.index()]
    }
}

fn candidates(face: Face) -> Vec<String> {
    let style = match (face.bold, face.italic) {
        (false, false) => 0,
        (false, true) => 1,
        (true, false) => 2,
        (true, true) => 3,
    };
    let (lib, lib_dirs, win, mac): (&str, &[&str], [&str; 4], [&str; 4]) = match face.family {
        Family::Sans => (
            "LiberationSans",
            &["liberation", "liberation-sans", "liberation2"],
            ["arial.ttf", "arialbd.ttf", "ariali.ttf", "arialbi.ttf"],
            ["Arial.ttf", "Arial Bold.ttf", "Arial Italic.ttf", "Arial Bold Italic.ttf"],
        ),
        Family::Serif => (
            "LiberationSerif",
            &["liberation", "liberation-serif", "liberation2"],
            ["times.ttf", "timesbd.ttf", "timesi.ttf", "timesbi.ttf"],
            ["Times New Roman.ttf", "Times New Roman Bold.ttf", "Times New Roman Italic.ttf", "Times New Roman Bold Italic.ttf"],
        ),
        Family::Mono => (
            "LiberationMono",
            &["liberation", "liberation-mono", "liberation2"],
            ["cour.ttf", "courbd.ttf", "couri.ttf", "courbi.ttf"],
            ["Courier New.ttf", "Courier New Bold.ttf", "Courier New Italic.ttf", "Courier New Bold Italic.ttf"],
        ),
    };
    let suffix = ["Regular", "Italic", "Bold", "BoldItalic"][match style {
        0 => 0,
        1 => 1,
        2 => 2,
        _ => 3,
    }];
    let mut v = Vec::new();
    for root in ["/usr/share/fonts/truetype", "/usr/share/fonts", "/usr/local/share/fonts"] {
        for d in lib_dirs {
            v.push(format!("{root}/{d}/{lib}-{suffix}.ttf"));
        }
    }
    v.push(format!("C:\\Windows\\Fonts\\{}", win[style]));
    v.push(format!("/System/Library/Fonts/Supplemental/{}", mac[style]));
    v.push(format!("/Library/Fonts/{}", mac[style]));
    v
}

pub fn install(ctx: &egui::Context) -> FontSet {
    let mut defs = FontDefinitions::default();
    let mut real = [false; 12];
    for idx in 0..12 {
        let face = Face::from_index(idx);
        let fam = FontFamily::Name(format!("md{idx}").into());
        let fallback = match face.family {
            Family::Mono => defs.families[&FontFamily::Monospace].clone(),
            _ => defs.families[&FontFamily::Proportional].clone(),
        };
        let mut list = Vec::new();
        if let Some(bytes) = candidates(face).iter().find_map(|p| std::fs::read(p).ok()) {
            let name = format!("md-font-{idx}");
            defs.font_data.insert(name.clone(), Arc::new(FontData::from_owned(bytes)));
            list.push(name);
            real[idx] = true;
        }
        list.extend(fallback);
        defs.families.insert(fam, list);
    }
    ctx.set_fonts(defs);
    FontSet { real }
}
