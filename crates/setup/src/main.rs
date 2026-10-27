#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
//! Z Setup: the installer (and uninstaller) of Z, built with the same widgets as the app.

#[path = "../../z/src/ui.rs"]
mod ui;
mod install;
mod reg;

use eframe::egui::{self, vec2, Align, Align2, Color32, FontFamily, FontId, Layout, Margin, Rect, RichText, Sense, Stroke};
use install::{Config, Progress};
use std::path::PathBuf;
use std::sync::{mpsc, Arc};
use std::time::Duration;
use ui::{checkbox, dropdown, paint_background, paint_logo, progress_bar, radio, tint, ACCENT_DEEP, RADIUS, TXT};

const LICENSE_TEXT: &str = include_str!("../../../LICENSE");
const HEADER: Color32 = Color32::from_rgb(28, 20, 72);
const FOOTER: Color32 = Color32::from_rgb(17, 13, 42);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page { License, Location, Components, Tasks, Installing, Done, Failed, Confirm, Removing, Removed }

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind { Full, Compact, Custom }

#[derive(Clone, Copy, PartialEq, Eq)]
enum Act { Next, Back, Cancel, Install, Finish, Uninstall }

struct Worker { rx: mpsc::Receiver<Result<(), String>>, prog: Arc<Progress> }

struct SetupApp {
    page: Page,
    uninstall: bool,
    cfg: Config,
    accepted: bool,
    kind: Kind,
    worker: Option<Worker>,
    error: String,
    launch: bool,
    admin: bool,
    autostart: bool,
    uninstall_dir: PathBuf,
    machine_wide: bool,
}

fn mb(n: usize) -> String { format!("{:.1} MB", n as f64 / 1_048_576.0) }

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let uninstall = args.iter().any(|a| a == "--uninstall");
    let yes = args.iter().any(|a| a == "--yes");
    let cfg_path: Option<String> = args.iter().position(|a| a == "--cfg").and_then(|i| args.get(i + 1)).cloned();
    let admin = install::is_admin();

    let mut app = SetupApp {
        page: if uninstall { Page::Confirm } else { Page::License },
        uninstall, cfg: Config::new(admin), accepted: false, kind: Kind::Full, worker: None, error: String::new(),
        launch: true, admin, autostart: false, uninstall_dir: PathBuf::new(), machine_wide: false,
    };
    if let Some(p) = cfg_path {
        if let Ok(t) = std::fs::read_to_string(&p) { app.cfg = Config::from_text(&t); app.autostart = true; }
        let _ = std::fs::remove_file(&p);
    }
    if uninstall {
        let dir = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.to_path_buf())).unwrap_or_default();
        app.machine_wide = install::dir_is_machine_wide(&dir);
        app.uninstall_dir = dir;
        if yes { app.autostart = true; }
    }

    let rgba = include_bytes!("../../../assets/icons/z-64.rgba").to_vec();
    let vp = egui::ViewportBuilder::default()
        .with_inner_size([780.0, 560.0])
        .with_title("Z Setup")
        .with_decorations(false)
        .with_resizable(false)
        .with_icon(Arc::new(egui::IconData { rgba, width: 64, height: 64 }));
    let options = eframe::NativeOptions { viewport: vp, centered: true, ..Default::default() };
    eframe::run_native("Z Setup", options, Box::new(move |cc| {
        ui::install_fonts(&cc.egui_ctx, false);
        ui::apply_theme(&cc.egui_ctx);
        Ok(Box::new(app))
    }))
}

fn title_bar(ui: &mut egui::Ui, ctx: &egui::Context, busy: bool) {
    let rect = ui.max_rect();
    let drag = ui.interact(rect, ui.id().with("drag"), Sense::click_and_drag());
    if drag.drag_started() { ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag); }
    paint_logo(ui.painter(), Rect::from_center_size(egui::pos2(rect.left() + 20.0, rect.center().y), vec2(16.0, 16.0)));
    ui.painter().text(egui::pos2(rect.left() + 36.0, rect.center().y), Align2::LEFT_CENTER, "Z Setup", FontId::new(13.0, FontFamily::Proportional), TXT);
    let bw = 46.0;
    for (i, close) in [false, true].into_iter().enumerate() {
        let r = Rect::from_min_size(egui::pos2(rect.right() - bw * (2.0 - i as f32), rect.top()), vec2(bw, rect.height()));
        let resp = ui.interact(r, ui.id().with(("tb", i)), Sense::click());
        if resp.hovered() { ui.painter().rect_filled(r, 0.0, if close { Color32::from_rgb(196, 43, 28) } else { tint(26) }); }
        let c = r.center();
        let st = Stroke::new(1.2_f32, TXT);
        if close {
            ui.painter().line_segment([c + vec2(-5.0, -5.0), c + vec2(5.0, 5.0)], st);
            ui.painter().line_segment([c + vec2(-5.0, 5.0), c + vec2(5.0, -5.0)], st);
        } else {
            ui.painter().line_segment([c + vec2(-5.0, 0.0), c + vec2(5.0, 0.0)], st);
        }
        if resp.clicked() {
            if close { if !busy { ctx.send_viewport_cmd(egui::ViewportCommand::Close); } }
            else { ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true)); }
        }
    }
}

fn btn(ui: &mut egui::Ui, label: &str, enabled: bool, primary: bool) -> bool {
    let b = if primary { egui::Button::new(RichText::new(label).color(TXT)).fill(ACCENT_DEEP) } else { egui::Button::new(label) };
    ui.add_enabled(enabled, b.min_size(vec2(100.0, 32.0))).clicked()
}

fn heading(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).font(FontId::new(13.5, FontFamily::Name("medium".into()))));
}

impl SetupApp {
    fn titles(&self) -> (&'static str, &'static str) {
        match self.page {
            Page::License => ("License Agreement", "Please read the following information before continuing."),
            Page::Location => ("Install Location", "Choose where Z is installed."),
            Page::Components => ("Select Components", "Which components should be installed?"),
            Page::Tasks => ("Additional Options", "Choose how Z works with Windows."),
            Page::Installing => ("Installing", "Please wait while Z is installed."),
            Page::Done => ("Setup Complete", "Z is ready to use."),
            Page::Failed => ("Setup Failed", "Z could not be completed."),
            Page::Confirm => ("Uninstall Z", "Remove Z from this computer."),
            Page::Removing => ("Uninstalling", "Please wait while Z is removed."),
            Page::Removed => ("Uninstall Complete", "Z has been removed."),
        }
    }

    fn start(&mut self, ctx: &egui::Context) {
        let prog = Arc::new(Progress::new());
        let (tx, rx) = mpsc::channel();
        let (p2, c2, cfg) = (prog.clone(), ctx.clone(), self.cfg.clone());
        let (un, dir, mw) = (self.uninstall, self.uninstall_dir.clone(), self.machine_wide);
        std::thread::spawn(move || {
            let r = if un { install::uninstall(&dir, mw, &p2) } else { install::install(&cfg, &p2) };
            let _ = tx.send(r.map_err(|e| e.to_string()));
            c2.request_repaint();
        });
        self.worker = Some(Worker { rx, prog });
        self.page = if self.uninstall { Page::Removing } else { Page::Installing };
    }

    fn poll(&mut self) {
        let done = self.worker.as_ref().and_then(|w| w.rx.try_recv().ok());
        if let Some(r) = done {
            self.worker = None;
            match r {
                Ok(()) => self.page = if self.uninstall { Page::Removed } else { Page::Done },
                Err(e) => { self.error = e; self.page = Page::Failed; }
            }
        }
    }

    fn apply(&mut self, act: Act, ctx: &egui::Context) {
        match act {
            Act::Cancel => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Act::Next => self.page = match self.page { Page::License => Page::Location, Page::Location => Page::Components, Page::Components => Page::Tasks, p => p },
            Act::Back => self.page = match self.page { Page::Location => Page::License, Page::Components => Page::Location, Page::Tasks => Page::Components, p => p },
            Act::Install => {
                if self.cfg.all_users && !self.admin {
                    let file = std::env::temp_dir().join(format!("z-setup-{}.cfg", std::process::id()));
                    let ok = std::fs::write(&file, self.cfg.to_text()).is_ok()
                        && install::relaunch_elevated(&format!("--cfg \"{}\"", file.display()));
                    if ok { ctx.send_viewport_cmd(egui::ViewportCommand::Close); }
                    else { let _ = std::fs::remove_file(&file); self.error = "Administrator permission was not granted".into(); self.page = Page::Failed; }
                } else { self.start(ctx); }
            }
            Act::Uninstall => {
                if self.machine_wide && !self.admin {
                    if install::relaunch_elevated("--uninstall --yes") { ctx.send_viewport_cmd(egui::ViewportCommand::Close); }
                    else { self.error = "Administrator permission was not granted".into(); self.page = Page::Failed; }
                } else { self.start(ctx); }
            }
            Act::Finish => {
                if self.launch && !self.uninstall && self.page == Page::Done {
                    let _ = std::process::Command::new("explorer").arg(format!("{}\\Z.exe", self.cfg.dir)).spawn();
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    fn footer(&self, ui: &mut egui::Ui) -> Option<Act> {
        let mut act: Option<Act> = None;
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add_space(24.0);
            match self.page {
                Page::License => {
                    if btn(ui, "Cancel", true, false) { act = Some(Act::Cancel); }
                    if btn(ui, "Next", self.accepted, true) { act = Some(Act::Next); }
                }
                Page::Location | Page::Components => {
                    if btn(ui, "Cancel", true, false) { act = Some(Act::Cancel); }
                    if btn(ui, "Next", true, true) { act = Some(Act::Next); }
                    if btn(ui, "Back", true, false) { act = Some(Act::Back); }
                }
                Page::Tasks => {
                    if btn(ui, "Cancel", true, false) { act = Some(Act::Cancel); }
                    if btn(ui, "Install", true, true) { act = Some(Act::Install); }
                    if btn(ui, "Back", true, false) { act = Some(Act::Back); }
                }
                Page::Confirm => {
                    if btn(ui, "Cancel", true, false) { act = Some(Act::Cancel); }
                    if btn(ui, "Uninstall", true, true) { act = Some(Act::Uninstall); }
                }
                Page::Done => { if btn(ui, "Finish", true, true) { act = Some(Act::Finish); } }
                Page::Failed | Page::Removed => { if btn(ui, "Close", true, true) { act = Some(Act::Finish); } }
                Page::Installing | Page::Removing => {}
            }
        });
        act
    }

    fn body(&mut self, ui: &mut egui::Ui) {
        match self.page {
            Page::License => {
                ui.label("Please read the following License Agreement. You must accept its terms before continuing.");
                ui.add_space(8.0);
                let h = (ui.available_height() - 76.0).max(120.0);
                egui::Frame::none().fill(tint(14)).rounding(RADIUS).inner_margin(12.0).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    egui::ScrollArea::vertical().max_height(h).auto_shrink([false, false]).show(ui, |ui| { ui.label(LICENSE_TEXT); });
                });
                ui.add_space(8.0);
                if radio(ui, self.accepted, "I accept the agreement").clicked() { self.accepted = true; }
                if radio(ui, !self.accepted, "I do not accept the agreement").clicked() { self.accepted = false; }
            }
            Page::Location => {
                ui.label("Z will be installed in the following folder.");
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.cfg.dir).desired_width(ui.available_width() - 110.0));
                    if ui.button("Browse").clicked() {
                        if let Some(d) = rfd::FileDialog::new().set_title("Install folder").pick_folder() { self.cfg.dir = d.display().to_string(); }
                    }
                });
                ui.add_space(8.0);
                let before = self.cfg.all_users;
                if checkbox(ui, &mut self.cfg.all_users, "Install for all users (needs administrator)").changed() && self.cfg.dir == install::default_dir(before) {
                    self.cfg.dir = install::default_dir(self.cfg.all_users);
                }
                ui.add_space(14.0);
                let need = install::Z_EXE.len() + if self.cfg.cli { install::ZC_EXE.len() } else { 0 } + 200_000;
                ui.label(format!("At least {} of free disk space is required.", mb(need)));
            }
            Page::Components => {
                ui.label("Select the components you want to install, then click Next.");
                ui.add_space(8.0);
                let opts = vec![
                    (Kind::Full, "Full installation".to_string()),
                    (Kind::Compact, "Compact installation (Z only)".to_string()),
                    (Kind::Custom, "Custom installation".to_string()),
                ];
                if let Some(k) = dropdown(ui, "kind", 380.0, &self.kind, &opts) {
                    self.kind = k;
                    if k == Kind::Full { self.cfg.cli = true; } else if k == Kind::Compact { self.cfg.cli = false; }
                }
                ui.add_space(10.0);
                egui::Frame::none().fill(tint(14)).rounding(RADIUS).inner_margin(14.0).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.set_min_height(120.0);
                    ui.horizontal(|ui| {
                        ui.add_enabled_ui(false, |ui| { let mut on = true; checkbox(ui, &mut on, "Z"); });
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| { ui.label(mb(install::Z_EXE.len())); });
                    });
                    ui.horizontal(|ui| {
                        if checkbox(ui, &mut self.cfg.cli, "Command-line tool (zc)").changed() {
                            self.kind = if self.cfg.cli { Kind::Full } else { Kind::Compact };
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| { ui.label(mb(install::ZC_EXE.len())); });
                    });
                });
            }
            Page::Tasks => {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    heading(ui, "Windows Explorer");
                    checkbox(ui, &mut self.cfg.ctx_archive, "Add extract and open commands to the right-click menu of archives");
                    checkbox(ui, &mut self.cfg.ctx_all, "Add \"Add to archive\" to the right-click menu of files and folders");
                    ui.add_space(10.0);
                    heading(ui, "Open these file types with Z");
                    checkbox(ui, &mut self.cfg.assoc[0], "ZIP (.zip)");
                    checkbox(ui, &mut self.cfg.assoc[1], "7-Zip (.7z)");
                    checkbox(ui, &mut self.cfg.assoc[2], "RAR (.rar)");
                    checkbox(ui, &mut self.cfg.assoc[3], "TAR (.tar .tgz .tbz2 .txz .tzst)");
                    checkbox(ui, &mut self.cfg.assoc[4], "Single-file compressed (.gz .bz2 .xz .zst)");
                    let any = self.cfg.assoc.iter().any(|a| *a);
                    ui.add_enabled_ui(any, |ui| { checkbox(ui, &mut self.cfg.set_default, "Set Z as the default app for the selected types"); });
                    ui.add_space(10.0);
                    heading(ui, "Command line");
                    ui.add_enabled_ui(self.cfg.cli, |ui| { checkbox(ui, &mut self.cfg.add_path, "Add zc to PATH"); });
                    ui.add_space(10.0);
                    heading(ui, "Shortcuts");
                    checkbox(ui, &mut self.cfg.start_menu, "Create a Start menu shortcut");
                    checkbox(ui, &mut self.cfg.desktop, "Create a desktop shortcut");
                });
            }
            Page::Installing | Page::Removing => {
                let (frac, text) = match &self.worker { Some(w) => (w.prog.fraction(), w.prog.label()), None => (0.0, String::new()) };
                ui.add_space(70.0);
                ui.label(text);
                ui.add_space(10.0);
                let w = ui.available_width();
                progress_bar(ui, frac, w);
            }
            Page::Done => {
                ui.add_space(40.0);
                ui.label("Z has been installed on your computer.");
                ui.add_space(14.0);
                checkbox(ui, &mut self.launch, "Launch Z");
            }
            Page::Failed => { ui.add_space(40.0); ui.label(self.error.clone()); }
            Page::Confirm => { ui.add_space(40.0); ui.label("Z and its Windows integration will be removed. Your archives are not touched."); }
            Page::Removed => { ui.add_space(40.0); ui.label("Z has been removed from this computer."); }
        }
    }
}

impl eframe::App for SetupApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        paint_background(ctx);
        if self.autostart { self.autostart = false; self.start(ctx); }
        self.poll();
        let busy = self.worker.is_some();
        if busy { ctx.request_repaint_after(Duration::from_millis(50)); }

        egui::TopBottomPanel::top("title").exact_height(34.0).frame(egui::Frame::none().fill(Color32::BLACK)).show(ctx, |ui| title_bar(ui, ctx, busy));
        let (t, sub) = self.titles();
        egui::TopBottomPanel::top("header").exact_height(72.0).frame(egui::Frame::none().fill(HEADER)).show(ctx, |ui| {
            let r = ui.max_rect();
            ui.painter().text(egui::pos2(r.left() + 28.0, r.center().y - 11.0), Align2::LEFT_CENTER, t, FontId::new(17.0, FontFamily::Name("medium".into())), TXT);
            ui.painter().text(egui::pos2(r.left() + 28.0, r.center().y + 12.0), Align2::LEFT_CENTER, sub, FontId::new(13.0, FontFamily::Proportional), TXT);
            paint_logo(ui.painter(), Rect::from_center_size(egui::pos2(r.right() - 44.0, r.center().y), vec2(44.0, 44.0)));
        });
        let mut act: Option<Act> = None;
        egui::TopBottomPanel::bottom("footer").exact_height(62.0).frame(egui::Frame::none().fill(FOOTER)).show(ctx, |ui| { act = self.footer(ui); });
        egui::CentralPanel::default().frame(egui::Frame::none().inner_margin(Margin::symmetric(28.0, 18.0))).show(ctx, |ui| self.body(ui));
        if let Some(a) = act { self.apply(a, ctx); }

        ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("window_border")))
            .rect_stroke(ctx.screen_rect().shrink(0.5), 0.0, Stroke::new(1.0_f32, tint(46)));
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] { [0.055, 0.043, 0.133, 1.0] }
}
