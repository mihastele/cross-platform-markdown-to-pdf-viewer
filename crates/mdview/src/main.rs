//! mdview: a native Markdown viewer. It paints the `mdpdf-core` layout directly
//! (no HTML engine, no web view) and exports the same layout to PDF.

mod fonts;

use eframe::egui::{self, Color32, FontId, Key, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};
use mdpdf_core::{layout, markdown_to_pdf, parse, Config, Document, Family, Layout, Op, Theme};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

const WELCOME: &str = "# mdview\n\nOpen a Markdown file with **Ctrl+O**, drop one onto this window, or start with `mdview file.md`.\n\n- The view reloads automatically when the file changes\n- **Ctrl+E** exports a PDF straight from the same layout\n- **Ctrl+D** toggles dark mode, **Ctrl +/-/0** zoom\n";

struct App {
    path: Option<PathBuf>,
    src: String,
    mtime: Option<SystemTime>,
    doc: Document,
    rev: u64,
    cache: Option<((i32, bool, bool, u64), Layout)>,
    zoom: f32,
    dark: bool,
    serif: bool,
    outline: bool,
    fonts: fonts::FontSet,
    scroll_to: Option<f32>,
    status: String,
    last_poll: Instant,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>, path: Option<PathBuf>) -> Self {
        let fonts = fonts::install(&cc.egui_ctx);
        cc.egui_ctx.options_mut(|o| o.zoom_with_keyboard = false);
        let mut app = App {
            path: None,
            src: WELCOME.to_string(),
            mtime: None,
            doc: parse(WELCOME),
            rev: 0,
            cache: None,
            zoom: 1.0,
            dark: cc.egui_ctx.global_style().visuals.dark_mode,
            serif: false,
            outline: true,
            fonts,
            scroll_to: None,
            status: String::new(),
            last_poll: Instant::now(),
        };
        if let Some(p) = path {
            app.load(p);
        }
        app
    }

    fn load(&mut self, path: PathBuf) {
        match std::fs::read_to_string(&path) {
            Ok(s) => {
                self.mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
                self.doc = parse(&s);
                self.src = s;
                self.rev += 1;
                self.status = format!("Loaded {}", path.display());
                self.path = Some(path);
            }
            Err(e) => self.status = format!("Cannot read {}: {e}", path.display()),
        }
    }

    fn reload_if_changed(&mut self) {
        if self.last_poll.elapsed() < Duration::from_millis(400) {
            return;
        }
        self.last_poll = Instant::now();
        let Some(p) = self.path.clone() else { return };
        let m = std::fs::metadata(&p).and_then(|m| m.modified()).ok();
        if m.is_some() && m != self.mtime {
            self.load(p);
            self.status = "Reloaded (file changed)".into();
        }
    }

    fn open_dialog(&mut self) {
        if let Some(p) = rfd::FileDialog::new().add_filter("Markdown", &["md", "markdown", "txt"]).pick_file() {
            self.load(p);
            self.scroll_to = Some(0.0);
        }
    }

    fn export_pdf(&mut self) {
        let default = self
            .path
            .as_ref()
            .and_then(|p| p.with_extension("pdf").file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_else(|| "document.pdf".into());
        let Some(out) = rfd::FileDialog::new().add_filter("PDF", &["pdf"]).set_file_name(default).save_file() else {
            return;
        };
        let mut cfg = Config::a4();
        cfg.family = if self.serif { Family::Serif } else { Family::Sans };
        let pdf = markdown_to_pdf(&self.src, &cfg);
        self.status = match std::fs::write(&out, &pdf) {
            Ok(()) => format!("Exported {} ({} KB)", out.display(), pdf.len() / 1024 + 1),
            Err(e) => format!("Export failed: {e}"),
        };
    }

    fn config(&self, width: f32) -> Config {
        let mut c = Config::a4();
        c.paginate = false;
        c.page_numbers = false;
        c.page_width = width;
        c.margin_left = 32.0;
        c.margin_right = 32.0;
        c.margin_top = 28.0;
        c.margin_bottom = 56.0;
        c.base_size = 13.0;
        c.family = if self.serif { Family::Serif } else { Family::Sans };
        c.theme = if self.dark { Theme::dark() } else { Theme::light() };
        c
    }

    fn follow_link(&mut self, ctx: &egui::Context, url: &str) {
        if let Some(anchor) = url.strip_prefix('#') {
            let want = slug(anchor);
            let y = self.cache.as_ref().and_then(|(_, l)| l.headings.iter().find(|h| slug(&h.title) == want).map(|h| h.y));
            if let Some(y) = y {
                self.scroll_to = Some(y * self.zoom - 12.0);
            }
            return;
        }
        if url.contains("://") || url.starts_with("mailto:") {
            ctx.open_url(egui::OpenUrl::new_tab(url));
            return;
        }
        let rel = url.split('#').next().unwrap_or(url);
        let base = self.path.as_deref().and_then(Path::parent).unwrap_or(Path::new("."));
        let target = base.join(rel);
        if matches!(target.extension().and_then(|e| e.to_str()), Some("md" | "markdown")) && target.exists() {
            self.load(target);
            self.scroll_to = Some(0.0);
        } else {
            self.status = format!("Not opening {url}");
        }
    }
}

fn slug(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .filter_map(|c| match c {
            ' ' => Some('-'),
            c if c.is_alphanumeric() || c == '-' || c == '_' => Some(c),
            _ => None,
        })
        .collect()
}

fn col(c: mdpdf_core::Color) -> Color32 {
    Color32::from_rgb(c.r, c.g, c.b)
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.reload_if_changed();
        ctx.request_repaint_after(Duration::from_millis(400));

        // Drag and drop, shortcuts.
        if let Some(f) = ctx.input(|i| i.raw.dropped_files.first().map(|f| f.path().to_path_buf())) {
            self.load(f);
            self.scroll_to = Some(0.0);
        }
        let (open, export, dark, zin, zout, zreset, wheel_zoom) = ctx.input(|i| {
            let c = i.modifiers.command;
            (
                c && i.key_pressed(Key::O),
                c && i.key_pressed(Key::E),
                c && i.key_pressed(Key::D),
                c && (i.key_pressed(Key::Equals) || i.key_pressed(Key::Plus)),
                c && i.key_pressed(Key::Minus),
                c && i.key_pressed(Key::Num0),
                i.zoom_delta(),
            )
        });
        if open { self.open_dialog(); }
        if export { self.export_pdf(); }
        if dark { self.dark = !self.dark; }
        if zin { self.zoom *= 1.1; }
        if zout { self.zoom /= 1.1; }
        if zreset { self.zoom = 1.0; }
        self.zoom = (self.zoom * wheel_zoom).clamp(0.4, 4.0);

        ctx.set_visuals(if self.dark { egui::Visuals::dark() } else { egui::Visuals::light() });

        egui::Panel::top("bar").show(ui, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Open…").clicked() { self.open_dialog(); }
                if ui.button("Export PDF…").clicked() { self.export_pdf(); }
                ui.separator();
                if ui.button("−").clicked() { self.zoom /= 1.1; }
                ui.label(format!("{:.0}%", self.zoom * 100.0));
                if ui.button("+").clicked() { self.zoom *= 1.1; }
                ui.separator();
                ui.toggle_value(&mut self.dark, "Dark");
                ui.toggle_value(&mut self.serif, "Serif");
                ui.toggle_value(&mut self.outline, "Outline");
            });
        });
        egui::Panel::bottom("status").show(ui, |ui| {
            let name = self.path.as_ref().map_or("(welcome)".to_string(), |p| p.display().to_string());
            ui.label(egui::RichText::new(format!("{name}   {}", self.status)).small());
        });

        let mut jump: Option<f32> = None;
        if self.outline {
            egui::Panel::left("outline").default_size(210.0).show(ui, |ui| {
                ui.strong("Outline");
                egui::ScrollArea::vertical().show(ui, |ui| {
                    if let Some((_, l)) = &self.cache {
                        for h in &l.headings {
                            ui.horizontal(|ui| {
                                ui.add_space((h.level.saturating_sub(1)) as f32 * 10.0);
                                if ui.selectable_label(false, &h.title).clicked() {
                                    jump = Some(h.y * self.zoom - 12.0);
                                }
                            });
                        }
                    }
                });
            });
        }
        if jump.is_some() { self.scroll_to = jump; }

        let theme = if self.dark { Theme::dark() } else { Theme::light() };
        let frame = egui::Frame::NONE.fill(col(theme.page_bg));
        let mut clicked_link: Option<String> = None;
        egui::CentralPanel::default().frame(frame).show(ui, |ui| {
            let avail = ui.available_width();
            let doc_w = (avail / self.zoom).clamp(320.0, 880.0);
            let key = (doc_w.round() as i32, self.dark, self.serif, self.rev);
            if self.cache.as_ref().map(|c| c.0) != Some(key) {
                let lay = layout(&self.doc, &self.config(doc_w));
                self.cache = Some((key, lay));
            }
            let lay = &self.cache.as_ref().unwrap().1;
            let z = self.zoom;
            let mut sa = egui::ScrollArea::vertical().auto_shrink([false, false]);
            if let Some(y) = self.scroll_to.take() {
                sa = sa.vertical_scroll_offset(y.max(0.0));
            }
            sa.show(ui, |ui| {
                let size = Vec2::new(avail, (lay.height * z).max(ui.available_height()));
                let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
                let painter = ui.painter_at(rect);
                let ox = rect.min.x + ((rect.width() - lay.width * z) / 2.0).max(0.0);
                let oy = rect.min.y;
                let vis = ui.clip_rect();
                let pt = |x: f32, y: f32| Pos2::new(ox + x * z, oy + y * z);
                let pointer = resp.hover_pos();
                for page in &lay.pages {
                    for op in &page.ops {
                        match op {
                            Op::Text { x, y, face, size, color, text } => {
                                let p = pt(*x, *y);
                                if p.y < vis.min.y - 80.0 || p.y > vis.max.y + 80.0 { continue; }
                                let font = FontId::new(size * z, self.fonts.family(*face));
                                let galley = painter.layout_no_wrap(text.clone(), font, col(*color));
                                // Place by baseline so text sits where the layout put it.
                                let base = galley.rows.first().map_or(0.0, |r| {
                                    r.pos.y + r.row.glyphs.first().map_or(0.0, |g| g.pos.y)
                                });
                                let at = Pos2::new(p.x, p.y - base);
                                if self.fonts.faux_bold(*face) {
                                    painter.galley(at + Vec2::new(0.45 * z, 0.0), galley.clone(), col(*color));
                                }
                                painter.galley(at, galley, col(*color));
                            }
                            Op::Rect { x, y, w, h, fill, stroke } => {
                                let r = Rect::from_min_size(pt(*x, *y), Vec2::new(w * z, h * z));
                                if !r.intersects(vis) { continue; }
                                if let Some(f) = fill { painter.rect_filled(r, 0.0, col(*f)); }
                                if let Some((c, lw)) = stroke {
                                    painter.rect_stroke(r, 0.0, Stroke::new(lw * z, col(*c)), StrokeKind::Middle);
                                }
                            }
                            Op::Line { x1, y1, x2, y2, color, width } => {
                                let (a, b) = (pt(*x1, *y1), pt(*x2, *y2));
                                if a.y < vis.min.y - 20.0 || a.y > vis.max.y + 20.0 { continue; }
                                painter.line_segment([a, b], Stroke::new((width * z).max(0.6), col(*color)));
                            }
                            Op::Circle { cx, cy, r, fill, color, width } => {
                                let c = pt(*cx, *cy);
                                if c.y < vis.min.y - 20.0 || c.y > vis.max.y + 20.0 { continue; }
                                if *fill {
                                    painter.circle_filled(c, r * z, col(*color));
                                } else {
                                    painter.circle_stroke(c, r * z, Stroke::new(width * z, col(*color)));
                                }
                            }
                            Op::Link { x, y, w, h, url } => {
                                let r = Rect::from_min_size(pt(*x, *y), Vec2::new(w * z, h * z));
                                if pointer.is_some_and(|p| r.contains(p)) {
                                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                    if resp.clicked() { clicked_link = Some(url.clone()); }
                                    resp.clone().on_hover_text(url);
                                }
                            }
                        }
                    }
                }
            });
        });
        if let Some(u) = clicked_link {
            self.follow_link(&ctx, &u);
        }
    }
}

fn main() -> eframe::Result {
    let path = std::env::args().nth(1).map(PathBuf::from);
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1100.0, 800.0]).with_title("mdview").with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native("mdview", opts, Box::new(move |cc| Ok(Box::new(App::new(cc, path)))))
}
