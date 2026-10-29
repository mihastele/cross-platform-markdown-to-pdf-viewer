use mdpdf_core::ast::{Align, Block, Inline};
use mdpdf_core::{layout, markdown_to_pdf, parse, Config, Op};

fn para(src: &str) -> Vec<Inline> {
    match parse(src).blocks.into_iter().next() {
        Some(Block::Paragraph(p)) => p,
        other => panic!("expected paragraph, got {other:?}"),
    }
}

#[test]
fn emphasis_nesting_and_intraword_underscore() {
    let p = para("a **b *c* d** snake_case_word");
    assert_eq!(
        p,
        vec![
            Inline::Text("a ".into()),
            Inline::Strong(vec![
                Inline::Text("b ".into()),
                Inline::Emph(vec![Inline::Text("c".into())]),
                Inline::Text(" d".into())
            ]),
            Inline::Text(" snake_case_word".into()),
        ]
    );
}

#[test]
fn strike_code_and_hard_break() {
    let p = para("~~gone~~ `x`  \nnext");
    assert_eq!(p[0], Inline::Strike(vec![Inline::Text("gone".into())]));
    assert!(p.contains(&Inline::Code("x".into())));
    assert!(p.contains(&Inline::HardBreak));
}

#[test]
fn links_images_refs_autolinks() {
    let doc = parse("[a](http://x.y \"t\") ![i](p.png) [r][k] <https://z.io> www.q.com.\n\n[k]: http://ref");
    let Block::Paragraph(p) = &doc.blocks[0] else { panic!() };
    assert!(matches!(&p[0], Inline::Link { url, .. } if url == "http://x.y"));
    assert!(p.iter().any(|i| matches!(i, Inline::Image { url, alt } if url == "p.png" && alt == "i")));
    assert!(p.iter().any(|i| matches!(i, Inline::Link { url, .. } if url == "http://ref")));
    assert!(p.iter().any(|i| matches!(i, Inline::Link { url, .. } if url == "https://z.io")));
    assert!(p.iter().any(|i| matches!(i, Inline::Link { url, .. } if url == "http://www.q.com")));
    assert_eq!(doc.blocks.len(), 1, "reference definition must not render");
}

#[test]
fn nested_lists_and_tasks() {
    let doc = parse("- a\n  - b\n- [x] c\n- [ ] d\n");
    let Block::List { items, tight, .. } = &doc.blocks[0] else { panic!() };
    assert!(*tight);
    assert_eq!(items.len(), 3);
    assert_eq!(items[0].blocks.len(), 2);
    assert_eq!(items[2].checked, Some(false));
    assert_eq!(items[1].checked, Some(true));
}

#[test]
fn table_alignment() {
    let doc = parse("| a | b |\n|:-:|--:|\n| 1 | 2 |\n| 3 |\n");
    let Block::Table { aligns, rows, .. } = &doc.blocks[0] else { panic!() };
    assert_eq!(aligns, &[Align::Center, Align::Right]);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[1].len(), 2, "short rows are padded");
}

#[test]
fn fences_keep_markdown_literal() {
    let doc = parse("```\n# not a heading\n*x*\n```\n");
    assert_eq!(
        doc.blocks,
        vec![Block::CodeBlock { lang: String::new(), text: "# not a heading\n*x*".into() }]
    );
}

#[test]
fn pagination_and_valid_structure() {
    let src: String = (0..400).map(|i| format!("Paragraph number {i} with some words.\n\n")).collect();
    let cfg = Config::a4();
    let lay = layout(&parse(&src), &cfg);
    assert!(lay.pages.len() > 3);
    // Nothing may be drawn below the bottom margin (footer excluded).
    let limit = cfg.page_height - cfg.margin_bottom + 1.0;
    for p in &lay.pages {
        for op in &p.ops {
            if let Op::Text { y, text, .. } = op {
                if !text.contains('/') {
                    assert!(*y <= limit, "text at y={y} overflows");
                }
            }
        }
    }
    let pdf = markdown_to_pdf(&src, &cfg);
    let s = String::from_utf8_lossy(&pdf);
    assert!(s.starts_with("%PDF-1.4"));
    assert!(s.trim_end().ends_with("%%EOF"));
    assert_eq!(s.matches("/Type /Page ").count(), lay.pages.len());
    // startxref must point at the xref table.
    let off: usize = s.rsplit("startxref\n").next().unwrap().lines().next().unwrap().parse().unwrap();
    assert!(pdf[off..].starts_with(b"xref"));
}

#[test]
fn continuous_mode_is_one_tall_page() {
    let mut cfg = Config::a4();
    cfg.paginate = false;
    let src: String = (0..200).map(|i| format!("P{i}\n\n")).collect();
    let lay = layout(&parse(&src), &cfg);
    assert_eq!(lay.pages.len(), 1);
    assert!(lay.height > cfg.page_height);
}

#[test]
fn degenerate_inputs_do_not_panic() {
    for s in ["", "\n\n", "|", "|-|", "[", "![", "**", "`", "> ", "- ", "1.", "<", "&", "\\", "# ", "~~~", "[]()", "[a](", "***a**b*"] {
        let _ = markdown_to_pdf(s, &Config::a4());
    }
}
