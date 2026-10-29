//! A deliberately small CSS engine: the properties that make sense for paged
//! documents (colour, background, alignment, font size/weight/style, margins,
//! padding, borders, page breaks, `display:none`, image sizes), simple
//! selectors (`tag`, `.class`, `#id`, `tag.class`, descendant chains, groups),
//! cascade by specificity, and `var(--custom)` properties.
//! Not supported: flex/grid/float/positioning, pseudo-classes/elements,
//! attribute selectors, `calc()`, web fonts, media queries.

use crate::ast::*;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FontSize {
    /// Relative to the 16px default (so `16px` is 1.0).
    Abs(f32),
    /// Relative to the parent element (`em`, `%`).
    Rel(f32),
}

#[derive(Debug, Clone, Default)]
pub struct Css {
    pub color: Option<Color>,
    pub bg: Option<Color>,
    pub align: Option<Align>,
    pub font_size: Option<FontSize>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<bool>,
    pub strike: Option<bool>,
    pub mono: Option<bool>,
    pub margin_top: Option<f32>,
    pub margin_bottom: Option<f32>,
    pub pad_x: Option<f32>,
    pub pad_y: Option<f32>,
    pub indent: Option<f32>,
    pub border: Option<(f32, Color)>,
    pub border_left: Option<(f32, Color)>,
    pub break_before: bool,
    pub break_after: bool,
    pub avoid_break: bool,
    pub hidden: bool,
    pub width: Option<Dim>,
    pub height: Option<Dim>,
    pub pre: bool,
}

const NAMED: &[(&str, (u8, u8, u8))] = &[
    ("black", (0, 0, 0)), ("white", (255, 255, 255)), ("red", (255, 0, 0)), ("green", (0, 128, 0)),
    ("blue", (0, 0, 255)), ("gray", (128, 128, 128)), ("grey", (128, 128, 128)), ("silver", (192, 192, 192)),
    ("yellow", (255, 255, 0)), ("orange", (255, 165, 0)), ("purple", (128, 0, 128)), ("pink", (255, 192, 203)),
    ("brown", (165, 42, 42)), ("navy", (0, 0, 128)), ("teal", (0, 128, 128)), ("maroon", (128, 0, 0)),
    ("olive", (128, 128, 0)), ("lime", (0, 255, 0)), ("aqua", (0, 255, 255)), ("cyan", (0, 255, 255)),
    ("magenta", (255, 0, 255)), ("fuchsia", (255, 0, 255)), ("gold", (255, 215, 0)), ("indigo", (75, 0, 130)),
    ("violet", (238, 130, 238)), ("crimson", (220, 20, 60)), ("coral", (255, 127, 80)), ("salmon", (250, 128, 114)),
    ("tomato", (255, 99, 71)), ("steelblue", (70, 130, 180)), ("slategray", (112, 128, 144)), ("slategrey", (112, 128, 144)),
    ("darkgray", (169, 169, 169)), ("darkgrey", (169, 169, 169)), ("lightgray", (211, 211, 211)), ("lightgrey", (211, 211, 211)),
    ("lightblue", (173, 216, 230)), ("lightgreen", (144, 238, 144)), ("whitesmoke", (245, 245, 245)),
    ("darkblue", (0, 0, 139)), ("darkgreen", (0, 100, 0)), ("darkred", (139, 0, 0)), ("royalblue", (65, 105, 225)),
    ("dodgerblue", (30, 144, 255)), ("skyblue", (135, 206, 235)), ("beige", (245, 245, 220)), ("ivory", (255, 255, 240)),
    ("lavender", (230, 230, 250)), ("khaki", (240, 230, 140)), ("tan", (210, 180, 140)), ("orchid", (218, 112, 214)),
    ("turquoise", (64, 224, 208)), ("firebrick", (178, 34, 34)), ("seagreen", (46, 139, 87)), ("midnightblue", (25, 25, 112)),
];

pub fn parse_color(v: &str) -> Option<Color> {
    let v = v.trim().to_ascii_lowercase();
    if let Some(h) = v.strip_prefix('#') {
        let d = |i: usize| u8::from_str_radix(h.get(i..i + 1)?, 16).ok().map(|x| x * 17);
        let p = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
        return match h.len() {
            3 | 4 => Some(Color::rgb(d(0)?, d(1)?, d(2)?)),
            6 | 8 => Some(Color::rgb(p(0)?, p(2)?, p(4)?)),
            _ => None,
        };
    }
    if let Some(args) = v.strip_prefix("rgb").and_then(|r| r.trim_start_matches('a').strip_prefix('(')) {
        let args = args.trim_end_matches(')');
        let parts: Vec<f32> = args
            .split(|c: char| c == ',' || c == '/' || c.is_whitespace())
            .filter(|s| !s.is_empty())
            .map(|s| {
                if let Some(p) = s.strip_suffix('%') {
                    p.parse::<f32>().unwrap_or(0.0) * 2.55
                } else {
                    s.parse::<f32>().unwrap_or(0.0)
                }
            })
            .collect();
        if parts.len() >= 3 {
            let c = |x: f32| x.round().clamp(0.0, 255.0) as u8;
            return Some(Color::rgb(c(parts[0]), c(parts[1]), c(parts[2])));
        }
        return None;
    }
    NAMED.iter().find(|(n, _)| *n == v).map(|&(_, (r, g, b))| Color::rgb(r, g, b))
}

/// Parses a length into points (`px` = 0.75pt, `em`/`rem` = 12pt).
pub fn parse_pt(v: &str) -> Option<f32> {
    let v = v.trim();
    let split = v.find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+')).unwrap_or(v.len());
    let n: f32 = v[..split].parse().ok()?;
    Some(match &v[split..] {
        "" => n * if n == 0.0 { 1.0 } else { 0.75 },
        "px" => n * 0.75,
        "pt" => n,
        "em" | "rem" => n * 12.0,
        "cm" => n * 28.35,
        "mm" => n * 2.835,
        "in" => n * 72.0,
        "pc" => n * 12.0,
        _ => return None,
    })
}

fn parse_dim(v: &str) -> Option<Dim> {
    let v = v.trim();
    if let Some(p) = v.strip_suffix('%') {
        return p.trim().parse().ok().map(Dim::Pct);
    }
    parse_pt(v).map(Dim::Pt)
}

fn parse_font_size(v: &str) -> Option<FontSize> {
    let v = v.trim();
    Some(match v {
        "xx-small" => FontSize::Abs(0.6),
        "x-small" => FontSize::Abs(0.75),
        "small" => FontSize::Abs(0.875),
        "medium" => FontSize::Abs(1.0),
        "large" => FontSize::Abs(1.125),
        "x-large" => FontSize::Abs(1.5),
        "xx-large" => FontSize::Abs(2.0),
        "smaller" => FontSize::Rel(0.83),
        "larger" => FontSize::Rel(1.2),
        _ => {
            if let Some(p) = v.strip_suffix('%') {
                FontSize::Rel(p.trim().parse::<f32>().ok()? / 100.0)
            } else if let Some(e) = v.strip_suffix("rem") {
                FontSize::Abs(e.trim().parse().ok()?)
            } else if let Some(e) = v.strip_suffix("em") {
                FontSize::Rel(e.trim().parse().ok()?)
            } else {
                FontSize::Abs(parse_pt(v)? / 12.0)
            }
        }
    })
}

/// Splits a CSS value on whitespace, keeping parenthesised groups together.
fn tokens(v: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut depth = 0;
    for c in v.chars() {
        match c {
            '(' => {
                depth += 1;
                cur.push(c);
            }
            ')' => {
                depth -= 1;
                cur.push(c);
            }
            c if c.is_whitespace() && depth == 0 => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn border_of(v: &str) -> Option<(f32, Color)> {
    let toks = tokens(v);
    if toks.iter().any(|t| t == "none" || t == "hidden") {
        return None;
    }
    let mut w = 0.75;
    let mut c = Color::rgb(160, 160, 160);
    for t in &toks {
        if let Some(col) = parse_color(t) {
            c = col;
        } else if let Some(p) = parse_pt(t) {
            w = p.max(0.25);
        }
    }
    Some((w, c))
}

fn box_values(v: &str) -> Option<[f32; 4]> {
    let vals: Vec<f32> = tokens(v).iter().map(|t| if t == "auto" { Some(0.0) } else { parse_pt(t) }).collect::<Option<_>>()?;
    Some(match vals.as_slice() {
        [a] => [*a; 4],
        [v, h] => [*v, *h, *v, *h],
        [t, h, b] => [*t, *h, *b, *h],
        [t, r, b, l, ..] => [*t, *r, *b, *l],
        _ => return None,
    })
}

impl Css {
    pub fn apply(&mut self, name: &str, value: &str) {
        let value = value.trim().trim_end_matches("!important").trim();
        match name {
            "color" => self.color = parse_color(value).or(self.color),
            "background" | "background-color" => {
                if !value.contains("gradient") && !value.contains("url(") {
                    if let Some(c) = tokens(value).iter().find_map(|t| parse_color(t)) {
                        self.bg = Some(c);
                    }
                }
            }
            "text-align" => {
                self.align = match value {
                    "center" => Some(Align::Center),
                    "right" | "end" => Some(Align::Right),
                    "left" | "start" | "justify" => Some(Align::Left),
                    _ => self.align,
                }
            }
            "font-size" => self.font_size = parse_font_size(value).or(self.font_size),
            "font-weight" => {
                self.bold = match value {
                    "bold" | "bolder" => Some(true),
                    "normal" | "lighter" => Some(false),
                    n => n.parse::<u32>().ok().map(|w| w >= 600).or(self.bold),
                }
            }
            "font-style" => self.italic = Some(value.contains("italic") || value.contains("oblique")),
            "text-decoration" | "text-decoration-line" => {
                if value.contains("underline") {
                    self.underline = Some(true);
                }
                if value.contains("line-through") {
                    self.strike = Some(true);
                }
                if value == "none" {
                    self.underline = Some(false);
                    self.strike = Some(false);
                }
            }
            "font-family" => {
                let v = value.to_ascii_lowercase();
                let first = v.split(',').next().unwrap_or("");
                if first.contains("mono") || first.contains("courier") || first.contains("consolas") || first.contains("menlo") {
                    self.mono = Some(true);
                }
            }
            "margin" => {
                if let Some([t, _, b, l]) = box_values(value) {
                    self.margin_top = Some(t);
                    self.margin_bottom = Some(b);
                    self.indent = Some(self.indent.unwrap_or(0.0).max(l));
                }
            }
            "margin-top" => self.margin_top = parse_pt(value).or(self.margin_top),
            "margin-bottom" => self.margin_bottom = parse_pt(value).or(self.margin_bottom),
            "margin-left" => self.indent = parse_pt(value).or(self.indent),
            "padding" => {
                if let Some([t, r, _, _]) = box_values(value) {
                    self.pad_y = Some(t);
                    self.pad_x = Some(r);
                }
            }
            "padding-top" | "padding-bottom" => self.pad_y = parse_pt(value).or(self.pad_y),
            "padding-left" | "padding-right" => self.pad_x = parse_pt(value).or(self.pad_x),
            "border" => self.border = border_of(value),
            "border-left" => self.border_left = border_of(value),
            "page-break-before" | "break-before" => self.break_before = matches!(value, "always" | "page" | "left" | "right"),
            "page-break-after" | "break-after" => self.break_after = matches!(value, "always" | "page" | "left" | "right"),
            "page-break-inside" | "break-inside" => self.avoid_break = value == "avoid" || value == "avoid-page",
            "display" => self.hidden = value == "none",
            "width" | "max-width" => {
                if name == "width" || self.width.is_none() {
                    self.width = parse_dim(value).or(self.width)
                }
            }
            "height" => self.height = parse_dim(value).or(self.height),
            "white-space" => self.pre = value.starts_with("pre"),
            _ => {}
        }
    }

    pub fn apply_decls(&mut self, decls: &[(String, String)], vars: &HashMap<String, String>) {
        for (n, v) in decls {
            let v = substitute_vars(v, vars);
            self.apply(n, &v);
        }
    }
}

fn substitute_vars(v: &str, vars: &HashMap<String, String>) -> String {
    let mut out = v.to_string();
    for _ in 0..4 {
        let Some(s) = out.find("var(") else { break };
        let rest = &out[s + 4..];
        let mut depth = 1;
        let mut end = None;
        for (i, c) in rest.char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(i);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(e) = end else { break };
        let inner = &rest[..e];
        let (name, fallback) = inner.split_once(',').map_or((inner, ""), |(a, b)| (a, b));
        let val = vars.get(name.trim()).cloned().unwrap_or_else(|| fallback.trim().to_string());
        out = format!("{}{}{}", &out[..s], val, &rest[e + 1..]);
    }
    out
}

pub fn parse_decls(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut depth = 0;
    let mut cur = String::new();
    let flush = |cur: &mut String, out: &mut Vec<(String, String)>| {
        if let Some((n, v)) = cur.split_once(':') {
            out.push((n.trim().to_ascii_lowercase(), v.trim().to_string()));
        }
        cur.clear();
    };
    for c in text.chars() {
        match c {
            '(' => {
                depth += 1;
                cur.push(c);
            }
            ')' => {
                depth -= 1;
                cur.push(c);
            }
            ';' if depth == 0 => flush(&mut cur, &mut out),
            _ => cur.push(c),
        }
    }
    flush(&mut cur, &mut out);
    out
}

// ---------------------------------------------------------------- sheet

#[derive(Debug, Clone, Default)]
pub struct ElemRef {
    pub tag: String,
    pub id: String,
    pub classes: Vec<String>,
}

#[derive(Debug, Clone)]
struct Simple {
    tag: Option<String>,
    id: Option<String>,
    classes: Vec<String>,
}

impl Simple {
    fn matches(&self, e: &ElemRef) -> bool {
        self.tag.as_ref().is_none_or(|t| *t == e.tag)
            && self.id.as_ref().is_none_or(|i| *i == e.id)
            && self.classes.iter().all(|c| e.classes.contains(c))
    }
}

#[derive(Debug, Clone)]
struct Rule {
    chain: Vec<Simple>,
    spec: (u32, u32, u32),
    decls: Vec<(String, String)>,
}

#[derive(Debug, Clone, Default)]
pub struct Sheet {
    rules: Vec<Rule>,
    vars: HashMap<String, String>,
}

fn parse_simple(s: &str) -> Option<Simple> {
    let mut tag = None;
    let mut id = None;
    let mut classes = Vec::new();
    let mut cur = String::new();
    let mut mode = 't';
    let commit = |mode: char, cur: &mut String, tag: &mut Option<String>, id: &mut Option<String>, classes: &mut Vec<String>| {
        if !cur.is_empty() {
            match mode {
                't' if cur != "*" => *tag = Some(cur.to_ascii_lowercase()),
                '#' => *id = Some(cur.clone()),
                '.' => classes.push(cur.clone()),
                _ => {}
            }
        }
        cur.clear();
    };
    for c in s.chars() {
        match c {
            '#' | '.' => {
                commit(mode, &mut cur, &mut tag, &mut id, &mut classes);
                mode = c;
            }
            c if c.is_alphanumeric() || c == '-' || c == '_' || c == '*' => cur.push(c),
            _ => return None,
        }
    }
    commit(mode, &mut cur, &mut tag, &mut id, &mut classes);
    Some(Simple { tag, id, classes })
}

impl Sheet {
    pub fn parse(src: &str) -> Sheet {
        let mut sheet = Sheet::default();
        sheet.add(src);
        sheet
    }

    pub fn add(&mut self, src: &str) {
        let mut text = String::new();
        let mut rest = src;
        while let Some(s) = rest.find("/*") {
            text.push_str(&rest[..s]);
            rest = rest[s..].find("*/").map_or("", |e| &rest[s + e + 2..]);
        }
        text.push_str(rest);
        let b: Vec<char> = text.chars().collect();
        let mut i = 0;
        while i < b.len() {
            let start = i;
            while i < b.len() && b[i] != '{' && b[i] != ';' {
                i += 1;
            }
            if i >= b.len() {
                break;
            }
            let head: String = b[start..i].iter().collect();
            if b[i] == ';' {
                i += 1;
                continue;
            }
            let mut depth = 1;
            let body_start = i + 1;
            i += 1;
            while i < b.len() && depth > 0 {
                match b[i] {
                    '{' => depth += 1,
                    '}' => depth -= 1,
                    _ => {}
                }
                i += 1;
            }
            let body: String = b[body_start..i.saturating_sub(1).max(body_start)].iter().collect();
            let head = head.trim();
            if head.starts_with('@') {
                continue;
            }
            self.add_rule(head, &body);
        }
    }

    fn add_rule(&mut self, selectors: &str, body: &str) {
        let decls = parse_decls(body);
        for sel in selectors.split(',') {
            let sel = sel.trim();
            if matches!(sel, ":root" | "html" | "body" | "*") {
                for (n, v) in &decls {
                    if n.starts_with("--") {
                        self.vars.insert(n.clone(), v.clone());
                    }
                }
            }
            let sel = if sel == ":root" { "html" } else { sel };
            let parts: Vec<&str> = sel.split(|c: char| c.is_whitespace() || c == '>').filter(|s| !s.is_empty()).collect();
            let chain: Option<Vec<Simple>> = parts.iter().map(|p| parse_simple(p)).collect();
            let Some(chain) = chain else { continue };
            if chain.is_empty() {
                continue;
            }
            let spec = chain.iter().fold((0, 0, 0), |a, s| {
                (a.0 + s.id.is_some() as u32, a.1 + s.classes.len() as u32, a.2 + s.tag.is_some() as u32)
            });
            let decls: Vec<_> = decls.iter().filter(|(n, _)| !n.starts_with("--")).cloned().collect();
            self.rules.push(Rule { chain, spec, decls });
        }
    }

    /// Computes the style of `el` (with `ancestors` outermost-first) from
    /// matching rules plus its inline `style` attribute.
    pub fn compute(&self, el: &ElemRef, ancestors: &[ElemRef], inline: Option<&str>) -> Css {
        let mut matched: Vec<(&Rule, usize)> = Vec::new();
        for (order, r) in self.rules.iter().enumerate() {
            let last = r.chain.last().unwrap();
            if !last.matches(el) {
                continue;
            }
            // Remaining chain must match some subsequence of ancestors, right to left.
            let mut ai = ancestors.len();
            let ok = r.chain[..r.chain.len() - 1].iter().rev().all(|s| {
                while ai > 0 {
                    ai -= 1;
                    if s.matches(&ancestors[ai]) {
                        return true;
                    }
                }
                false
            });
            if ok {
                matched.push((r, order));
            }
        }
        matched.sort_by_key(|(r, o)| (r.spec, *o));
        let mut css = Css::default();
        for (r, _) in matched {
            css.apply_decls(&r.decls, &self.vars);
        }
        if let Some(s) = inline {
            css.apply_decls(&parse_decls(s), &self.vars);
        }
        css
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_and_lengths() {
        assert_eq!(parse_color("#f80"), Some(Color::rgb(255, 136, 0)));
        assert_eq!(parse_color("rgb(10, 20, 30)"), Some(Color::rgb(10, 20, 30)));
        assert_eq!(parse_color("Navy"), Some(Color::rgb(0, 0, 128)));
        assert_eq!(parse_pt("16px"), Some(12.0));
        assert_eq!(parse_pt("1.5em"), Some(18.0));
    }

    #[test]
    fn cascade_specificity_and_vars() {
        let s = Sheet::parse(":root{--c:#112233} p{color:red} .x{color:var(--c)} p.x{margin-top:4px}");
        let el = ElemRef { tag: "p".into(), id: String::new(), classes: vec!["x".into()] };
        let css = s.compute(&el, &[], Some("text-align:center"));
        assert_eq!(css.color, Some(Color::rgb(0x11, 0x22, 0x33)));
        assert_eq!(css.margin_top, Some(3.0));
        assert_eq!(css.align, Some(Align::Center));
    }

    #[test]
    fn descendant_selectors() {
        let s = Sheet::parse(".card p { color: blue } p { color: red }");
        let card = ElemRef { tag: "div".into(), id: String::new(), classes: vec!["card".into()] };
        let p = ElemRef { tag: "p".into(), ..Default::default() };
        assert_eq!(s.compute(&p, &[card], None).color, Some(Color::rgb(0, 0, 255)));
        assert_eq!(s.compute(&p, &[], None).color, Some(Color::rgb(255, 0, 0)));
    }
}
