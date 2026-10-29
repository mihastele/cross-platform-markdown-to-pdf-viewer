use mdpdf_core::{convert, Family, Mode, Source};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "\
mdpdf - convert Markdown (or HTML) straight to PDF, with no browser or other intermediate step

USAGE:
    mdpdf [OPTIONS] <input.md | input.html | ->

OPTIONS:
    -o, --output <file>     Output path (default: input name with .pdf; '-' = stdout)
        --mode <m>          standard (default) | designed | designed-builtin
                              standard: plain Markdown, built-in engine
                              designed: opt-in. Markdown + embedded HTML + whole .html files are rendered
                              by an installed Chromium-based browser (Chrome, Edge, Chromium, Brave) with
                              page-break rules that never cut headings, tables, images or code
                              designed-builtin: the same look from the built-in engine (offline; supports
                              a smaller HTML/CSS subset)
        --browser <path>    Browser executable for designed mode (default: auto-detect, or $MDPDF_BROWSER)
        --html              Treat input as HTML (implied by a .html/.htm extension; needs --mode designed)
        --base-dir <dir>    Where relative image paths resolve (default: the input's directory)
        --page <a4|letter>  Page size (default: a4)
        --font <sans|serif> Body font (default: sans)
        --size <pt>         Base font size (default: 11, designed: 10.5)
        --margin <pt>       Margin on all sides
        --no-page-numbers   Omit page numbers
    -h, --help              Show this help
";

fn fail(msg: &str) -> ExitCode {
    eprintln!("mdpdf: {msg}\n\n{USAGE}");
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let mut input: Option<String> = None;
    let mut output: Option<String> = None;
    let (mut mode, mut letter, mut html) = (Mode::Standard, false, false);
    let (mut family, mut size, mut margin, mut numbers) = (None, None::<f32>, None::<f32>, true);
    let mut base_dir: Option<PathBuf> = None;
    let mut browser: Option<PathBuf> = None;

    while let Some(a) = args.next() {
        let mut val = |name: &str| args.next().ok_or_else(|| format!("{name} needs a value"));
        let r: Result<(), String> = (|| {
            match a.as_str() {
                "-h" | "--help" => {
                    print!("{USAGE}");
                    std::process::exit(0);
                }
                "-o" | "--output" => output = Some(val("--output")?),
                "--mode" => {
                    mode = match val("--mode")?.to_lowercase().as_str() {
                        "standard" => Mode::Standard,
                        "designed" => Mode::Designed,
                        "designed-builtin" => Mode::DesignedBuiltin,
                        m => return Err(format!("unknown mode '{m}' (standard | designed | designed-builtin)")),
                    }
                }
                "--html" => html = true,
                "--browser" => browser = Some(PathBuf::from(val("--browser")?)),
                "--base-dir" => base_dir = Some(PathBuf::from(val("--base-dir")?)),
                "--page" => match val("--page")?.to_lowercase().as_str() {
                    "a4" => letter = false,
                    "letter" => letter = true,
                    p => return Err(format!("unknown page size '{p}'")),
                },
                "--font" => {
                    family = Some(match val("--font")?.to_lowercase().as_str() {
                        "sans" => Family::Sans,
                        "serif" => Family::Serif,
                        f => return Err(format!("unknown font '{f}'")),
                    })
                }
                "--size" => size = Some(val("--size")?.parse().map_err(|_| "bad --size")?),
                "--margin" => margin = Some(val("--margin")?.parse().map_err(|_| "bad --margin")?),
                "--no-page-numbers" => numbers = false,
                s if s.starts_with('-') && s != "-" => return Err(format!("unknown option '{s}'")),
                _ => input = Some(a.clone()),
            }
            Ok(())
        })();
        if let Err(e) = r {
            return fail(&e);
        }
    }
    let Some(input) = input else { return fail("no input file") };

    let mut cfg = mode.config(letter);
    cfg.browser_path = browser;
    if let Some(f) = family {
        cfg.family = f;
    }
    if let Some(s) = size {
        cfg.base_size = s;
    }
    if let Some(m) = margin {
        cfg.margin_top = m;
        cfg.margin_right = m;
        cfg.margin_bottom = m;
        cfg.margin_left = m;
    }
    cfg.page_numbers = numbers;
    cfg.base_dir = base_dir.or_else(|| {
        (input != "-").then(|| Path::new(&input).parent().map(|p| if p.as_os_str().is_empty() { PathBuf::from(".") } else { p.to_path_buf() })).flatten()
    });
    let is_html = html || matches!(Path::new(&input).extension().and_then(|e| e.to_str()), Some("html" | "htm" | "xhtml"));
    let kind = if is_html { Source::Html } else { Source::Markdown };

    let mut src = String::new();
    let res = if input == "-" {
        std::io::stdin().read_to_string(&mut src).map(|_| ())
    } else {
        std::fs::read_to_string(&input).map(|s| src = s)
    };
    if let Err(e) = res {
        eprintln!("mdpdf: cannot read {input}: {e}");
        return ExitCode::FAILURE;
    }

    let pdf = match convert(&src, kind, &cfg) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("mdpdf: {e}");
            return ExitCode::from(2);
        }
    };
    let out = output.unwrap_or_else(|| {
        if input == "-" {
            "-".into()
        } else {
            Path::new(&input).with_extension("pdf").to_string_lossy().into_owned()
        }
    });
    let res = if out == "-" { std::io::stdout().write_all(&pdf) } else { std::fs::write(&out, &pdf) };
    if let Err(e) = res {
        eprintln!("mdpdf: cannot write {out}: {e}");
        return ExitCode::FAILURE;
    }
    if out != "-" {
        eprintln!("wrote {out} ({} bytes)", pdf.len());
    }
    ExitCode::SUCCESS
}
