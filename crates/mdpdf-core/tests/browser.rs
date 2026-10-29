//! "Never cut" guarantees of Designed mode (browser engine), checked on the
//! real PDFs a real browser produces. Needs a Chromium-based browser and
//! `pdftotext` (poppler); otherwise each test skips itself.

use mdpdf_core::browser::find_browser;
use mdpdf_core::{convert, Config, Source};
use std::process::{Command, Stdio};

const FX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

fn available() -> bool {
    let tool = Command::new("pdftotext").arg("-v").stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok();
    let ok = tool && find_browser(None).is_some();
    if !ok {
        eprintln!("SKIPPED: needs a Chromium-based browser (set MDPDF_BROWSER) and pdftotext");
    }
    ok
}

fn cfg() -> Config {
    let mut c = Config::designed_a4();
    c.base_dir = Some(FX.into());
    c
}

/// Renders Markdown/HTML through the browser; returns the text of every page.
fn pages(src: &str, kind: Source) -> Vec<String> {
    let pdf = convert(src, kind, &cfg()).unwrap_or_else(|e| panic!("browser conversion failed: {e}"));
    let dir = std::env::temp_dir().join(format!("mdpdf-test-{}-{:?}", std::process::id(), std::thread::current().id()));
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join(format!("{}.pdf", pdf.len() ^ src.len()));
    std::fs::write(&f, &pdf).unwrap();
    let out = Command::new("pdftotext").args(["-layout"]).arg(&f).arg("-").output().unwrap();
    let _ = std::fs::remove_file(&f);
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let mut v: Vec<String> = text.split('\x0c').map(str::to_string).collect();
    if v.last().is_some_and(|l| l.trim().is_empty()) {
        v.pop();
    }
    v
}

/// Runs `f` over `cases` on a few threads (each case spawns a browser).
fn par<T: Sync, R: Send>(cases: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let n = 6;
    let chunk = cases.len().div_ceil(n).max(1);
    std::thread::scope(|s| {
        let hs: Vec<_> = cases.chunks(chunk).map(|c| s.spawn(|| c.iter().map(&f).collect::<Vec<R>>())).collect();
        hs.into_iter().flat_map(|h| h.join().unwrap()).collect()
    })
}

/// Fillers plus a spacer swept in small steps, so the block under test meets
/// the page bottom at many different alignments.
fn prefix(fillers: usize, spacer: usize) -> String {
    let mut s = String::new();
    for i in 0..fillers {
        s.push_str(&format!("Filler {i}.\n\n"));
    }
    s.push_str(&format!("<p style=\"margin-bottom: {spacer}pt\">spacer</p>\n\n"));
    s
}

fn has_word(page: &str, w: &str) -> bool {
    page.split_whitespace().any(|x| x == w)
}

fn grid(fillers: &[usize], spacers: std::ops::Range<usize>, step: usize) -> Vec<(usize, usize)> {
    fillers.iter().flat_map(|&f| spacers.clone().step_by(step).map(move |s| (f, s))).collect()
}

#[test]
fn headings_stay_with_whatever_follows_them() {
    if !available() {
        return;
    }
    let followers: [(&str, &str); 5] = [
        ("| H1 | H2 |\n|---|---|\n| ROWMARK | x |\n| r2 | y |\n", "ROWMARK"),
        ("```\nCODEMARK\nline 2\nline 3\nline 4\n```\n", "CODEMARK"),
        ("- ITEMMARK one\n- two\n", "ITEMMARK"),
        ("Plain paragraph PARAMARK with enough words to wrap onto a second line when it is long enough to do so, which it now is.\n", "PARAMARK"),
        ("![p](photo.jpg \"CAPMARK\")\n", "CAPMARK"),
    ];
    let mut cases = Vec::new();
    for (f, m) in followers {
        for (fillers, spacer) in grid(&[24, 25, 26, 27], 0..40, 8) {
            cases.push((f, m, fillers, spacer));
        }
    }
    let results = par(&cases, |&(follow, marker, fillers, spacer)| {
        let src = format!("{}## HEADMARK\n\n{}", prefix(fillers, spacer), follow);
        let ps = pages(&src, Source::Markdown);
        let bad = ps.iter().enumerate().any(|(_, p)| has_word(p, "HEADMARK") && !p.contains(marker));
        (bad, ps.len() > 1, format!("{marker} fillers={fillers} spacer={spacer}"))
    });
    for (bad, _, what) in &results {
        assert!(!bad, "heading stranded from its content: {what}");
    }
    let crossed = results.iter().filter(|r| r.1).count();
    assert!(crossed > results.len() / 2, "sweep must reach page boundaries ({crossed}/{})", results.len());
}

#[test]
fn paragraphs_have_no_widows_or_orphans() {
    if !available() {
        return;
    }
    let para = "word ".repeat(120);
    let cases = grid(&[22, 24, 26], 0..60, 4);
    let results = par(&cases, |&(fillers, spacer)| {
        let ps = pages(&format!("{}{para}\n", prefix(fillers, spacer)), Source::Markdown);
        let counts: Vec<usize> = ps.iter().map(|p| p.lines().filter(|l| l.contains("word")).count()).collect();
        (counts, fillers, spacer)
    });
    let mut split = 0;
    for (counts, f, s) in &results {
        if counts.iter().filter(|&&c| c > 0).count() > 1 {
            split += 1;
        }
        for (p, &c) in counts.iter().enumerate() {
            assert!(c == 0 || c >= 2, "fillers={f} spacer={s}: page {p} has a lone line: {counts:?}");
        }
    }
    assert!(split > 5, "sweep must split paragraphs ({split})");
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
    if !available() {
        return;
    }
    let cases = grid(&[18, 22, 24, 26], 0..40, 5);
    let results = par(&cases, |&(fillers, spacer)| {
        let ps = pages(&format!("{}{}", prefix(fillers, spacer), table_src(8)), Source::Markdown);
        let with_rows: Vec<usize> = (0..ps.len()).filter(|&p| ps[p].contains("row")).collect();
        (with_rows, ps.len(), fillers, spacer)
    });
    for (pages_with_rows, _, f, s) in &results {
        assert_eq!(pages_with_rows.len(), 1, "fillers={f} spacer={s}: table split over pages {pages_with_rows:?}");
    }
    assert!(results.iter().any(|r| r.1 > 1 && r.0[0] > 0), "some case must push the table to a later page");
}

#[test]
fn long_tables_split_between_rows_with_repeated_header_and_no_stragglers() {
    if !available() {
        return;
    }
    let cases: Vec<(usize, usize)> = [0usize, 12, 24].iter().flat_map(|&f| (30..110).step_by(3).map(move |r| (f, r))).collect();
    let results = par(&cases, |&(fillers, rows)| {
        let ps = pages(&format!("{}{}", prefix(fillers, 0), table_src(rows)), Source::Markdown);
        let per_page: Vec<usize> = ps
            .iter()
            .map(|p| (0..rows).filter(|i| has_word(p, &format!("row{i}"))).count())
            .collect();
        let headers: Vec<bool> = ps.iter().map(|p| has_word(p, "A") && has_word(p, "B")).collect();
        (per_page, headers, fillers, rows)
    });
    let mut split = 0;
    for (per_page, headers, f, r) in &results {
        assert_eq!(per_page.iter().sum::<usize>(), *r, "fillers={f} rows={r}: every row exactly once {per_page:?}");
        let used: Vec<usize> = (0..per_page.len()).filter(|&p| per_page[p] > 0).collect();
        if used.len() > 1 {
            split += 1;
        }
        for &p in &used {
            assert!(headers[p], "fillers={f} rows={r}: page {p} lacks the repeated header");
            if used.len() > 1 {
                assert!(per_page[p] >= 2, "fillers={f} rows={r}: page {p} holds a lone row {per_page:?}");
            }
        }
    }
    assert!(split > 40, "sweep must split tables ({split})");
}

#[test]
fn code_images_and_callouts_stay_whole() {
    if !available() {
        return;
    }
    let code: String = (0..12).map(|i| format!("CODE{i}\n")).collect();
    let body = "word ".repeat(40);
    let cases = grid(&[22, 24, 26], 0..60, 6);
    let results = par(&cases, |&(fillers, spacer)| {
        let src = format!(
            "{}```\n{code}```\n\n![p](photo.jpg \"CAPMARK\")\n\n> [!TIP]\n> CALLMARK {body}ENDMARK\n",
            prefix(fillers, spacer)
        );
        let ps = pages(&src, Source::Markdown);
        let on = |m: &str| -> Vec<usize> { (0..ps.len()).filter(|&p| ps[p].contains(m)).collect() };
        (on("CODE0").len() + on("CODE11").len(), on("CODE0"), on("CODE11"), on("CALLMARK"), on("ENDMARK"), fillers, spacer)
    });
    for (_, c0, c11, cm, em, f, s) in &results {
        assert_eq!(c0, c11, "fillers={f} spacer={s}: code block split");
        assert_eq!(cm, em, "fillers={f} spacer={s}: callout split");
    }
}

#[test]
fn html_documents_get_the_same_protection() {
    if !available() {
        return;
    }
    // A hand-written HTML page with its own CSS: tables, headings and details.
    let cases: Vec<usize> = (0..14).map(|i| 20 + i).collect();
    let results = par(&cases, |&fillers| {
        let mut h = String::from("<html><head><style>body{font-family:sans-serif} p{margin:0 0 9pt} .box{background:#eef}</style></head><body>");
        for i in 0..fillers {
            h.push_str(&format!("<p>Filler {i}.</p>"));
        }
        h.push_str("<h2>HEADMARK</h2><table class=box><tr><th>A</th><th>B</th></tr>");
        for i in 0..10 {
            h.push_str(&format!("<tr><td>row{i}</td><td>v</td></tr>"));
        }
        h.push_str("</table><details><summary>SUMMARY</summary><p>DETAILMARK text</p></details></body></html>");
        let ps = pages(&h, Source::Html);
        let on = |m: &str| -> Vec<usize> { (0..ps.len()).filter(|&p| ps[p].contains(m)).collect() };
        let rows: Vec<usize> = (0..ps.len()).filter(|&p| ps[p].contains("row")).collect();
        (on("HEADMARK"), rows, on("DETAILMARK"), fillers)
    });
    for (head, rows, detail, f) in &results {
        assert_eq!(rows.len(), 1, "fillers={f}: user's table split {rows:?}");
        assert_eq!(head[0], rows[0], "fillers={f}: heading {head:?} separated from its table {rows:?}");
        assert_eq!(detail.len(), 1, "fillers={f}: <details> content must be printed (it is closed by default)");
    }
}

#[test]
fn explicit_page_breaks_and_markdown_in_html_blocks() {
    if !available() {
        return;
    }
    let ps = pages("# Title\n\n<div align=\"center\">\n\n**centered**\n\n</div>\n\n<!-- pagebreak -->\n\nSecond page\n", Source::Markdown);
    assert_eq!(ps.len(), 2, "{ps:?}");
    assert!(ps[0].contains("centered") && ps[1].contains("Second page"));
}
