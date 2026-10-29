//! Minimal PDF 1.4 writer: base-14 fonts, vector drawing, link annotations,
//! document info and a nested bookmark outline. No compression, no deps.

use crate::deflate::zlib_compress;
use crate::fonts::winansi;
use crate::image::{ImageData, Pixels};
use crate::layout::{Color, Layout, Op};
use std::collections::BTreeSet;
use std::fmt::Write as _;

fn num(v: f32) -> String {
    let s = format!("{:.2}", v);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-" || s == "-0" { "0".into() } else { s.into() }
}

fn rg(c: Color, stroke: bool) -> String {
    let (r, g, b) = c.unit();
    format!("{} {} {} {}", num(r), num(g), num(b), if stroke { "RG" } else { "rg" })
}

/// Literal string for text drawn in a WinAnsi-encoded base font.
fn pdf_text(s: &str) -> Vec<u8> {
    let mut out = vec![b'('];
    for c in s.chars() {
        let b = winansi(c).unwrap_or(b'?');
        match b {
            b'(' | b')' | b'\\' => {
                out.push(b'\\');
                out.push(b);
            }
            32..=126 => out.push(b),
            _ => out.extend_from_slice(format!("\\{:03o}", b).as_bytes()),
        }
    }
    out.push(b')');
    out
}

/// Text string (titles, bookmarks): UTF-16BE with BOM as a hex string.
fn text_string(s: &str) -> String {
    let mut out = String::from("<FEFF");
    for u in s.encode_utf16() {
        let _ = write!(out, "{:04X}", u);
    }
    out.push('>');
    out
}

fn ascii_url(url: &str) -> Vec<u8> {
    let mut out = vec![b'('];
    for &b in url.as_bytes() {
        match b {
            b'(' | b')' | b'\\' => {
                out.push(b'\\');
                out.push(b);
            }
            33..=126 => out.push(b),
            _ => out.extend_from_slice(format!("%{:02X}", b).as_bytes()),
        }
    }
    out.push(b')');
    out
}

struct Objects {
    objs: Vec<Vec<u8>>,
}

impl Objects {
    fn alloc(&mut self) -> usize {
        self.objs.push(Vec::new());
        self.objs.len()
    }
    fn set(&mut self, id: usize, body: Vec<u8>) {
        self.objs[id - 1] = body;
    }
}

struct OutlineNode {
    level: u8,
    title: String,
    page: usize,
    y: f32,
    parent: Option<usize>,
    children: Vec<usize>,
}

fn content_stream(page_h: f32, bg: Option<Color>, w: f32, ops: &[Op]) -> Vec<u8> {
    let mut s: Vec<u8> = Vec::new();
    let fy = |y: f32| page_h - y;
    if let Some(bg) = bg {
        let _ = writeln!(Str(&mut s), "{} 0 0 {} {} re f", rg(bg, false), num(w), num(page_h));
    }
    for op in ops {
        match op {
            Op::Text { x, y, face, size, color, text } => {
                let _ = write!(
                    Str(&mut s),
                    "BT /F{} {} Tf {} {} {} Td ",
                    face.index(),
                    num(*size),
                    rg(*color, false),
                    num(*x),
                    num(fy(*y))
                );
                s.extend(pdf_text(text));
                s.extend_from_slice(b" Tj ET\n");
            }
            Op::Rect { x, y, w, h, fill, stroke } => {
                let mut cmd = String::new();
                if let Some(f) = fill {
                    cmd += &format!("{} ", rg(*f, false));
                }
                if let Some((c, lw)) = stroke {
                    cmd += &format!("{} {} w ", rg(*c, true), num(*lw));
                }
                let paint = match (fill.is_some(), stroke.is_some()) {
                    (true, true) => "B",
                    (true, false) => "f",
                    (false, true) => "S",
                    _ => continue,
                };
                let _ = writeln!(Str(&mut s), "{}{} {} {} {} re {}", cmd, num(*x), num(fy(*y + *h)), num(*w), num(*h), paint);
            }
            Op::Line { x1, y1, x2, y2, color, width } => {
                let _ = writeln!(
                    Str(&mut s),
                    "{} {} w {} {} m {} {} l S",
                    rg(*color, true),
                    num(*width),
                    num(*x1),
                    num(fy(*y1)),
                    num(*x2),
                    num(fy(*y2))
                );
            }
            Op::Circle { cx, cy, r, fill, color, width } => {
                let k = 0.5523 * r;
                let (cx, cy) = (*cx, fy(*cy));
                let p = |a: f32, b: f32| format!("{} {}", num(a), num(b));
                let path = format!(
                    "{} m {} {} {} c {} {} {} c {} {} {} c {} {} {} c h",
                    p(cx + r, cy),
                    p(cx + r, cy + k), p(cx + k, cy + r), p(cx, cy + r),
                    p(cx - k, cy + r), p(cx - r, cy + k), p(cx - r, cy),
                    p(cx - r, cy - k), p(cx - k, cy - r), p(cx, cy - r),
                    p(cx + k, cy - r), p(cx + r, cy - k), p(cx + r, cy),
                );
                if *fill {
                    let _ = writeln!(Str(&mut s), "{} {} f", rg(*color, false), path);
                } else {
                    let _ = writeln!(Str(&mut s), "{} {} w {} S", rg(*color, true), num(*width), path);
                }
            }
            Op::Image { x, y, w, h, img } => {
                let _ = writeln!(Str(&mut s), "q {} 0 0 {} {} {} cm /Im{} Do Q", num(*w), num(*h), num(*x), num(fy(*y + *h)), img);
            }
            Op::Link { .. } => {}
        }
    }
    s
}

/// A stream object: `dict` holds extra dictionary entries.
fn stream_obj(dict: &str, data: &[u8], compress: bool) -> Vec<u8> {
    let packed;
    let (filter, bytes) = if compress {
        packed = zlib_compress(data);
        (" /Filter /FlateDecode", &packed[..])
    } else {
        ("", data)
    };
    let mut body = format!("<< {}{} /Length {} >>\nstream\n", dict, filter, bytes.len()).into_bytes();
    body.extend_from_slice(bytes);
    body.extend_from_slice(b"\nendstream");
    body
}

fn write_image(o: &mut Objects, im: &ImageData) -> usize {
    let id = o.alloc();
    let base = format!("/Type /XObject /Subtype /Image /Width {} /Height {} /BitsPerComponent 8", im.width, im.height);
    match &im.pixels {
        Pixels::Raw { rgb, alpha } => {
            let mut dict = format!("{base} /ColorSpace /DeviceRGB");
            if let Some(a) = alpha {
                let mid = o.alloc();
                let d = format!("/Type /XObject /Subtype /Image /Width {} /Height {} /BitsPerComponent 8 /ColorSpace /DeviceGray", im.width, im.height);
                o.set(mid, stream_obj(&d, a, true));
                let _ = write!(dict, " /SMask {} 0 R", mid);
            }
            o.set(id, stream_obj(&dict, rgb, true));
        }
        Pixels::Jpeg { data, components, inverted_cmyk } => {
            let cs = match components {
                1 => "/DeviceGray",
                4 => "/DeviceCMYK",
                _ => "/DeviceRGB",
            };
            let decode = if *inverted_cmyk { " /Decode [1 0 1 0 1 0 1 0]" } else { "" };
            let dict = format!("{base} /ColorSpace {cs}{decode} /Filter /DCTDecode");
            o.set(id, stream_obj(&dict, data, false));
        }
    }
    id
}

/// Adapter so `write!` can target a byte vector.
struct Str<'a>(&'a mut Vec<u8>);
impl std::fmt::Write for Str<'_> {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        self.0.extend_from_slice(s.as_bytes());
        Ok(())
    }
}

pub fn write_pdf(layout: &Layout) -> Vec<u8> {
    let mut o = Objects { objs: Vec::new() };
    let catalog = o.alloc();
    let pages_id = o.alloc();
    let info = o.alloc();

    let used: BTreeSet<usize> = layout
        .pages
        .iter()
        .flat_map(|p| p.ops.iter())
        .filter_map(|op| match op {
            Op::Text { face, .. } => Some(face.index()),
            _ => None,
        })
        .collect();
    let mut font_ids = Vec::new();
    for &idx in &used {
        let id = o.alloc();
        let f = crate::fonts::Face::from_index(idx);
        o.set(
            id,
            format!(
                "<< /Type /Font /Subtype /Type1 /BaseFont /{} /Encoding /WinAnsiEncoding >>",
                f.pdf_name()
            )
            .into_bytes(),
        );
        font_ids.push((idx, id));
    }
    let font_res: String = font_ids.iter().map(|(i, id)| format!("/F{} {} 0 R ", i, id)).collect();

    let image_ids: Vec<usize> = layout.images.iter().map(|im| write_image(&mut o, im)).collect();

    let white = Color::rgb(255, 255, 255);
    let bg = (layout.background != white).then_some(layout.background);

    let mut page_ids = Vec::new();
    for page in &layout.pages {
        let pid = o.alloc();
        let cid = o.alloc();
        let stream = content_stream(layout.height, bg, layout.width, &page.ops);
        o.set(cid, stream_obj("", &stream, true));
        let xobjs: String = page
            .ops
            .iter()
            .filter_map(|op| match op {
                Op::Image { img, .. } => Some(*img),
                _ => None,
            })
            .collect::<BTreeSet<_>>()
            .into_iter()
            .map(|i| format!("/Im{} {} 0 R ", i, image_ids[i]))
            .collect();

        let mut annots = String::new();
        for op in &page.ops {
            if let Op::Link { x, y, w, h, url } = op {
                let aid = o.alloc();
                let mut body = format!(
                    "<< /Type /Annot /Subtype /Link /Border [0 0 0] /Rect [{} {} {} {}] /A << /S /URI /URI ",
                    num(*x),
                    num(layout.height - y - h),
                    num(x + w),
                    num(layout.height - y)
                )
                .into_bytes();
                body.extend(ascii_url(url));
                body.extend_from_slice(b" >> >>");
                o.set(aid, body);
                let _ = write!(annots, "{} 0 R ", aid);
            }
        }
        o.set(
            pid,
            format!(
                "<< /Type /Page /Parent {} 0 R /MediaBox [0 0 {} {}] /Resources << /Font << {}>> /XObject << {}>> >> /Contents {} 0 R /Annots [{}] >>",
                pages_id, num(layout.width), num(layout.height), font_res, xobjs, cid, annots
            )
            .into_bytes(),
        );
        page_ids.push(pid);
    }

    let kids: String = page_ids.iter().map(|id| format!("{} 0 R ", id)).collect();
    o.set(
        pages_id,
        format!("<< /Type /Pages /Kids [{}] /Count {} >>", kids, page_ids.len()).into_bytes(),
    );

    // Bookmarks.
    let mut nodes: Vec<OutlineNode> = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    for h in &layout.headings {
        while stack.last().is_some_and(|&s| nodes[s].level >= h.level) {
            stack.pop();
        }
        let idx = nodes.len();
        let parent = stack.last().copied();
        if let Some(p) = parent {
            nodes[p].children.push(idx);
        }
        nodes.push(OutlineNode { level: h.level, title: h.title.clone(), page: h.page, y: h.y, parent, children: vec![] });
        stack.push(idx);
    }
    let mut outlines_ref = String::new();
    if !nodes.is_empty() {
        let root = o.alloc();
        let ids: Vec<usize> = nodes.iter().map(|_| o.alloc()).collect();
        let top: Vec<usize> = (0..nodes.len()).filter(|&i| nodes[i].parent.is_none()).collect();
        fn descendants(nodes: &[OutlineNode], i: usize) -> usize {
            nodes[i].children.iter().map(|&c| 1 + descendants(nodes, c)).sum()
        }
        for (i, n) in nodes.iter().enumerate() {
            let siblings: &[usize] = match n.parent {
                Some(p) => &nodes[p].children,
                None => &top,
            };
            let pos = siblings.iter().position(|&s| s == i).unwrap();
            let mut b = format!(
                "<< /Title {} /Parent {} 0 R /Dest [{} 0 R /XYZ 0 {} 0]",
                text_string(&n.title),
                n.parent.map_or(root, |p| ids[p]),
                page_ids[n.page],
                num(layout.height - n.y + 6.0)
            );
            if pos > 0 {
                let _ = write!(b, " /Prev {} 0 R", ids[siblings[pos - 1]]);
            }
            if pos + 1 < siblings.len() {
                let _ = write!(b, " /Next {} 0 R", ids[siblings[pos + 1]]);
            }
            if let (Some(&f), Some(&l)) = (n.children.first(), n.children.last()) {
                let _ = write!(b, " /First {} 0 R /Last {} 0 R /Count {}", ids[f], ids[l], descendants(&nodes, i));
            }
            b.push_str(" >>");
            o.set(ids[i], b.into_bytes());
        }
        o.set(
            root,
            format!(
                "<< /Type /Outlines /First {} 0 R /Last {} 0 R /Count {} >>",
                ids[top[0]],
                ids[*top.last().unwrap()],
                nodes.len()
            )
            .into_bytes(),
        );
        outlines_ref = format!(" /Outlines {} 0 R /PageMode /UseOutlines", root);
    }

    o.set(
        catalog,
        format!("<< /Type /Catalog /Pages {} 0 R{} >>", pages_id, outlines_ref).into_bytes(),
    );
    let mut inf = String::from("<< /Producer (mdpdf) ");
    if let Some(t) = &layout.title {
        let _ = write!(inf, "/Title {} ", text_string(t));
    }
    inf.push_str(">>");
    o.set(info, inf.into_bytes());

    let mut out: Vec<u8> = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in o.objs.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", o.objs.len() + 1).as_bytes());
    for off in offsets {
        out.extend_from_slice(format!("{:010} 00000 n \n", off).as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root {} 0 R /Info {} 0 R >>\nstartxref\n{}\n%%EOF\n",
            o.objs.len() + 1,
            catalog,
            info,
            xref
        )
        .as_bytes(),
    );
    out
}
