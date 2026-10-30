//! Drives a headless Chromium-family browser (Chrome, Edge, Chromium, Brave,
//! chrome-headless-shell) to print HTML to PDF. Standard library only: the
//! browser is an external tool the user already has, never bundled.

use crate::layout::Config;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const PATH_NAMES: &[&str] = &[
    "google-chrome",
    "google-chrome-stable",
    "chromium",
    "chromium-browser",
    "microsoft-edge",
    "microsoft-edge-stable",
    "brave-browser",
    "chrome",
    "chrome-headless-shell",
];

#[cfg(target_os = "macos")]
const KNOWN_PATHS: &[&str] = &[
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    "/Applications/Chromium.app/Contents/MacOS/Chromium",
    "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
    "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser",
];
#[cfg(target_os = "windows")]
const KNOWN_PATHS: &[&str] = &[
    r"C:\Program Files\Google\Chrome\Application\chrome.exe",
    r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
    r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
    r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
    r"C:\Program Files\BraveSoftware\Brave-Browser\Application\brave.exe",
];
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
const KNOWN_PATHS: &[&str] = &["/usr/bin/google-chrome", "/usr/bin/chromium", "/snap/bin/chromium", "/usr/bin/microsoft-edge"];

pub const NOT_FOUND: &str = "Designed mode renders HTML with a Chromium-based browser (Chrome, Edge, Chromium or Brave), \
but none was found. Install one, set MDPDF_BROWSER=/path/to/browser (or pass --browser), \
or use --mode designed-builtin for the built-in offline engine.";

fn on_path(name: &str) -> Option<PathBuf> {
    let exts: &[&str] = if cfg!(windows) { &[".exe", ".cmd", ""] } else { &[""] };
    std::env::split_paths(&std::env::var_os("PATH")?).find_map(|dir| {
        exts.iter().map(|e| dir.join(format!("{name}{e}"))).find(|p| p.is_file())
    })
}

/// Finds a browser: explicit path, `MDPDF_BROWSER`, `PATH`, then well-known install locations.
pub fn find_browser(explicit: Option<&Path>) -> Option<PathBuf> {
    if let Some(p) = explicit {
        return Some(p.to_path_buf());
    }
    if let Some(v) = std::env::var_os("MDPDF_BROWSER") {
        let p = PathBuf::from(&v);
        return if p.is_file() { Some(p) } else { on_path(&v.to_string_lossy()) };
    }
    PATH_NAMES
        .iter()
        .find_map(|n| on_path(n))
        .or_else(|| KNOWN_PATHS.iter().map(PathBuf::from).find(|p| p.is_file()))
}

/// Friendly name for status messages.
pub fn browser_label(p: &Path) -> String {
    p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "browser".into())
}

/// `file://` URL for a directory, with a trailing slash (for `<base href>`).
pub fn dir_url(dir: &Path) -> String {
    let mut s = dir.to_string_lossy().replace('\\', "/");
    if let Some(r) = s.strip_prefix("//?/") {
        s = r.to_string(); // Windows verbatim prefix from canonicalize()
    }
    let mut out = String::from("file://");
    if !s.starts_with('/') {
        out.push('/');
    }
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' | b':' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    if !out.ends_with('/') {
        out.push('/');
    }
    out
}

struct TempDir(PathBuf);
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn temp_dir() -> Result<TempDir, String> {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let dir = TempDir(std::env::temp_dir().join(format!("mdpdf-{}-{nanos}", std::process::id())));
    std::fs::create_dir_all(&dir.0).map_err(|e| format!("cannot create temp dir: {e}"))?;
    Ok(dir)
}

#[cfg(target_os = "linux")]
fn running_as_root() -> bool {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| s.lines().find(|l| l.starts_with("Uid:")).map(|l| l.split_whitespace().nth(1) == Some("0")))
        .unwrap_or(false)
}
#[cfg(not(target_os = "linux"))]
fn running_as_root() -> bool {
    false
}

fn is_ready(pdf: &Path) -> bool {
    std::fs::read(pdf).is_ok_and(|b| b.starts_with(b"%PDF") && b.windows(5).rev().take(64).any(|w| w == b"%%EOF"))
}

fn timeout_secs() -> u64 {
    std::env::var("MDPDF_BROWSER_TIMEOUT").ok().and_then(|s| s.parse().ok()).unwrap_or(90)
}

/// Runs the browser on `dir/doc.html` with `extra` flags and returns once it
/// exits, or once `ready()` says its output is complete (some builds linger).
/// The caller's temp dir holds the profile, the log and all outputs.
fn run(browser: &Path, dir: &Path, extra: &[String], stdout: Option<&Path>, ready: &dyn Fn() -> bool) -> Result<(), String> {
    let mut cmd = Command::new(browser);
    if !browser.to_string_lossy().contains("headless-shell") {
        cmd.arg("--headless=new");
    }
    cmd.args([
        "--disable-gpu",
        "--disable-dev-shm-usage",
        "--no-first-run",
        "--no-default-browser-check",
        "--disable-extensions",
        "--hide-scrollbars",
        "--allow-file-access-from-files",
    ]);
    if running_as_root() {
        cmd.arg("--no-sandbox"); // Chromium refuses to start as root otherwise
    }
    if let Ok(more) = std::env::var("MDPDF_BROWSER_ARGS") {
        cmd.args(more.split_whitespace());
    }
    cmd.args(extra)
        .arg(format!("--user-data-dir={}", dir.join("profile").display()))
        .arg(dir_url(dir).trim_end_matches('/').to_string() + "/doc.html")
        .stdin(Stdio::null())
        .stdout(stdout.and_then(|p| std::fs::File::create(p).ok()).map_or_else(Stdio::null, Stdio::from))
        .stderr(std::fs::File::create(dir.join("browser.log")).map_or_else(|_| Stdio::null(), Stdio::from));
    let mut child = cmd.spawn().map_err(|e| format!("cannot start {}: {e}", browser.display()))?;
    let secs = timeout_secs();
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return Ok(()),
            Ok(None) => {}
            Err(e) => return Err(format!("waiting for browser failed: {e}")),
        }
        if ready() {
            std::thread::sleep(Duration::from_millis(150));
            let _ = child.kill();
            let _ = child.wait();
            return Ok(());
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!(
                "{} did not finish within {secs}s (is another instance blocking it? set MDPDF_BROWSER_TIMEOUT to wait longer)",
                browser_label(browser)
            ));
        }
        std::thread::sleep(Duration::from_millis(40));
    }
}

/// Prints a complete HTML page to PDF bytes.
pub fn print_pdf(html: &str, cfg: &Config) -> Result<Vec<u8>, String> {
    let browser = find_browser(cfg.browser_path.as_deref()).ok_or(NOT_FOUND)?;
    let dir = temp_dir()?;
    let out = dir.0.join("out.pdf");
    std::fs::write(dir.0.join("doc.html"), html).map_err(|e| format!("cannot write temp file: {e}"))?;
    let extra = vec![
        "--no-pdf-header-footer".to_string(),
        "--print-to-pdf-no-header".to_string(),
        "--generate-pdf-document-outline".to_string(),
        "--run-all-compositor-stages-before-draw".to_string(),
        format!("--print-to-pdf={}", out.display()),
    ];
    run(&browser, &dir.0, &extra, None, &|| is_ready(&out))?;
    match std::fs::read(&out) {
        Ok(b) if b.starts_with(b"%PDF") => Ok(b),
        _ => {
            let log = std::fs::read_to_string(dir.0.join("browser.log")).unwrap_or_default();
            let tail: Vec<&str> = log.lines().filter(|l| !l.contains("dbus") && !l.trim().is_empty()).collect();
            let tail = tail[tail.len().saturating_sub(4)..].join("\n");
            Err(format!("{} failed to print the document.\n{tail}", browser_label(&browser)))
        }
    }
}

/// Lays the page out in the browser (screen media, `width_px` wide) and returns
/// the indices (document order) of the tables taller than `max_height_px`.
/// `html` must contain the measuring script allowed by its CSP nonce.
/// Returns `None` if the browser or script did not report; callers then fall back to estimates.
pub fn measure_tall_tables(html: &str, cfg: &Config, width_px: f32) -> Option<Vec<usize>> {
    let browser = find_browser(cfg.browser_path.as_deref())?;
    let dir = temp_dir().ok()?;
    std::fs::write(dir.0.join("doc.html"), html).ok()?;
    let dump = dir.0.join("dom.html");
    let extra = vec![
        "--dump-dom".to_string(),
        format!("--window-size={},2400", width_px.round() as i64),
        "--virtual-time-budget=4000".to_string(),
    ];
    let done = || std::fs::read_to_string(&dump).is_ok_and(|d| d.contains("</html>"));
    run(&browser, &dir.0, &extra, Some(&dump), &done).ok()?;
    let dom = std::fs::read_to_string(&dump).ok()?;
    let at = dom.find("data-mdpdf-tall=\"")? + "data-mdpdf-tall=\"".len();
    let list = &dom[at..at + dom[at..].find('"')?];
    Some(list.split(',').filter_map(|n| n.trim().parse().ok()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dir_urls_are_escaped() {
        assert_eq!(dir_url(Path::new("/home/me/my docs")), "file:///home/me/my%20docs/");
        assert_eq!(dir_url(Path::new(r"C:\Users\me")), "file:///C:/Users/me/");
        assert_eq!(dir_url(Path::new(r"\\?\C:\x y")), "file:///C:/x%20y/");
    }

    #[test]
    fn missing_browser_gives_actionable_error() {
        let mut cfg = Config::designed_a4();
        cfg.browser_path = Some(PathBuf::from("/definitely/not/a/browser"));
        let err = print_pdf("<p>x</p>", &cfg).unwrap_err();
        assert!(err.contains("cannot start"), "{err}");
    }
}
