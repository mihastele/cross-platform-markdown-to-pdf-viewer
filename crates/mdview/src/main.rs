//! mdview: a native Markdown viewer. It paints the `mdpdf-core` layout directly
//! (no HTML engine, no web view) and exports the same layout to PDF.

mod fonts;

use eframe::egui::{self, Color32, FontId, Key, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};
use mdpdf_core::image::Pixels;
use mdpdf_core::{convert, layout, parse, render, Config, Family, Layout, Mode, Op, Source, Theme};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

const WELCOME: &str = "# mdview\n\nOpen a Markdown file with **Ctrl+O**, drop one onto this window, or start with `mdview file.md`.\n\n- The view reloads automatically when the file changes\n- **Ctrl+E** exports a PDF straight from the same layout\n- **Ctrl+D** toggles dark mode, **Ctrl +/-/0** zoom\n";

struct App {
    path: Option<PathBuf>,
    src: String,
    mtime: Option<SystemTime>,
    kind: Source,
    mode: Mode,
    textures: HashMap<u64, Option<egui::TextureHandle>>,
    rev: u64,
    cache: Option<((i32, bool, bool, u64, Mode), Layout)>,
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
    fn new(cc: &eframe::CreationContext<'_>, path: Option<PathBuf>, mode: Mode) -> Self {
        let fonts = fonts::install(&cc.egui_ctx);
        cc.egui_ctx.options_mut(|o| o.zoom_with_keyboard = false);
        let mut app = App {
            path: None,
            src: WELCOME.to_string(),
            mtime: None,
            kind: Source::Markdown,
            mode,
            textures: HashMap::new(),
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
                self.kind = if matches!(path.extension().and_then(|e| e.to_str()), Some("html" | "htm" | "xhtml")) {
                    Source::Html
                } else {
                    Source::Markdown
                };
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
        if let Some(p) = rfd::FileDialog::new().add_filter("Markdown / HTML", &["md", "markdown", "txt", "html", "htm"]).pick_file() {
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
        let mut cfg = self.mode.config(false);
        cfg.family = if self.serif { Family::Serif } else { Family::Sans };
        cfg.base_dir = self.base_dir();
        self.status = match self.mode {
            Mode::Designed => format!("Rendering with {}…", browser_name()),
            _ => "Rendering…".into(),
        };
        let pdf = match convert(&self.src, self.kind, &cfg) {
            Ok(p) => p,
            Err(e) => {
                self.status = format!("Export failed: {e}");
                return;
            }
        };
        self.status = match std::fs::write(&out, &pdf) {
            Ok(()) => format!("Exported {} ({} KB)", out.display(), pdf.len() / 1024 + 1),
            Err(e) => format!("Export failed: {e}"),
        };
    }

    fn base_dir(&self) -> Option<PathBuf> {
        self.path.as_deref().and_then(Path::parent).map(|p| if p.as_os_str().is_empty() { PathBuf::from(".") } else { p.to_path_buf() })
    }

    fn config(&self, width: f32) -> Config {
        // The preview always uses the built-in engine; the browser only prints PDFs.
        let mut c = match self.mode {
            Mode::Standard => Config::a4(),
            Mode::Designed | Mode::DesignedBuiltin => Config::designed_builtin_a4(),
        };
        c.paginate = false;
        c.page_numbers = false;
        c.header = false;
        c.page_width = width;
        c.margin_left = 32.0;
        c.margin_right = 32.0;
        c.margin_top = 28.0;
        c.margin_bottom = 56.0;
        c.base_size = if self.mode == Mode::Standard { 13.0 } else { 12.5 };
        c.family = if self.serif { Family::Serif } else { Family::Sans };
        c.theme = match (self.mode, self.dark) {
            (Mode::Standard, false) => Theme::light(),
            (Mode::Standard, true) => Theme::dark(),
            (_, false) => Theme::designed(),
            (_, true) => Theme::designed_dark(),
        };
        c.base_dir = self.base_dir();
        c
    }

    /// Lays out the current source; failures (e.g. HTML without opting in) become a notice page.
    fn build_layout(&self, width: f32) -> Layout {
        let cfg = self.config(width);
        render(&self.src, self.kind, &cfg).unwrap_or_else(|e| {
            let msg = format!("# Cannot render this file\n\n{e}.\n\nChoose a **Designed** entry in the *Mode* dropdown in the toolbar to opt in.");
            layout(&parse(&msg), &cfg)
        })
    }

    fn texture(&mut self, ctx: &egui::Context, lay: &Layout, idx: usize) -> Option<egui::TextureId> {
        let data = &lay.images[idx];
        let entry = self.textures.entry(data.uid).or_insert_with(|| {
            let (w, h) = (data.width as usize, data.height as usize);
            let color = match &data.pixels {
                Pixels::Raw { rgb, alpha } => {
                    let mut rgba = Vec::with_capacity(w * h * 4);
                    for i in 0..w * h {
                        rgba.extend_from_slice(&rgb[i * 3..i * 3 + 3]);
                        rgba.push(alpha.as_ref().map_or(255, |a| a[i]));
                    }
                    Some(egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba))
                }
                Pixels::Jpeg { data, .. } => image::load_from_memory_with_format(data, image::ImageFormat::Jpeg)
                    .ok()
                    .map(|im| {
                        let im = im.to_rgba8();
                        egui::ColorImage::from_rgba_unmultiplied([im.width() as usize, im.height() as usize], im.as_raw())
                    }),
            };
            color.map(|c| ctx.load_texture(format!("img{}", data.uid), c, egui::TextureOptions::LINEAR))
        });
        entry.as_ref().map(|t| t.id())
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

fn browser_name() -> String {
    mdpdf_core::browser::find_browser(None).map_or_else(|| "no browser found".to_string(), |p| mdpdf_core::browser::browser_label(&p))
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
                ui.label("Mode");
                egui::ComboBox::from_id_salt("mode")
                    .selected_text(self.mode.label())
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.mode, Mode::Standard, Mode::Standard.label());
                        ui.selectable_value(&mut self.mode, Mode::Designed, Mode::Designed.label());
                        ui.selectable_value(&mut self.mode, Mode::DesignedBuiltin, Mode::DesignedBuiltin.label());
                    })
                    .response
                    .on_hover_text(
                        "Designed (opt-in): full HTML/CSS through an installed Chromium-based browser, with page breaks that never cut headings, tables, images or code.\nDesigned (built-in): the same look from our own engine; works offline with a smaller HTML/CSS subset.",
                    );
                if self.mode == Mode::Designed {
                    ui.label(egui::RichText::new(format!("PDF via {}; preview approximate", browser_name())).small().weak());
                }
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

        let theme = self.config(800.0).theme;
        let frame = egui::Frame::NONE.fill(col(theme.page_bg));
        let mut clicked_link: Option<String> = None;
        egui::CentralPanel::default().frame(frame).show(ui, |ui| {
            let avail = ui.available_width();
            let doc_w = (avail / self.zoom).clamp(320.0, 880.0);
            let key = (doc_w.round() as i32, self.dark, self.serif, self.rev, self.mode);
            if self.cache.as_ref().map(|c| c.0) != Some(key) {
                let lay = self.build_layout(doc_w);
                self.cache = Some((key, lay));
            }
            let (ckey, lay_owned) = self.cache.take().unwrap();
            let lay = &lay_owned;
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
                let ctx2 = ui.ctx().clone();
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
                            Op::Image { x, y, w, h, img } => {
                                let r = Rect::from_min_size(pt(*x, *y), Vec2::new(w * z, h * z));
                                if !r.intersects(vis) { continue; }
                                match self.texture(&ctx2, lay, *img) {
                                    Some(id) => {
                                        painter.image(id, r, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
                                    }
                                    None => {
                                        painter.rect_stroke(r, 0.0, Stroke::new(1.0, Color32::GRAY), StrokeKind::Middle);
                                    }
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
            self.cache = Some((ckey, lay_owned));
        });
        if let Some(u) = clicked_link {
            self.follow_link(&ctx, &u);
        }
    }
}

fn main() -> eframe::Result {
    let mut path = None;
    let mut mode = Mode::Standard;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--mode" => mode = if args.next().as_deref() == Some("designed") { Mode::Designed } else { Mode::Standard },
            "--designed" => mode = Mode::Designed,
            _ => path = Some(PathBuf::from(a)),
        }
    }
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1100.0, 800.0]).with_title("mdview").with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native("mdview", opts, Box::new(move |cc| Ok(Box::new(App::new(cc, path, mode)))))
}
