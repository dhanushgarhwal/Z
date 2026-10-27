use eframe::egui::{self, vec2, Align, Align2, Color32, FontFamily, FontId, Layout, Pos2, Rect, RichText, Sense, Shape, Stroke, TextStyle};
use egui_extras::{Column, TableBuilder};
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};
use z::jobs::{self, Dest, Job};
use zcore::{fsutil, CreateOpts, Entry, Format, Level, Overwrite, Progress};

/// Logo violet (icons, folder glyphs, focus rings).
pub const ACCENT: Color32 = Color32::from_rgb(108, 71, 255);
/// Deeper violet for filled controls, so light text on them keeps a readable contrast.
const ACCENT_DEEP: Color32 = Color32::from_rgb(76, 48, 206);
/// #c1c5ce: the one text colour used across the whole interface.
const TXT: Color32 = Color32::from_rgb(0xc1, 0xc5, 0xce);
/// Background gradient, top-left to bottom-right (same stops as the installer artwork).
const GRADIENT: [Color32; 3] = [Color32::from_rgb(44, 28, 112), Color32::from_rgb(28, 20, 72), Color32::from_rgb(14, 11, 34)];
const MENU_BG: Color32 = Color32::from_rgb(24, 19, 58);

/// #c1c5ce at a given opacity (hairlines, hover washes, field fills).
fn tint(a: u8) -> Color32 { Color32::from_rgba_unmultiplied(0xc1, 0xc5, 0xce, a) }

pub enum Startup { Empty, Open(PathBuf), Job(Job), Error(String) }

#[derive(Clone)]
struct Row { name: String, key: String, full: String, is_dir: bool, size: u64, packed: u64, modified: i64 }

#[derive(Clone, Copy, PartialEq, Eq)]
enum Sort { Name, Size, Packed, Modified }

struct Opened { path: PathBuf, fmt: Format, entries: Vec<Entry>, disk_size: u64, password: Option<String> }

struct Loader { path: PathBuf, password: Option<String>, rx: mpsc::Receiver<Result<(Format, Vec<Entry>), String>> }

struct Running { title: String, progress: Arc<Progress>, rx: mpsc::Receiver<Result<String, String>>, job: Job, open_after: Option<PathBuf>, started: Instant }

enum Outcome { Done(String, Option<PathBuf>), Failed(String, Job) }

struct AddDialog { inputs: Vec<PathBuf>, out: String, fmt: Format, level: Level, pw: String, pw2: String }

const WRITE_FORMATS: [Format; 11] = [Format::Zip, Format::SevenZ, Format::TarGz, Format::TarBz2, Format::TarXz, Format::TarZst, Format::Tar, Format::Gz, Format::Bz2, Format::Xz, Format::Zst];

pub struct ZApp {
    job_mode: bool,
    open: Option<Opened>,
    loader: Option<Loader>,
    cwd: String,
    rows: Vec<Row>,
    dirty: bool,
    sel: BTreeSet<String>,
    anchor: Option<usize>,
    filter: String,
    sort: (Sort, bool),
    running: Option<Running>,
    outcome: Option<Outcome>,
    retry_pw: String,
    add: Option<AddDialog>,
    open_pw: Option<(PathBuf, String)>,
    error: Option<String>,
    wide_fonts: bool,
    temp_root: PathBuf,
}

impl ZApp {
    pub fn new(cc: &eframe::CreationContext<'_>, startup: Startup) -> Self {
        install_fonts(&cc.egui_ctx, false);
        apply_theme(&cc.egui_ctx);
        let temp_root = std::env::temp_dir().join("Z");
        let _ = std::fs::remove_dir_all(&temp_root);                       // leftovers from older sessions
        let mut app = ZApp {
            job_mode: false, open: None, loader: None, cwd: String::new(), rows: vec![], dirty: false, sel: BTreeSet::new(),
            anchor: None, filter: String::new(), sort: (Sort::Name, true),
            running: None, outcome: None, retry_pw: String::new(),
            add: None, open_pw: None, error: None, wide_fonts: false, temp_root,
        };
        let ctx = cc.egui_ctx.clone();
        match startup {
            Startup::Empty => {}
            Startup::Error(e) => app.error = Some(e),
            Startup::Open(p) => app.load(&ctx, p, None),
            Startup::Job(job) => {
                app.job_mode = true;
                match job {
                    Job::Add { inputs, opts, dialog: true, .. } => app.add = Some(AddDialog::new(inputs, opts.format)),
                    Job::Extract { archives, dest: Dest::Ask, password, overwrite, only } => {
                        match rfd::FileDialog::new().set_title("Extract to").pick_folder() {
                            Some(d) => app.start(&ctx, "Extracting".into(), Job::Extract { archives, dest: Dest::Dir(d), password, overwrite, only }, None),
                            None => std::process::exit(0),
                        }
                    }
                    j => app.start(&ctx, "Working".into(), j, None),
                }
            }
        }
        app
    }

    // ------------------------------------------------------------ archive loading

    fn load(&mut self, ctx: &egui::Context, path: PathBuf, pw: Option<String>) {
        let (tx, rx) = mpsc::channel();
        let (p2, pw2, c2) = (path.clone(), pw.clone(), ctx.clone());
        std::thread::spawn(move || {
            let r = (|| -> anyhow::Result<(Format, Vec<Entry>)> {
                let fmt = Format::detect(&p2).ok_or_else(|| anyhow::anyhow!("Z cannot open this file: unrecognised archive format"))?;
                Ok((fmt, zcore::list::list_as(&p2, fmt, pw2.as_deref())?))
            })().map_err(|e| e.to_string());
            let _ = tx.send(r); c2.request_repaint();
        });
        self.loader = Some(Loader { path, password: pw, rx });
    }

    fn poll_loader(&mut self, ctx: &egui::Context) {
        let Some(l) = &self.loader else { return };
        let Ok(res) = l.rx.try_recv() else { return };
        let (path, password) = (l.path.clone(), l.password.clone());
        self.loader = None;
        match res {
            Ok((fmt, entries)) => {
                if !self.wide_fonts && entries.iter().any(|e| e.path.chars().any(|c| (c as u32) > 0x58F && !(0x2000..0x2070).contains(&(c as u32)))) {
                    self.wide_fonts = true; install_fonts(ctx, true);          // CJK/Arabic/Thai names: load system fallback fonts on demand
                }
                let disk_size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!("{} - Z", path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default())));
                self.open = Some(Opened { path, fmt, entries, disk_size, password });
                self.cwd.clear(); self.sel.clear(); self.anchor = None; self.filter.clear(); self.dirty = true;
            }
            Err(e) if e.to_lowercase().contains("password") => self.open_pw = Some((path, String::new())),
            Err(e) => self.error = Some(e),
        }
    }

    fn rebuild(&mut self) {
        self.dirty = false;
        let Some(o) = &self.open else { self.rows.clear(); return };
        let mk = |name: &str, full: String, is_dir: bool, size, packed, modified| Row { key: name.to_lowercase(), name: name.to_string(), full, is_dir, size, packed, modified };
        let mut rows: Vec<Row> = Vec::new();
        if !self.filter.is_empty() {
            let q = self.filter.to_lowercase();
            for e in o.entries.iter().filter(|e| !e.is_dir && e.path.to_lowercase().contains(&q)) {
                rows.push(mk(&e.path, e.path.clone(), false, e.size, e.packed, e.modified));
            }
        } else {
            let prefix = if self.cwd.is_empty() { String::new() } else { format!("{}/", self.cwd) };
            let mut dirs: HashMap<String, Row> = HashMap::new();
            for e in &o.entries {
                let Some(rest) = e.path.strip_prefix(prefix.as_str()) else { continue };
                if rest.is_empty() { continue; }
                match rest.split_once('/') {
                    Some((d, _)) => {
                        let r = dirs.entry(d.to_string()).or_insert_with(|| mk(d, format!("{prefix}{d}"), true, 0, 0, 0));
                        if !e.is_dir { r.size += e.size; r.packed += e.packed; }
                        r.modified = r.modified.max(e.modified);
                    }
                    None if e.is_dir => { dirs.entry(rest.to_string()).or_insert_with(|| mk(rest, format!("{prefix}{rest}"), true, 0, 0, e.modified)); }
                    None => rows.push(mk(rest, e.path.clone(), false, e.size, e.packed, e.modified)),
                }
            }
            rows.extend(dirs.into_values());
        }
        let (key, asc) = self.sort;
        rows.sort_by(|a, b| {
            b.is_dir.cmp(&a.is_dir).then_with(|| {
                let o = match key { Sort::Name => a.key.cmp(&b.key), Sort::Size => a.size.cmp(&b.size), Sort::Packed => a.packed.cmp(&b.packed), Sort::Modified => a.modified.cmp(&b.modified) };
                let o = o.then_with(|| a.key.cmp(&b.key));
                if asc { o } else { o.reverse() }
            })
        });
        self.rows = rows;
    }

    fn go(&mut self, dir: &str) { self.cwd = dir.to_string(); self.filter.clear(); self.sel.clear(); self.anchor = None; self.dirty = true; }
    fn up(&mut self) { if let Some((p, _)) = self.cwd.rsplit_once('/') { let p = p.to_string(); self.go(&p); } else { self.go(""); } }

    // ------------------------------------------------------------ jobs

    fn start(&mut self, ctx: &egui::Context, title: String, job: Job, open_after: Option<PathBuf>) {
        let progress = Arc::new(Progress::new());
        let (tx, rx) = mpsc::channel();
        let (p2, j2, c2) = (progress.clone(), job.clone(), ctx.clone());
        std::thread::spawn(move || { let r = jobs::run(&j2, &p2).map_err(|e| e.to_string()); let _ = tx.send(r); c2.request_repaint(); });
        self.running = Some(Running { title, progress, rx, job, open_after, started: Instant::now() });
    }

    fn poll_job(&mut self, ctx: &egui::Context) {
        let polled = self.running.as_ref().and_then(|r| r.rx.try_recv().ok().map(|res| (res, r.job.clone(), r.open_after.clone())));
        let Some((res, job, open_after)) = polled else { return };
        self.running = None;
        match res {
            Ok(_) if open_after.is_some() => { if let Some(p) = open_after { launch(&p); } }
            Ok(msg) => {
                if self.job_mode && !msg.contains("skipped") { ctx.send_viewport_cmd(egui::ViewportCommand::Close); }
                else { let reveal = if let Job::Extract { dest: Dest::Dir(d), .. } = &job { Some(d.clone()) } else { None }; self.outcome = Some(Outcome::Done(msg, reveal)); }
            }
            Err(e) if e == "Cancelled" => { if self.job_mode { ctx.send_viewport_cmd(egui::ViewportCommand::Close); } }
            Err(e) => self.outcome = Some(Outcome::Failed(e, job)),
        }
    }

    fn selected_or_all(&self) -> Vec<String> { self.sel.iter().cloned().collect() }

    fn extract_clicked(&mut self, ctx: &egui::Context) {
        let Some(o) = &self.open else { return };
        let Some(dir) = rfd::FileDialog::new().set_title("Extract to").set_directory(o.path.parent().unwrap_or(Path::new("."))).pick_folder() else { return };
        let job = Job::Extract { archives: vec![o.path.clone()], dest: Dest::Dir(dir), password: o.password.clone(), overwrite: Overwrite::Rename, only: self.selected_or_all() };
        self.start(ctx, "Extracting".into(), job, None);
    }

    fn open_entry(&mut self, ctx: &egui::Context, full: &str) {
        if self.running.is_some() { return; }
        let Some(o) = &self.open else { return };
        let dir = self.temp_root.join(format!("{:x}", fnv(&format!("{}|{full}", o.path.display()))));
        let Some(target) = fsutil::safe_join(&dir, full) else { return };
        let job = Job::Extract { archives: vec![o.path.clone()], dest: Dest::Dir(dir), password: o.password.clone(), overwrite: Overwrite::Always, only: vec![full.to_string()] };
        self.start(ctx, "Opening".into(), job, Some(target));
    }

    fn activate(&mut self, ctx: &egui::Context, idx: usize) {
        let Some(r) = self.rows.get(idx).cloned() else { return };
        if r.is_dir { self.go(&r.full); } else { self.open_entry(ctx, &r.full); }
    }

    // ------------------------------------------------------------ UI pieces

    fn toolbar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            if ui.button("Open").clicked() {
                if let Some(p) = rfd::FileDialog::new().set_title("Open archive").pick_file() { self.load(ctx, p, None); }
            }
            if ui.button("Add").clicked() {
                if let Some(files) = rfd::FileDialog::new().set_title("Add files").pick_files() { self.add = Some(AddDialog::new(files, Format::Zip)); }
            }
            if ui.button("Add folder").clicked() {
                if let Some(d) = rfd::FileDialog::new().set_title("Add folder").pick_folder() { self.add = Some(AddDialog::new(vec![d], Format::Zip)); }
            }
            ui.add_space(8.0);
            let has = self.open.is_some() && self.running.is_none();
            if ui.add_enabled(has, primary("Extract")).clicked() { self.extract_clicked(ctx); }
        });
    }

    fn path_bar(&mut self, ui: &mut egui::Ui) {
        let Some(o) = &self.open else { return };
        let root = o.path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let mut nav: Option<String> = None;
        let cwd = self.cwd.clone();
        ui.horizontal(|ui| {
            if ui.add_enabled(!cwd.is_empty(), egui::Button::new("Up")).clicked() { nav = Some(match cwd.rsplit_once('/') { Some((p, _)) => p.to_string(), None => String::new() }); }
            if ui.link(RichText::new(root).strong()).clicked() { nav = Some(String::new()); }
            let mut acc = String::new();
            for part in cwd.split('/').filter(|s| !s.is_empty()) {
                if !acc.is_empty() { acc.push('/'); }
                acc.push_str(part);
                ui.label("/");
                if ui.link(part).clicked() { nav = Some(acc.clone()); }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let r = ui.add(egui::TextEdit::singleline(&mut self.filter).hint_text("Search").desired_width(200.0));
                if r.changed() { self.dirty = true; }
            });
        });
        if let Some(n) = nav { self.go(&n); }
    }

    fn table(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let mods = ctx.input(|i| i.modifiers);
        let (mut clicked, mut dbl, mut sort_click): (Option<usize>, Option<usize>, Option<Sort>) = (None, None, None);
        let (sort, rows, sel) = (self.sort, &self.rows, &self.sel);
        TableBuilder::new(ui).striped(true).sense(Sense::click()).resizable(true).cell_layout(Layout::left_to_right(Align::Center))
            .column(Column::remainder().at_least(180.0).clip(true))
            .column(Column::initial(92.0).at_least(60.0))
            .column(Column::initial(92.0).at_least(60.0))
            .column(Column::initial(132.0).at_least(96.0))
            .header(26.0, |mut h| {
                for (label, key) in [("Name", Sort::Name), ("Size", Sort::Size), ("Packed", Sort::Packed), ("Modified", Sort::Modified)] {
                    h.col(|ui| {
                        let r = ui.add(egui::Label::new(RichText::new(label).strong()).sense(Sense::click()));
                        if sort.0 == key {
                            let c = r.rect.right_center() + vec2(9.0, 0.0);
                            let s = 4.0; let dir = if sort.1 { -1.0 } else { 1.0 };
                            ui.painter().add(Shape::convex_polygon(vec![c + vec2(-s, -dir * s * 0.6), c + vec2(s, -dir * s * 0.6), c + vec2(0.0, dir * s * 0.8)], ACCENT, Stroke::NONE));
                        }
                        if r.clicked() { sort_click = Some(key); }
                    });
                }
            })
            .body(|body| {
                body.rows(24.0, rows.len(), |mut row| {
                    let i = row.index();
                    let r = &rows[i];
                    row.set_selected(sel.contains(&r.full));
                    row.col(|ui| {
                        let (rect, _) = ui.allocate_exact_size(vec2(16.0, 14.0), Sense::hover());
                        if r.is_dir {
                            ui.painter().rect_filled(Rect::from_min_size(rect.min + vec2(0.0, 2.0), vec2(16.0, 11.0)), 2.5, ACCENT);
                            ui.painter().rect_filled(Rect::from_min_size(rect.min, vec2(7.0, 4.0)), 1.5, ACCENT);
                        } else {
                            ui.painter().rect_filled(Rect::from_min_size(rect.min + vec2(2.0, 0.0), vec2(11.0, 14.0)), 2.0, tint(120));
                        }
                        ui.add(egui::Label::new(&r.name).truncate());
                    });
                    row.col(|ui| { if !(r.is_dir && r.size == 0) { ui.label(fsutil::human_size(r.size)); } });
                    row.col(|ui| { if r.packed > 0 { ui.label(fsutil::human_size(r.packed)); } });
                    row.col(|ui| { if r.modified > 0 { let (y, m, d, hh, mm, _) = fsutil::unix_to_civil(r.modified); ui.label(format!("{y:04}-{m:02}-{d:02} {hh:02}:{mm:02}")); } });
                    let resp = row.response();
                    if resp.clicked() { clicked = Some(i); }
                    if resp.double_clicked() { dbl = Some(i); }
                });
            });
        if let Some(k) = sort_click { self.sort = if self.sort.0 == k { (k, !self.sort.1) } else { (k, true) }; self.dirty = true; }
        if let Some(i) = clicked {
            let full = self.rows[i].full.clone();
            if mods.shift && self.anchor.is_some() {
                let a = self.anchor.unwrap_or(i); let (lo, hi) = (a.min(i), a.max(i));
                self.sel = self.rows[lo..=hi].iter().map(|r| r.full.clone()).collect();
            } else if mods.command {
                if !self.sel.remove(&full) { self.sel.insert(full); }
                self.anchor = Some(i);
            } else { self.sel.clear(); self.sel.insert(full); self.anchor = Some(i); }
        }
        if let Some(i) = dbl { self.activate(ctx, i); }
    }

    fn running_ui(&mut self, ui: &mut egui::Ui) {
        let Some(r) = &self.running else { return };
        ui.label(RichText::new(&r.title).strong());
        ui.add_space(6.0);
        ui.add(egui::ProgressBar::new(r.progress.fraction()).show_percentage().desired_width(380.0));
        ui.add(egui::Label::new(r.progress.current()).truncate());
        ui.add_space(8.0);
        if ui.button("Cancel").clicked() { r.progress.cancel(); }
    }

    fn outcome_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let mut next: Option<Job> = None; let mut close = false;
        match &self.outcome {
            Some(Outcome::Done(msg, reveal)) => {
                ui.set_max_width(400.0);
                ui.label(msg);
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.add(primary("OK")).clicked() { close = true; }
                    if let Some(d) = reveal { if ui.button("Show folder").clicked() { launch(d); } }
                });
            }
            Some(Outcome::Failed(msg, job)) => {
                ui.set_max_width(400.0);
                error_line(ui, msg);
                ui.add_space(6.0);
                if msg.to_lowercase().contains("password") {
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut self.retry_pw).password(true).hint_text("Password").desired_width(200.0));
                        if ui.button("Retry").clicked() { next = Some(with_password(job.clone(), self.retry_pw.clone())); }
                    });
                }
                if ui.button("Close").clicked() { close = true; }
            }
            None => {}
        }
        if let Some(j) = next { self.outcome = None; self.start(ctx, "Working".into(), j, None); }
        else if close { self.outcome = None; if self.job_mode { ctx.send_viewport_cmd(egui::ViewportCommand::Close); } }
    }

    fn add_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let mut result: Option<Job> = None; let mut cancel = false;
        if let Some(d) = self.add.as_mut() {
            ui.set_width(420.0);
            ui.label(plural(d.inputs.len()));
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut d.out).desired_width(330.0));
                if ui.button("Browse").clicked() {
                    if let Some(p) = rfd::FileDialog::new().set_file_name(Path::new(&d.out).file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()).save_file() { d.out = p.display().to_string(); }
                }
            });
            ui.horizontal(|ui| {
                let fmt_opts: Vec<(Format, String)> = WRITE_FORMATS.into_iter().map(|f| (f, format!(".{}", f.extension()))).collect();
                if let Some(f) = dropdown(ui, "fmt", 120.0, &d.fmt, &fmt_opts) {
                    if f != d.fmt { d.fmt = f; d.out = zcore::create::suggest_name(&d.inputs, f).display().to_string(); }
                }
                let lvl_opts = vec![(Level::Store, "Store".to_string()), (Level::Fast, "Fast".to_string()), (Level::Normal, "Normal".to_string()), (Level::Max, "Maximum".to_string())];
                if let Some(l) = dropdown(ui, "lvl", 150.0, &d.level, &lvl_opts) { d.level = l; }
            });
            let can_encrypt = matches!(d.fmt, Format::Zip | Format::SevenZ);
            ui.add_enabled_ui(can_encrypt, |ui| {
                ui.add(egui::TextEdit::singleline(&mut d.pw).password(true).hint_text("Password (AES-256)").desired_width(260.0));
                ui.add(egui::TextEdit::singleline(&mut d.pw2).password(true).hint_text("Repeat password").desired_width(260.0));
            });
            let mismatch = can_encrypt && d.pw != d.pw2;
            if mismatch { error_line(ui, "Passwords do not match"); }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.add_enabled(!mismatch && !d.out.trim().is_empty(), primary("Create")).clicked() {
                    result = Some(Job::Add { inputs: d.inputs.clone(), out: Some(PathBuf::from(d.out.trim())),
                        opts: CreateOpts { format: d.fmt, level: d.level, password: if can_encrypt && !d.pw.is_empty() { Some(d.pw.clone()) } else { None }, threads: 0 }, dialog: false });
                }
                if ui.button("Cancel").clicked() { cancel = true; }
            });
        }
        if let Some(j) = result { self.add = None; self.start(ctx, "Creating archive".into(), j, None); }
        if cancel { self.add = None; if self.job_mode { ctx.send_viewport_cmd(egui::ViewportCommand::Close); } }
    }
}

impl AddDialog {
    fn new(inputs: Vec<PathBuf>, fmt: Format) -> Self {
        let out = zcore::create::suggest_name(&inputs, fmt).display().to_string();
        AddDialog { inputs, out, fmt, level: Level::Normal, pw: String::new(), pw2: String::new() }
    }
}

fn with_password(job: Job, pw: String) -> Job {
    let pw = if pw.is_empty() { None } else { Some(pw) };
    match job {
        Job::Extract { archives, dest, overwrite, only, .. } => Job::Extract { archives, dest, password: pw, overwrite, only },
        Job::Test { archives, .. } => Job::Test { archives, password: pw },
        other => other,
    }
}

fn fnv(s: &str) -> u64 { let mut h = 0xcbf29ce484222325u64; for b in s.bytes() { h ^= b as u64; h = h.wrapping_mul(0x100000001b3); } h }

fn launch(p: &Path) {
    #[cfg(windows)] { let _ = std::process::Command::new("explorer").arg(p).spawn(); }
    #[cfg(not(windows))] { let _ = std::process::Command::new("xdg-open").arg(p).spawn(); }
}

fn plural(n: usize) -> String { if n == 1 { "1 item".into() } else { format!("{n} items") } }

/// Filled violet call-to-action button.
fn primary(label: &str) -> impl egui::Widget {
    egui::Button::new(RichText::new(label).color(TXT)).fill(ACCENT_DEEP)
}

/// Error line: red marker, text stays in the interface colour.
fn error_line(ui: &mut egui::Ui, msg: &str) {
    ui.horizontal_wrapped(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(8.0, 8.0), Sense::hover());
        ui.painter().circle_filled(r.center() + vec2(0.0, 1.0), 4.0, Color32::from_rgb(235, 90, 90));
        ui.label(msg);
    });
}

/// Custom drop-down: a field-style trigger and a floating menu. Returns the option the user picked.
fn dropdown<T: Clone + PartialEq>(ui: &mut egui::Ui, salt: &str, width: f32, current: &T, options: &[(T, String)]) -> Option<T> {
    let id = ui.make_persistent_id(salt);
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 28.0), Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    let mut open = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
    if resp.clicked() { open = !open; }

    let label = options.iter().find(|(v, _)| v == current).map(|(_, l)| l.as_str()).unwrap_or("");
    let lit = open || resp.hovered();
    ui.painter().rect(rect, 6.0, tint(if lit { 30 } else { 16 }), Stroke::new(1.0_f32, if open { ACCENT } else { tint(if lit { 70 } else { 34 }) }));
    ui.painter().text(rect.left_center() + vec2(11.0, 0.0), Align2::LEFT_CENTER, label, FontId::new(13.5, FontFamily::Proportional), TXT);
    let c = rect.right_center() - vec2(14.0, 0.0);
    ui.painter().add(Shape::convex_polygon(vec![c + vec2(-4.5, -2.5), c + vec2(4.5, -2.5), c + vec2(0.0, 3.0)], TXT, Stroke::NONE));

    let mut picked: Option<T> = None;
    if open {
        let area = egui::Area::new(id.with("menu")).order(egui::Order::Foreground).fixed_pos(rect.left_bottom() + vec2(0.0, 4.0)).show(ui.ctx(), |ui| {
            egui::Frame::none().fill(MENU_BG).stroke(Stroke::new(1.0_f32, tint(44))).rounding(8.0).inner_margin(4.0).show(ui, |ui| {
                ui.spacing_mut().item_spacing = vec2(0.0, 2.0);
                for (v, l) in options {
                    let (r, rr) = ui.allocate_exact_size(vec2(width - 10.0, 26.0), Sense::click());
                    let selected = v == current;
                    if selected { ui.painter().rect_filled(r, 5.0, ACCENT_DEEP); }
                    else if rr.hovered() { ui.painter().rect_filled(r, 5.0, tint(26)); }
                    ui.painter().text(r.left_center() + vec2(10.0, 0.0), Align2::LEFT_CENTER, l, FontId::new(13.5, FontFamily::Proportional), TXT);
                    if selected {
                        let m = r.right_center() - vec2(14.0, 0.0);
                        let st = Stroke::new(1.6_f32, TXT);
                        ui.painter().line_segment([m + vec2(-4.0, 0.0), m + vec2(-1.0, 3.0)], st);
                        ui.painter().line_segment([m + vec2(-1.0, 3.0), m + vec2(4.5, -3.5)], st);
                    }
                    if rr.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() { picked = Some(v.clone()); }
                }
            });
        });
        if picked.is_some() || ui.input(|i| i.key_pressed(egui::Key::Escape)) || area.response.clicked_elsewhere() { open = false; }
    }
    ui.data_mut(|d| d.insert_temp(id, open));
    picked
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()))
}

fn gradient_at(t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0) * 2.0;
    if t < 1.0 { lerp_color(GRADIENT[0], GRADIENT[1], t) } else { lerp_color(GRADIENT[1], GRADIENT[2], t - 1.0) }
}

/// Window-wide brand gradient, painted behind every panel.
fn paint_background(ctx: &egui::Context) {
    const N: usize = 12;
    let rect = ctx.screen_rect();
    let mut mesh = egui::Mesh::default();
    for j in 0..=N {
        for i in 0..=N {
            let (u, v) = (i as f32 / N as f32, j as f32 / N as f32);
            mesh.colored_vertex(rect.min + vec2(u * rect.width(), v * rect.height()), gradient_at(u * 0.45 + v * 0.55));
        }
    }
    let row = N as u32 + 1;
    for j in 0..N as u32 {
        for i in 0..N as u32 {
            let (a, b, c, d) = (j * row + i, j * row + i + 1, (j + 1) * row + i, (j + 1) * row + i + 1);
            mesh.add_triangle(a, b, c); mesh.add_triangle(b, d, c);
        }
    }
    ctx.layer_painter(egui::LayerId::background()).add(Shape::mesh(mesh));
}

// ------------------------------------------------------------ theme, fonts, logo

pub fn apply_theme(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    let r = egui::Rounding::same(6.0);
    for w in [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.rounding = r;
        w.fg_stroke = Stroke::new(1.0_f32, TXT);
    }
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, tint(30));
    v.widgets.inactive.bg_fill = tint(16);  v.widgets.inactive.weak_bg_fill = tint(16);  v.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, tint(34));
    v.widgets.hovered.bg_fill = tint(30);   v.widgets.hovered.weak_bg_fill = tint(30);   v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, tint(70));
    v.widgets.active.bg_fill = tint(44);    v.widgets.active.weak_bg_fill = tint(44);    v.widgets.active.bg_stroke = Stroke::new(1.0_f32, ACCENT);
    v.widgets.open.bg_fill = tint(30);      v.widgets.open.weak_bg_fill = tint(30);

    v.override_text_color = Some(TXT);
    v.hyperlink_color = TXT;
    v.selection.bg_fill = ACCENT_DEEP;
    v.selection.stroke = Stroke::new(1.0_f32, TXT);

    v.window_rounding = egui::Rounding::same(12.0);
    v.window_fill = MENU_BG;
    v.window_stroke = Stroke::new(1.0_f32, tint(40));
    v.window_shadow = egui::Shadow::NONE;
    v.popup_shadow = egui::Shadow::NONE;

    v.panel_fill = Color32::TRANSPARENT;           // the gradient is painted behind the panels
    v.extreme_bg_color = Color32::from_rgb(15, 12, 34);
    v.faint_bg_color = tint(9);
    let mut s = (*ctx.style()).clone();
    s.visuals = v;
    s.spacing.item_spacing = vec2(8.0, 6.0);
    s.spacing.button_padding = vec2(12.0, 5.0);
    s.text_styles = [
        (TextStyle::Heading, FontId::new(18.0, FontFamily::Name("medium".into()))),
        (TextStyle::Body, FontId::new(13.5, FontFamily::Proportional)),
        (TextStyle::Button, FontId::new(13.5, FontFamily::Name("medium".into()))),
        (TextStyle::Small, FontId::new(11.5, FontFamily::Proportional)),
        (TextStyle::Monospace, FontId::new(12.5, FontFamily::Monospace)),
    ].into();
    ctx.set_style(s);
}

/// Z Sans (22 KB per weight) everywhere. Wide-script fallbacks are read from the OS only when an
/// archive actually contains such names, so a normal session stays small in memory.
pub fn install_fonts(ctx: &egui::Context, with_fallbacks: bool) {
    let mut fonts = egui::FontDefinitions::empty();
    fonts.font_data.insert("zsans".into(), egui::FontData::from_static(include_bytes!("../../../assets/font/ZSans-Regular.ttf")));
    fonts.font_data.insert("zsans-medium".into(), egui::FontData::from_static(include_bytes!("../../../assets/font/ZSans-Medium.ttf")));
    let mut fallback: Vec<String> = Vec::new();
    if with_fallbacks {
        let candidates = [
            r"C:\Windows\Fonts\segoeui.ttf", r"C:\Windows\Fonts\msyh.ttc", r"C:\Windows\Fonts\YuGothM.ttc", r"C:\Windows\Fonts\malgun.ttf",
            r"C:\Windows\Fonts\leelawui.ttf", "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc", "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        ];
        for (i, p) in candidates.iter().enumerate() {
            if let Ok(bytes) = std::fs::read(p) { let k = format!("sys{i}"); fonts.font_data.insert(k.clone(), egui::FontData::from_owned(bytes)); fallback.push(k); }
        }
    }
    let mut mono_key = "zsans".to_string();
    for (i, p) in [r"C:\Windows\Fonts\consola.ttf", "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf"].iter().enumerate() {
        if let Ok(bytes) = std::fs::read(p) { let k = format!("mono{i}"); fonts.font_data.insert(k.clone(), egui::FontData::from_owned(bytes)); mono_key = k; break; }
    }
    let fam = |first: &str| { let mut v = vec![first.to_string()]; v.extend(fallback.iter().cloned()); v };
    fonts.families.insert(FontFamily::Proportional, fam("zsans"));
    fonts.families.insert(FontFamily::Name("medium".into()), fam("zsans-medium"));
    fonts.families.insert(FontFamily::Monospace, { let mut v = fam(&mono_key); if mono_key != "zsans" { v.push("zsans".into()); } v });
    ctx.set_fonts(fonts);
}

pub fn paint_logo(painter: &egui::Painter, rect: Rect) {
    let s = rect.width();
    painter.rect_filled(rect, s * 0.225, ACCENT);
    let p = |x: f32, y: f32| -> Pos2 { rect.min + vec2(x * s, y * s) };
    let (l, t, r, b, th, k) = (0.27, 0.27, 0.73, 0.73, 0.118, 0.17);
    painter.rect_filled(Rect::from_min_max(p(l, t), p(r, t + th)), s * 0.012, TXT);
    painter.rect_filled(Rect::from_min_max(p(l, b - th), p(r, b)), s * 0.012, TXT);
    painter.add(Shape::convex_polygon(vec![p(r - k, t + th), p(r, t + th), p(l + k, b - th), p(l, b - th)], TXT, Stroke::NONE));
}

// ------------------------------------------------------------ main loop

impl eframe::App for ZApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_loader(ctx); self.poll_job(ctx);
        paint_background(ctx);
        if self.running.is_some() || self.loader.is_some() { ctx.request_repaint_after(Duration::from_millis(60)); }

        // Drag a file onto the window: archives open, anything else becomes "add to archive".
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        if !dropped.is_empty() && !self.job_mode {
            if dropped.len() == 1 && Format::detect(&dropped[0]).is_some() && dropped[0].is_file() { self.load(ctx, dropped[0].clone(), None); }
            else { self.add = Some(AddDialog::new(dropped, Format::Zip)); }
        }
        if self.dirty { self.rebuild(); }

        if !ctx.wants_keyboard_input() && self.open.is_some() {
            if ctx.input(|i| i.key_pressed(egui::Key::Backspace)) { self.up(); }
            if ctx.input(|i| i.key_pressed(egui::Key::Enter)) { if let Some(i) = self.rows.iter().position(|r| self.sel.contains(&r.full)) { self.activate(ctx, i); } }
            if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::A)) { self.sel = self.rows.iter().map(|r| r.full.clone()).collect(); }
        }

        if self.job_mode {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.add_space(4.0);
                if self.running.is_some() { self.running_ui(ui); }
                else if self.outcome.is_some() { self.outcome_ui(ui, ctx); }
                else if self.add.is_some() { self.add_ui(ui, ctx); }
            });
            return;
        }

        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| { ui.add_space(8.0); self.toolbar(ui, ctx); ui.add_space(2.0); if self.open.is_some() { self.path_bar(ui); } ui.add_space(6.0); });
        if self.open.is_some() {
            egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
                let (n, sel_n) = (self.rows.len(), self.sel.len());
                let sel_size: u64 = self.rows.iter().filter(|r| self.sel.contains(&r.full)).map(|r| r.size).sum();
                let o = self.open.as_ref();
                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    ui.label(plural(n));
                    if sel_n > 0 { ui.label(format!("{sel_n} selected, {}", fsutil::human_size(sel_size))); }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if let Some(o) = o { let total: u64 = o.entries.iter().map(|e| e.size).sum(); ui.label(format!("{}  \u{b7}  {} \u{2192} {}", o.fmt.extension().to_uppercase(), fsutil::human_size(o.disk_size), fsutil::human_size(total))); }
                    });
                });
                ui.add_space(2.0);
            });
            egui::CentralPanel::default().show(ctx, |ui| self.table(ui, ctx));
        } else {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space((ui.available_height() * 0.30).max(20.0));
                    let (rect, _) = ui.allocate_exact_size(vec2(96.0, 96.0), Sense::hover());
                    paint_logo(ui.painter(), rect);
                    ui.add_space(18.0);
                    if self.loader.is_some() { ui.spinner(); } else { ui.label("Drop an archive here"); }
                });
            });
        }

        let win = |_ctx: &egui::Context, title: &str| egui::Window::new(title.to_string()).collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, [0.0, 0.0]);
        // Opening one file from an archive is usually instant: only show the dialog if it takes a moment.
        let show_running = self.running.as_ref().map_or(false, |r| r.open_after.is_none() || r.started.elapsed() > Duration::from_millis(400));
        if self.running.is_some() {
            if show_running { win(ctx, "Z").show(ctx, |ui| self.running_ui(ui)); }
        }
        else if self.outcome.is_some() { win(ctx, "Z").show(ctx, |ui| self.outcome_ui(ui, ctx)); }
        else if self.add.is_some() { win(ctx, "Add to archive").show(ctx, |ui| self.add_ui(ui, ctx)); }
        if let Some((path, mut pw)) = self.open_pw.take() {
            let (mut go, mut cancel) = (false, false);
            win(ctx, "Password").show(ctx, |ui| {
                ui.label("This archive is encrypted.");
                let r = ui.add(egui::TextEdit::singleline(&mut pw).password(true).desired_width(240.0));
                if r.lost_focus() && ctx.input(|i| i.key_pressed(egui::Key::Enter)) { go = true; }
                ui.horizontal(|ui| { if ui.button("Open").clicked() { go = true; } if ui.button("Cancel").clicked() { cancel = true; } });
            });
            if go { self.load(ctx, path, Some(pw)); } else if !cancel { self.open_pw = Some((path, pw)); }
        }
        if let Some(e) = self.error.clone() {
            win(ctx, "Z").show(ctx, |ui| { ui.set_max_width(400.0); error_line(ui, &e); ui.add_space(6.0); if ui.button("Close").clicked() { self.error = None; } });
        }
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] { [0.055, 0.043, 0.133, 1.0] }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) { let _ = std::fs::remove_dir_all(&self.temp_root); }
}
