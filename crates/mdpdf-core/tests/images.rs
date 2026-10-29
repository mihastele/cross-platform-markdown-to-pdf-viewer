use mdpdf_core::image::{decode, load, Pixels};
use mdpdf_core::{markdown_to_pdf, Config};
use std::path::Path;

const FX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

fn fx(name: &str) -> Vec<u8> {
    std::fs::read(Path::new(FX).join(name)).unwrap()
}

/// Expected RGBA comes from PIL (an independent decoder).
fn check_png(name: &str) {
    let img = decode(&fx(&format!("{name}.png"))).unwrap_or_else(|e| panic!("{name}: {e}"));
    let expected = fx(&format!("{name}.expected"));
    let Pixels::Raw { rgb, alpha } = &img.pixels else { panic!("png must decode to raw pixels") };
    assert_eq!(expected.len(), (img.width * img.height * 4) as usize, "{name}: size");
    for i in 0..(img.width * img.height) as usize {
        let e = &expected[i * 4..i * 4 + 4];
        let a = alpha.as_ref().map_or(255, |a| a[i]);
        assert_eq!(&rgb[i * 3..i * 3 + 3], &e[..3], "{name}: rgb at pixel {i}");
        assert_eq!(a, e[3], "{name}: alpha at pixel {i}");
    }
}

#[test]
fn png_rgba_with_all_filter_types() {
    check_png("rgba");
}

#[test]
fn png_gray_16_bit() {
    check_png("gray16");
}

#[test]
fn png_palette_4_bit_with_transparency() {
    check_png("pal4");
}

#[test]
fn png_adam7_interlaced() {
    check_png("adam7");
}

#[test]
fn jpeg_headers_baseline_progressive_gray() {
    for (f, comps) in [("photo.jpg", 3), ("progressive.jpg", 3), ("gray.jpg", 1)] {
        let img = decode(&fx(f)).unwrap();
        assert_eq!((img.width, img.height), (480, 300), "{f}");
        let Pixels::Jpeg { components, data, .. } = &img.pixels else { panic!() };
        assert_eq!(*components, comps, "{f}");
        assert_eq!(data, &fx(f), "JPEG bytes must pass through untouched");
    }
}

#[test]
fn corrupt_and_unknown_images_are_rejected_not_panicking() {
    assert!(decode(b"GIF89a....").is_err());
    let mut png = fx("rgba.png");
    png.truncate(png.len() / 2);
    assert!(decode(&png).is_err());
    assert!(decode(&[0xff, 0xd8, 0xff]).is_err());
}

#[test]
fn data_uri_and_remote_urls() {
    let b64: String = {
        const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let data = fx("rgba.png");
        let mut out = String::new();
        for c in data.chunks(3) {
            let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
            for i in 0..4 {
                if i <= c.len() {
                    out.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
                } else {
                    out.push('=');
                }
            }
        }
        out
    };
    assert!(load(&format!("data:image/png;base64,{b64}"), None).is_some());
    assert!(load("https://example.com/x.png", None).is_none(), "remote images are never fetched");
    assert!(load("missing.png", Some(Path::new(FX))).is_none());
}

#[test]
fn pdf_embeds_images_with_soft_mask() {
    let mut cfg = Config::a4();
    cfg.base_dir = Some(FX.into());
    let pdf = markdown_to_pdf("![a](rgba.png)\n\n![b](photo.jpg)\n\n![c](rgba.png)", &cfg);
    let s = String::from_utf8_lossy(&pdf);
    assert!(s.contains("/Subtype /Image"));
    assert!(s.contains("/SMask"), "alpha PNG needs a soft mask");
    assert!(s.contains("/DCTDecode"), "JPEG passes through");
    assert_eq!(s.matches("/Subtype /Image").count(), 3, "rgba (+mask) and jpeg; duplicates are shared");
    assert!(s.contains("/Im0 ") && s.contains("/Im1 "), "page resources must name both images");
}

#[test]
fn missing_image_degrades_to_labelled_placeholder() {
    let pdf = markdown_to_pdf("![logo](nope.png)", &Config::a4());
    assert!(!String::from_utf8_lossy(&pdf).contains("/Subtype /Image"));
    let lay = mdpdf_core::layout(&mdpdf_core::parse("![logo](nope.png)"), &Config::a4());
    let text: String = lay.pages[0]
        .ops
        .iter()
        .filter_map(|o| if let mdpdf_core::Op::Text { text, .. } = o { Some(text.as_str()) } else { None })
        .collect::<Vec<_>>()
        .join(" ");
    assert!(text.contains("logo"), "{text}");
}
