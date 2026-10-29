//! Image loading. PNG is decoded here (no dependencies); JPEG is passed
//! through untouched to the PDF (DCTDecode) after reading its header.

use crate::deflate::zlib_decompress;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

#[derive(Debug)]
pub enum Pixels {
    /// 8-bit RGB, optional 8-bit alpha, row-major.
    Raw { rgb: Vec<u8>, alpha: Option<Vec<u8>> },
    /// Original JPEG bytes (baseline or progressive).
    Jpeg { data: Vec<u8>, components: u8, inverted_cmyk: bool },
}

#[derive(Debug)]
pub struct ImageData {
    /// Unique id; lets the viewer cache textures.
    pub uid: u64,
    pub width: u32,
    pub height: u32,
    pub pixels: Pixels,
}

static NEXT_UID: AtomicU64 = AtomicU64::new(1);

fn uid() -> u64 {
    NEXT_UID.fetch_add(1, Ordering::Relaxed)
}

pub fn decode(bytes: &[u8]) -> Result<ImageData, String> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]) {
        decode_png(bytes)
    } else if bytes.starts_with(&[0xff, 0xd8]) {
        decode_jpeg(bytes)
    } else {
        Err("unsupported image format (PNG and JPEG are supported)".into())
    }
}

/// Loads `url`: `data:` URIs and local files (relative to `base`). Remote
/// URLs are never fetched.
pub fn load(url: &str, base: Option<&Path>) -> Option<Arc<ImageData>> {
    let bytes = if let Some(rest) = url.strip_prefix("data:") {
        let (meta, data) = rest.split_once(',')?;
        if meta.ends_with(";base64") {
            base64(data)?
        } else {
            percent_decode(data)
        }
    } else if url.contains("://") {
        return None;
    } else {
        let clean = url.split(['?', '#']).next()?;
        let clean = String::from_utf8(percent_decode(clean)).ok()?;
        let p = Path::new(&clean);
        let full = if p.is_absolute() { p.to_path_buf() } else { base?.join(p) };
        if std::fs::metadata(&full).ok()?.len() > 64 << 20 {
            return None;
        }
        std::fs::read(full).ok()?
    };
    decode(&bytes).ok().map(Arc::new)
}

fn percent_decode(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = ((b[i + 1] as char).to_digit(16), (b[i + 2] as char).to_digit(16)) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    out
}

pub fn base64(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut n) = (0u32, 0);
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' | b'\n' | b'\r' | b' ' => continue,
            _ => return None,
        };
        acc = (acc << 6) | v as u32;
        n += 6;
        if n >= 8 {
            n -= 8;
            out.push((acc >> n) as u8);
            acc &= (1 << n) - 1;
        }
    }
    Some(out)
}

// ---------------------------------------------------------------- JPEG

fn decode_jpeg(b: &[u8]) -> Result<ImageData, String> {
    let mut i = 2;
    let mut adobe = false;
    while i + 4 <= b.len() {
        if b[i] != 0xff {
            i += 1;
            continue;
        }
        let m = b[i + 1];
        if m == 0xff {
            i += 1;
            continue;
        }
        if m == 0xd8 || m == 0x01 || (0xd0..=0xd7).contains(&m) {
            i += 2;
            continue;
        }
        let len = u16::from_be_bytes([b[i + 2], b[i + 3]]) as usize;
        let seg = b.get(i + 4..i + 2 + len).ok_or("truncated JPEG")?;
        if m == 0xee && seg.starts_with(b"Adobe") {
            adobe = true;
        }
        if matches!(m, 0xc0..=0xcf) && !matches!(m, 0xc4 | 0xc8 | 0xcc) {
            if seg.len() < 6 {
                return Err("bad JPEG frame header".into());
            }
            if m >= 0xc3 && m != 0xc2 {
                return Err("unsupported JPEG variant".into());
            }
            let h = u16::from_be_bytes([seg[1], seg[2]]) as u32;
            let w = u16::from_be_bytes([seg[3], seg[4]]) as u32;
            let comps = seg[5];
            if w == 0 || h == 0 || !matches!(comps, 1 | 3 | 4) || seg[0] != 8 {
                return Err("unsupported JPEG".into());
            }
            return Ok(ImageData {
                uid: uid(),
                width: w,
                height: h,
                pixels: Pixels::Jpeg { data: b.to_vec(), components: comps, inverted_cmyk: adobe && comps == 4 },
            });
        }
        i += 2 + len;
    }
    Err("no JPEG frame header".into())
}

// ----------------------------------------------------------------- PNG

fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

fn sample(row: &[u8], i: usize, depth: u8) -> u16 {
    match depth {
        8 => row[i] as u16,
        16 => u16::from_be_bytes([row[2 * i], row[2 * i + 1]]),
        d => {
            let bit = i * d as usize;
            let byte = row[bit / 8];
            let shift = 8 - d as usize - (bit % 8);
            ((byte >> shift) & ((1u8 << d) - 1)) as u16
        }
    }
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let (ia, ib, ic) = (a as i32, b as i32, c as i32);
    let p = ia + ib - ic;
    let (pa, pb, pc) = ((p - ia).abs(), (p - ib).abs(), (p - ic).abs());
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

fn decode_png(b: &[u8]) -> Result<ImageData, String> {
    let mut pos = 8;
    let (mut w, mut h, mut depth, mut ctype, mut interlace) = (0u32, 0u32, 0u8, 0u8, 0u8);
    let mut idat = Vec::new();
    let mut plte: Vec<u8> = Vec::new();
    let mut trns: Vec<u8> = Vec::new();
    while pos + 8 <= b.len() {
        let len = be32(&b[pos..]) as usize;
        let kind = &b[pos + 4..pos + 8];
        let data = b.get(pos + 8..pos + 8 + len).ok_or("truncated PNG chunk")?;
        match kind {
            b"IHDR" => {
                if data.len() < 13 {
                    return Err("bad IHDR".into());
                }
                w = be32(data);
                h = be32(&data[4..]);
                depth = data[8];
                ctype = data[9];
                interlace = data[12];
            }
            b"PLTE" => plte = data.to_vec(),
            b"tRNS" => trns = data.to_vec(),
            b"IDAT" => idat.extend_from_slice(data),
            b"IEND" => break,
            _ => {}
        }
        pos += 12 + len;
    }
    if w == 0 || h == 0 || w > 30000 || h > 30000 || (w as u64 * h as u64) > 120_000_000 {
        return Err("bad PNG dimensions".into());
    }
    let channels = match ctype {
        0 | 3 => 1,
        2 => 3,
        4 => 2,
        6 => 4,
        _ => return Err("bad PNG color type".into()),
    };
    if !matches!(depth, 1 | 2 | 4 | 8 | 16) || (ctype == 3 && plte.is_empty()) {
        return Err("unsupported PNG".into());
    }
    let raw = zlib_decompress(&idat)?;
    let bits_pp = channels * depth as usize;
    let bpp = bits_pp.div_ceil(8).max(1);
    let (w, h) = (w as usize, h as usize);
    let mut rgb = vec![0u8; w * h * 3];
    let has_alpha = matches!(ctype, 4 | 6) || !trns.is_empty();
    let mut alpha = if has_alpha { vec![255u8; w * h] } else { Vec::new() };
    let passes: &[(usize, usize, usize, usize)] = if interlace == 0 {
        &[(0, 0, 1, 1)]
    } else {
        &[(0, 0, 8, 8), (4, 0, 8, 8), (0, 4, 4, 8), (2, 0, 4, 4), (0, 2, 2, 4), (1, 0, 2, 2), (0, 1, 1, 2)]
    };
    let scale8 = |v: u16| -> u8 {
        match depth {
            16 => (v >> 8) as u8,
            8 => v as u8,
            d => (v as u32 * 255 / ((1u32 << d) - 1)) as u8,
        }
    };
    let mut off = 0;
    for &(x0, y0, dx, dy) in passes {
        if w <= x0 || h <= y0 {
            continue;
        }
        let pw = (w - x0).div_ceil(dx);
        let ph = (h - y0).div_ceil(dy);
        let stride = (pw * bits_pp).div_ceil(8);
        let mut prev = vec![0u8; stride];
        for py in 0..ph {
            let line = raw.get(off..off + 1 + stride).ok_or("truncated PNG data")?;
            off += 1 + stride;
            let mut cur = line[1..].to_vec();
            for i in 0..stride {
                let a = if i >= bpp { cur[i - bpp] } else { 0 };
                let up = prev[i];
                let c = if i >= bpp { prev[i - bpp] } else { 0 };
                cur[i] = cur[i].wrapping_add(match line[0] {
                    0 => 0,
                    1 => a,
                    2 => up,
                    3 => ((a as u16 + up as u16) / 2) as u8,
                    4 => paeth(a, up, c),
                    _ => return Err("bad PNG filter".into()),
                });
            }
            for px in 0..pw {
                let (x, y) = (x0 + px * dx, y0 + py * dy);
                let o = y * w + x;
                let s = |k: usize| sample(&cur, px * channels + k, depth);
                let (r, g, bl, a): (u8, u8, u8, u8) = match ctype {
                    0 => {
                        let v = s(0);
                        let a = if trns.len() >= 2 && v == u16::from_be_bytes([trns[0], trns[1]]) { 0 } else { 255 };
                        let g = scale8(v);
                        (g, g, g, a)
                    }
                    2 => {
                        let (r, g, b2) = (s(0), s(1), s(2));
                        let a = if trns.len() >= 6
                            && r == u16::from_be_bytes([trns[0], trns[1]])
                            && g == u16::from_be_bytes([trns[2], trns[3]])
                            && b2 == u16::from_be_bytes([trns[4], trns[5]])
                        {
                            0
                        } else {
                            255
                        };
                        (scale8(r), scale8(g), scale8(b2), a)
                    }
                    3 => {
                        let i = s(0) as usize;
                        let p = plte.get(i * 3..i * 3 + 3).ok_or("palette index out of range")?;
                        (p[0], p[1], p[2], trns.get(i).copied().unwrap_or(255))
                    }
                    4 => {
                        let g = scale8(s(0));
                        (g, g, g, scale8(s(1)))
                    }
                    _ => (scale8(s(0)), scale8(s(1)), scale8(s(2)), scale8(s(3))),
                };
                rgb[o * 3] = r;
                rgb[o * 3 + 1] = g;
                rgb[o * 3 + 2] = bl;
                if has_alpha {
                    alpha[o] = a;
                }
            }
            prev = cur;
        }
    }
    let alpha = (has_alpha && alpha.iter().any(|&a| a != 255)).then_some(alpha);
    Ok(ImageData { uid: uid(), width: w as u32, height: h as u32, pixels: Pixels::Raw { rgb, alpha } })
}
