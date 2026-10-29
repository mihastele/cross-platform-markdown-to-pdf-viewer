use mdpdf_core::{markdown_to_pdf, Config, Family};
use std::io::{Read, Write};
use std::process::ExitCode;

const USAGE: &str = "\
mdpdf - convert Markdown straight to PDF (no HTML, no intermediate formats)

USAGE:
    mdpdf [OPTIONS] <input.md | ->

OPTIONS:
    -o, --output <file>   Output path (default: input name with .pdf; '-' = stdout)
        --page <a4|letter>  Page size (default: a4)
        --font <sans|serif> Body font (default: sans)
        --size <pt>       Base font size (default: 11)
        --margin <pt>     Margin on all sides (default: 60)
        --no-page-numbers Omit the page number footer
    -h, --help            Show this help
";

fn fail(msg: &str) -> ExitCode {
    eprintln!("mdpdf: {msg}\n\n{USAGE}");
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let mut input: Option<String> = None;
    let mut output: Option<String> = None;
    let mut cfg = Config::a4();
    let mut margin: Option<f32> = None;

    while let Some(a) = args.next() {
        let mut val = |name: &str| args.next().ok_or_else(|| format!("{name} needs a value"));
        let r: Result<(), String> = (|| {
            match a.as_str() {
                "-h" | "--help" => {
                    print!("{USAGE}");
                    std::process::exit(0);
                }
                "-o" | "--output" => output = Some(val("--output")?),
                "--page" => match val("--page")?.to_lowercase().as_str() {
                    "a4" => cfg = Config { ..Config::a4() },
                    "letter" => cfg = Config::letter(),
                    p => return Err(format!("unknown page size '{p}'")),
                },
                "--font" => {
                    cfg.family = match val("--font")?.to_lowercase().as_str() {
                        "sans" => Family::Sans,
                        "serif" => Family::Serif,
                        f => return Err(format!("unknown font '{f}'")),
                    }
                }
                "--size" => cfg.base_size = val("--size")?.parse().map_err(|_| "bad --size")?,
                "--margin" => margin = Some(val("--margin")?.parse().map_err(|_| "bad --margin")?),
                "--no-page-numbers" => cfg.page_numbers = false,
                s if s.starts_with('-') && s != "-" => return Err(format!("unknown option '{s}'")),
                _ => input = Some(a.clone()),
            }
            Ok(())
        })();
        if let Err(e) = r {
            return fail(&e);
        }
    }
    if let Some(m) = margin {
        cfg.margin_top = m;
        cfg.margin_right = m;
        cfg.margin_bottom = m;
        cfg.margin_left = m;
    }
    let Some(input) = input else { return fail("no input file") };

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

    let pdf = markdown_to_pdf(&src, &cfg);
    let out = output.unwrap_or_else(|| {
        if input == "-" {
            "-".into()
        } else {
            std::path::Path::new(&input).with_extension("pdf").to_string_lossy().into_owned()
        }
    });
    let res = if out == "-" {
        std::io::stdout().write_all(&pdf)
    } else {
        std::fs::write(&out, &pdf)
    };
    if let Err(e) = res {
        eprintln!("mdpdf: cannot write {out}: {e}");
        return ExitCode::FAILURE;
    }
    if out != "-" {
        eprintln!("wrote {out} ({} bytes)", pdf.len());
    }
    ExitCode::SUCCESS
}
