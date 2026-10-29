use mdpdf_core::ast::{Align, Block, Inline};
use mdpdf_core::{convert, layout, parse_source, render, Config, Layout, Op, Source};

fn designed() -> Config {
    let mut c = Config::designed_builtin_a4();
    c.base_dir = Some(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures").into());
    c
}

fn text_of(lay: &Layout, page: usize) -> String {
    lay.pages[page]
        .ops
        .iter()
        .filter_map(|o| if let Op::Text { text, .. } = o { Some(text.as_str()) } else { None })
        .collect::<Vec<_>>()
        .join(" ")
}

fn body_ops<'a>(lay: &'a Layout, page: usize, cfg: &Config) -> Vec<&'a Op> {
    // Ops inside the page body (excludes running header/footer furniture).
    lay.pages[page]
        .ops
        .iter()
        .filter(|o| match o {
            Op::Text { y, .. } => *y > cfg.margin_top - 4.0 && *y < cfg.page_height - cfg.margin_bottom + 2.0,
            _ => true,
        })
        .collect()
}

#[test]
fn html_is_opt_in() {
    let std = Config::a4();
    let err = convert("<h1>x</h1>", Source::Html, &std).unwrap_err();
    assert!(err.contains("Designed"), "{err}");
    assert!(convert("<h1>x</h1>", Source::Html, &designed()).is_ok());
    // In Standard mode embedded HTML is NOT interpreted: tags stay literal text.
    let doc = parse_source("<div align=\"center\">\n\nhi\n\n</div>", Source::Markdown, &std).unwrap();
    assert!(doc.blocks.iter().all(|b| !matches!(b, Block::Div { .. })));
}

#[test]
fn markdown_embeds_html_blocks_and_inline_tags() {
    let doc = parse_source(
        "<div align=\"center\">\n\n# Title\n\n</div>\n\nA <b>bold</b> and <span style=\"color:#ff0000\">red</span> word<br>next",
        Source::Markdown,
        &designed(),
    )
    .unwrap();
    let Block::Div { style, blocks } = &doc.blocks[0] else { panic!("{:?}", doc.blocks) };
    assert_eq!(style.align, Some(Align::Center));
    assert!(matches!(blocks[0], Block::Heading { level: 1, .. }), "markdown inside the div is parsed");
    let Block::Paragraph(p) = &doc.blocks[1] else { panic!() };
    assert!(p.iter().any(|i| matches!(i, Inline::Styled { style, .. } if style.bold)));
    assert!(p.iter().any(|i| matches!(i, Inline::Styled { style, .. } if style.color.is_some())));
    assert!(p.contains(&Inline::HardBreak));
}

#[test]
fn callouts_only_in_designed_mode() {
    let src = "> [!WARNING]\n> careful";
    let d = parse_source(src, Source::Markdown, &designed()).unwrap();
    assert!(matches!(d.blocks[0], Block::Callout { .. }));
    let s = parse_source(src, Source::Markdown, &Config::a4()).unwrap();
    assert!(matches!(s.blocks[0], Block::Quote(_)));
}

#[test]
fn page_break_comment_and_css() {
    let lay = render("one\n\n<!-- pagebreak -->\n\ntwo", Source::Markdown, &designed()).unwrap();
    assert_eq!(lay.pages.len(), 2);
    let lay = render("<p>a</p><p style=\"page-break-before: always\">b</p>", Source::Html, &designed()).unwrap();
    assert_eq!(lay.pages.len(), 2, "exactly one break, no blank page");
}

/// Fillers followed by a spacer whose height is swept in 2pt steps, so the
/// block under test meets the page bottom at every possible alignment.
fn prefix(fillers: usize, spacer: usize) -> String {
    let mut src = String::new();
    for i in 0..fillers {
        src.push_str(&format!("Filler {i}.\n\n"));
    }
    src.push_str(&format!("<p style=\"margin-bottom: {spacer}pt\">spacer</p>\n\n"));
    src
}

#[test]
fn headings_stay_with_whatever_follows_them() {
    let cfg = designed();
    let followers: [(&str, &str); 5] = [
        ("| H1 | H2 |\n|---|---|\n| ROWMARK | x |\n| r2 | y |\n", "ROWMARK"),
        ("```\nCODEMARK\nline 2\nline 3\nline 4\n```\n", "CODEMARK"),
        ("- ITEMMARK one\n- two\n", "ITEMMARK"),
        ("Plain paragraph PARAMARK with enough words to wrap onto a second line when it is long enough to do so.\n", "PARAMARK"),
        ("![p](photo.jpg)\n", "\u{0}image"),
    ];
    for (follow, marker) in followers {
        for fillers in [22, 24, 26, 28, 40] {
            for spacer in (0..80).step_by(2) {
                let src = format!("{}## HEADMARK\n\n{}", prefix(fillers, spacer), follow);
                let lay = render(&src, Source::Markdown, &cfg).unwrap();
                for (pi, page) in lay.pages.iter().enumerate() {
                    let t = text_of(&lay, pi);
                    if !t.contains("HEADMARK") {
                        continue;
                    }
                    let has_follower = if marker == "\u{0}image" {
                        page.ops.iter().any(|o| matches!(o, Op::Image { .. }))
                    } else {
                        t.contains(marker)
                    };
                    assert!(has_follower, "{marker}: fillers={fillers} spacer={spacer}: heading stranded on page {pi}");
                }
            }
        }
    }
}

#[test]
fn paragraphs_have_no_widows_or_orphans() {
    let cfg = designed();
    let para = "word ".repeat(120);
    let mut multi = 0;
    for fillers in [18, 20, 22, 24, 26, 40] {
        for spacer in (0..70).step_by(2) {
            let src = format!("{}{para}\n", prefix(fillers, spacer));
            let lay = render(&src, Source::Markdown, &cfg).unwrap();
            if lay.pages.len() < 2 {
                continue;
            }
            let counts: Vec<usize> = (0..lay.pages.len())
                .map(|p| {
                    let mut ys: Vec<i32> = body_ops(&lay, p, &cfg)
                        .iter()
                        .filter_map(|o| match o {
                            Op::Text { y, text, .. } if text == "word" => Some((*y * 10.0) as i32),
                            _ => None,
                        })
                        .collect();
                    ys.dedup();
                    ys.len()
                })
                .collect();
            if counts.iter().filter(|&&c| c > 0).count() > 1 {
                multi += 1;
            }
            for (p, &c) in counts.iter().enumerate() {
                assert!(c == 0 || c >= 2, "fillers={fillers} spacer={spacer}: page {p} has {c} line(s): {counts:?}");
            }
        }
    }
    assert!(multi > 10, "sweep must actually split paragraphs ({multi})");
}

fn table_src(rows: usize) -> String {
    let mut s = String::from("| A | B |\n|---|---|\n");
    for i in 0..rows {
        s.push_str(&format!("| row{i} | v{i} |\n"));
    }
    s
}

#[test]
fn small_tables_are_never_split() {
    let cfg = designed();
    for filler in 0..50 {
        let mut src = String::new();
        for i in 0..filler {
            src.push_str(&format!("Filler {i}.\n\n"));
        }
        src.push_str(&table_src(8));
        let lay = render(&src, Source::Markdown, &cfg).unwrap();
        let pages_with_rows: Vec<usize> = (0..lay.pages.len()).filter(|&p| text_of(&lay, p).contains("row")).collect();
        assert_eq!(pages_with_rows.len(), 1, "filler={filler}: table split over pages {pages_with_rows:?}");
    }
}

#[test]
fn long_tables_split_between_rows_with_repeated_header_and_no_stragglers() {
    let cfg = designed();
    let mut split = 0;
    for fillers in [0, 10, 20, 26, 34] {
        for rows in 20..110 {
            let src = format!("{}{}", prefix(fillers, 0), table_src(rows));
            let lay = render(&src, Source::Markdown, &cfg).unwrap();
            let mut seen = 0;
            let mut pages_with_rows = 0;
            for p in 0..lay.pages.len() {
                let t = text_of(&lay, p);
                let n = (0..rows).filter(|i| t.split_whitespace().any(|w| w == format!("row{i}"))).count();
                if n == 0 {
                    continue;
                }
                pages_with_rows += 1;
                seen += n;
                assert!(t.split_whitespace().any(|w| w == "A"), "fillers={fillers} rows={rows}: page {p} lacks the repeated header");
                if lay.pages.len() > 1 && rows > 12 {
                    assert!(n >= 2, "fillers={fillers} rows={rows}: page {p} holds a lone row ({n})");
                }
            }
            assert_eq!(seen, rows, "fillers={fillers} rows={rows}: every row exactly once");
            if pages_with_rows > 1 {
                split += 1;
            }
        }
    }
    assert!(split > 50, "sweep must exercise splitting ({split})");
}

#[test]
fn images_and_callouts_stay_whole() {
    let cfg = designed();
    for fillers in [20, 24, 26, 28, 34] {
        for spacer in (0..80).step_by(4) {
            let src = format!(
                "{}![p](photo.jpg \"CAPMARK\")\n\n> [!NOTE]\n> line one\n> line two\n> line three\n",
                prefix(fillers, spacer)
            );
            let lay = render(&src, Source::Markdown, &cfg).unwrap();
            for (pi, page) in lay.pages.iter().enumerate() {
                for op in &page.ops {
                    if let Op::Image { y, h, .. } = op {
                        assert!(*y + *h <= cfg.page_height - cfg.margin_bottom + 0.5, "fillers={fillers} spacer={spacer}: image overflows page {pi}");
                        assert!(text_of(&lay, pi).contains("CAPMARK"), "fillers={fillers} spacer={spacer}: caption separated from image");
                    }
                }
            }
            let note_pages: Vec<usize> = (0..lay.pages.len()).filter(|&p| text_of(&lay, p).contains("line")).collect();
            assert_eq!(note_pages.len(), 1, "fillers={fillers} spacer={spacer}: callout split across {note_pages:?}");
            let has_title = (0..lay.pages.len()).any(|p| text_of(&lay, p).contains("Note"));
            assert!(has_title);
        }
    }
}

#[test]
fn nothing_is_drawn_below_the_bottom_margin() {
    let cfg = designed();
    let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/designed.md")).unwrap();
    let mut c = cfg.clone();
    c.base_dir = Some(concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples").into());
    let lay = layout(&mdpdf_core::parse_with(&src, &mdpdf_core::ParseOptions { html: true, callouts: true, raw_html: false }), &c);
    for (pi, p) in lay.pages.iter().enumerate() {
        for op in &p.ops {
            let bottom = match op {
                Op::Text { y, text, .. } if !text.starts_with("Page ") => *y,
                Op::Image { y, h, .. } => y + h,
                Op::Rect { y, h, .. } => y + h,
                _ => continue,
            };
            assert!(bottom <= c.page_height - c.margin_bottom + 12.0, "page {pi}: op reaches y={bottom}");
        }
    }
    assert!(lay.pages.len() >= 5);
}

#[test]
fn degenerate_html_does_not_panic() {
    for s in ["", "<", "<<>>", "<div", "</div></div>", "<table></table>", "<table><tr></tr></table>", "<ul><li></ul>", "<img>", "<a><a><a>",
              "<p style=\"color:\">x", "<style>{{{</style>", "<div style='padding:99999px'>x</div>", "&#xFFFFFFF; &bogus;"] {
        let _ = convert(s, Source::Html, &designed());
        let _ = convert(s, Source::Markdown, &designed());
    }
}

#[test]
fn callouts_and_quotes_are_kept_whole_when_they_fit() {
    let cfg = designed();
    let mut moved = 0;
    for fillers in [20, 22, 24, 26, 28] {
        for spacer in (0..80).step_by(2) {
            let body = "word ".repeat(70);
            let src = format!("{}> [!TIP]\n> CALLMARK {body}ENDMARK\n", prefix(fillers, spacer));
            let lay = render(&src, Source::Markdown, &cfg).unwrap();
            let pages: Vec<usize> = (0..lay.pages.len()).filter(|&p| { let t = text_of(&lay, p); t.contains("CALLMARK") || t.contains("ENDMARK") || t.split_whitespace().any(|w| w == "Tip") }).collect();
            assert_eq!(pages.len(), 1, "fillers={fillers} spacer={spacer}: callout split across pages {pages:?}");
            if pages[0] > 0 {
                moved += 1;
            }
        }
    }
    assert!(moved > 5, "sweep must hit the page boundary ({moved})");
}
