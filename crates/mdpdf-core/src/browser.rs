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

/// Prints a complete HTML page to PDF bytes.
pub fn print_pdf(html: &str, cfg: &Config) -> Result<Vec<u8>, String> {
    let browser = find_browser(cfg.browser_path.as_deref()).ok_or(NOT_FOUND)?;
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let dir = TempDir(std::env::temp_dir().join(format!("mdpdf-{}-{nanos}", std::process::id())));
    std::fs::create_dir_all(&dir.0).map_err(|e| format!("cannot create temp dir: {e}"))?;
    let (page, out, log) = (dir.0.join("doc.html"), dir.0.join("out.pdf"), dir.0.join("browser.log"));
    std::fs::write(&page, html).map_err(|e| format!("cannot write temp file: {e}"))?;

    let mut cmd = Command::new(&browser);
    let shell = browser.to_string_lossy().contains("headless-shell");
    if !shell {
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
        "--no-pdf-header-footer",
        "--print-to-pdf-no-header",
        "--generate-pdf-document-outline",
        "--run-all-compositor-stages-before-draw",
    ]);
    if running_as_root() {
        cmd.arg("--no-sandbox"); // Chromium refuses to start as root otherwise
    }
    if let Ok(extra) = std::env::var("MDPDF_BROWSER_ARGS") {
        cmd.args(extra.split_whitespace());
    }
    cmd.arg(format!("--user-data-dir={}", dir.0.join("profile").display()))
        .arg(format!("--print-to-pdf={}", out.display()))
        .arg(dir_url(&dir.0).trim_end_matches('/').to_string() + "/doc.html")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(std::fs::File::create(&log).map_or_else(|_| Stdio::null(), Stdio::from));
    let mut child = cmd.spawn().map_err(|e| format!("cannot start {}: {e}", browser.display()))?;

    let timeout = std::env::var("MDPDF_BROWSER_TIMEOUT").ok().and_then(|s| s.parse().ok()).unwrap_or(90);
    let deadline = Instant::now() + Duration::from_secs(timeout);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {}
            Err(e) => return Err(format!("waiting for browser failed: {e}")),
        }
        // Some browser builds finish the PDF but linger; accept a complete file.
        if is_ready(&out) {
            std::thread::sleep(Duration::from_millis(150));
            let _ = child.kill();
            let _ = child.wait();
            break;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!(
                "{} did not produce a PDF within {timeout}s (is another instance blocking it? set MDPDF_BROWSER_TIMEOUT to wait longer)",
                browser_label(&browser)
            ));
        }
        std::thread::sleep(Duration::from_millis(40));
    }
    match std::fs::read(&out) {
        Ok(b) if b.starts_with(b"%PDF") => Ok(b),
        _ => {
            let log = std::fs::read_to_string(&log).unwrap_or_default();
            let tail: Vec<&str> = log.lines().filter(|l| !l.contains("dbus") && !l.trim().is_empty()).collect();
            let tail = tail[tail.len().saturating_sub(4)..].join("\n");
            Err(format!("{} failed to print the document.\n{tail}", browser_label(&browser)))
        }
    }
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
