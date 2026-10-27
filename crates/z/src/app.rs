use crate::ui::{self, checkbox, dropdown, option_list, paint_background, paint_logo, popup_close, popup_is_open, popup_toggle, primary, tint, ACCENT, RADIUS};
use eframe::egui::{self, vec2, Align, Align2, Layout, Rect, RichText, Sense, Shape, Stroke};
use egui_extras::{Column, TableBuilder};
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};
use z::jobs::{self, Dest, Job};
use zcore::{fsutil, CreateOpts, Entry, Format, Level, Overwrite, Progress};

pub enum Startup { Empty, Open(PathBuf), Job(Job), Error(String) }

#[derive(Clone)]
struct Row { name: String, key: String, full: String, is_dir: bool, size: u64, packed: u64, modified: i64 }

#[derive(Clone, Copy, PartialEq, Eq)]
enum Sort { Name, Size, Packed, Modified }

struct Opened { path: PathBuf, fmt: Format, entries: Vec<Entry>, disk_size: u64, password: Option<String> }

struct Loader { path: PathBuf, password: Option<String>, rx: mpsc::Receiver<Result<(Format, Vec<Entry>), String>> }

struct Running { title: String, progress: Arc<Progress>, rx: mpsc::Receiver<Result<String, String>>, job: Job, open_after: Option<PathBuf>, started: Instant }

enum Outcome { Done(String, Option<PathBuf>), Failed(String, Job) }

struct AddDialog { inputs: Vec<PathBuf>, out: String, fmt: Format, level: Level, use_pw: bool, pw: String, pw2: String }

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
    open_pw: Option<(PathBuf, String, bool)>,
    error: Option<String>,
    wide_fonts: bool,
    temp_root: PathBuf,
}

impl ZApp {
    pub fn new(cc: &eframe::CreationContext<'_>, startup: Startup) -> Self {
        ui::install_fonts(&cc.egui_ctx, false);
        ui::apply_theme(&cc.egui_ctx);
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
            Startup::Error(e) => app.error = Some(crisp(&e)),
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
                let fmt = Format::detect(&p2).ok_or_else(|| anyhow::anyhow!("Not an archive file"))?;
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
                    self.wide_fonts = true; ui::install_fonts(ctx, true);          // CJK/Arabic/Thai names: load system fallback fonts on demand
                }
                let disk_size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                let title = path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Z".to_string());
                ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
                self.open = Some(Opened { path, fmt, entries, disk_size, password });
                self.cwd.clear(); self.sel.clear(); self.anchor = None; self.filter.clear(); self.dirty = true;
            }
            Err(e) if e.to_lowercase().contains("password") => self.open_pw = Some((path, String::new(), password.is_some())),
            Err(e) => self.error = Some(crisp(&e)),
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
        if self.job_mode { ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(vec2(460.0, 210.0))); }
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
            // One button for files and folders alike.
            let arch = ui.button("Archive");
            let menu_id = egui::Id::new("archive_menu");
            if arch.clicked() { popup_toggle(ctx, menu_id); }
            if popup_is_open(ctx, menu_id) {
                let labels = vec!["Files".to_string(), "Folder".to_string()];
                let (picked, close) = option_list(ctx, menu_id, arch.rect, 140.0, &labels, None);
                if close { popup_close(ctx); }
                match picked {
                    Some(0) => { if let Some(files) = rfd::FileDialog::new().set_title("Add files").pick_files() { self.add = Some(AddDialog::new(files, Format::Zip)); } }
                    Some(1) => { if let Some(d) = rfd::FileDialog::new().set_title("Add folder").pick_folder() { self.add = Some(AddDialog::new(vec![d], Format::Zip)); } }
                    _ => {}
                }
            }
            ui.add_space(8.0);
            let has = self.open.is_some() && self.running.is_none();
            if ui.add_enabled(has, primary("Extract")).clicked() { self.extract_clicked(ctx); }
        });
    }

    fn path_bar(&mut self, ui: &mut egui::Ui) {
        if self.open.is_none() { return; }
        let mut nav: Option<String> = None;
        let cwd = self.cwd.clone();
        ui.horizontal(|ui| {
            if ui::icon_back(ui, !cwd.is_empty()).clicked() && !cwd.is_empty() {
                nav = Some(match cwd.rsplit_once('/') { Some((p, _)) => p.to_string(), None => String::new() });
            }
            let mut acc = String::new();
            for (n, part) in cwd.split('/').filter(|s| !s.is_empty()).enumerate() {
                if !acc.is_empty() { acc.push('/'); }
                acc.push_str(part);
                if n > 0 { ui.label("/"); }
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
        ui.scope(|ui| {
            // Column dividers are drawn in the header row only, never down the file list.
            ui.visuals_mut().widgets.noninteractive.bg_stroke = Stroke::NONE;
            ui.visuals_mut().widgets.hovered.bg_stroke = Stroke::NONE;
            ui.visuals_mut().widgets.active.bg_stroke = Stroke::NONE;
            TableBuilder::new(ui).striped(false).sense(Sense::click()).resizable(true).cell_layout(Layout::left_to_right(Align::Center))
                .column(Column::remainder().at_least(180.0).clip(true))
                .column(Column::initial(92.0).at_least(60.0))
                .column(Column::initial(92.0).at_least(60.0))
                .column(Column::initial(132.0).at_least(96.0))
                .header(26.0, |mut h| {
                    for (n, (label, key)) in [("Name", Sort::Name), ("Size", Sort::Size), ("Packed", Sort::Packed), ("Modified", Sort::Modified)].into_iter().enumerate() {
                        h.col(|ui| {
                            if n < 3 {
                                let r = ui.max_rect();
                                ui.painter().line_segment([egui::pos2(r.right() - 0.5, r.top() + 5.0), egui::pos2(r.right() - 0.5, r.bottom() - 5.0)], Stroke::new(1.0_f32, tint(46)));
                            }
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
                ui.label(crisp(msg));
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
                ui.add(egui::TextEdit::singleline(&mut d.out).desired_width(326.0));
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
            if !can_encrypt { d.use_pw = false; }
            ui.add_enabled_ui(can_encrypt, |ui| { checkbox(ui, &mut d.use_pw, "Protect with a password"); });
            if d.use_pw {
                ui.add(egui::TextEdit::singleline(&mut d.pw).password(true).hint_text("Password").desired_width(260.0));
                ui.add(egui::TextEdit::singleline(&mut d.pw2).password(true).hint_text("Repeat password").desired_width(260.0));
            } else {
                d.pw.clear(); d.pw2.clear();
            }
            let mismatch = d.use_pw && (d.pw.is_empty() || d.pw != d.pw2);
            if d.use_pw && !d.pw2.is_empty() && d.pw != d.pw2 { ui.label("Passwords do not match"); }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.add_enabled(!mismatch && !d.out.trim().is_empty(), primary("Create")).clicked() {
                    result = Some(Job::Add { inputs: d.inputs.clone(), out: Some(PathBuf::from(d.out.trim())),
                        opts: CreateOpts { format: d.fmt, level: d.level, password: if d.use_pw && !d.pw.is_empty() { Some(d.pw.clone()) } else { None }, threads: 0 }, dialog: false });
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
        AddDialog { inputs, out, fmt, level: Level::Normal, use_pw: false, pw: String::new(), pw2: String::new() }
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

/// Short, plain wording for every error the user can meet. No technical detail, no markers.
fn crisp(e: &str) -> String {
    let l = e.to_lowercase();
    let has = |s: &str| l.contains(s);
    let msg = if has("wrong password") { "Wrong password" }
    else if has("password") { "Password required" }
    else if has("not an archive") || has("unrecognised") || has("unrecognized") || has("not a valid") || has("invalid zip") || has("invalid archive") { "Not an archive file" }
    else if has("corrupt") || has("crc") || has("checksum") || has("unexpected end") { "Archive is damaged" }
    else if has("compiled without rar") { "RAR is not supported in this build" }
    else if has("access is denied") || has("permission denied") { "Access denied" }
    else if has("cannot find") || has("no such file") || has("not found") { "File not found" }
    else if has("no space") || has("disk full") { "Not enough disk space" }
    else if has("holds exactly one file") { "This format holds one file only" }
    else if has("cannot create") { "Cannot create this format" }
    else if has("nothing to add") { "Nothing to add" }
    else if has("cancelled") { "Cancelled" }
    else { "Something went wrong" };
    msg.to_string()
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
                        if let Some(o) = o { let total: u64 = o.entries.iter().map(|e| e.size).sum(); ui.label(format!("{}  \u{b7}  {}  |  {}", o.fmt.extension().to_uppercase(), fsutil::human_size(o.disk_size), fsutil::human_size(total))); }
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
        if let Some((path, mut pw, wrong)) = self.open_pw.take() {
            let (mut go, mut cancel) = (false, false);
            win(ctx, "Password").show(ctx, |ui| {
                ui.label(if wrong { "Wrong password" } else { "Password required" });
                let r = ui.add(egui::TextEdit::singleline(&mut pw).password(true).desired_width(240.0));
                if r.lost_focus() && ctx.input(|i| i.key_pressed(egui::Key::Enter)) { go = true; }
                ui.horizontal(|ui| { if ui.button("Open").clicked() { go = true; } if ui.button("Cancel").clicked() { cancel = true; } });
            });
            if go { self.load(ctx, path, Some(pw)); } else if !cancel { self.open_pw = Some((path, pw, wrong)); }
        }
        if let Some(e) = self.error.clone() {
            win(ctx, "Z").show(ctx, |ui| { ui.set_max_width(400.0); ui.label(&e); ui.add_space(6.0); if ui.button("Close").clicked() { self.error = None; } });
        }
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] { [0.055, 0.043, 0.133, 1.0] }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) { let _ = std::fs::remove_dir_all(&self.temp_root); }
}
