#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
mod app;

use app::{Startup, ZApp};
use std::sync::Arc;
use z::cli::{self, Cmd};

fn main() -> eframe::Result<()> {
    let startup = match cli::parse(std::env::args_os().skip(1).collect()) {
        Ok(Cmd::Gui(None)) | Ok(Cmd::Help) | Ok(Cmd::Version) => Startup::Empty,
        Ok(Cmd::Gui(Some(p))) => Startup::Open(p),
        Ok(Cmd::List(p, _)) => Startup::Open(p),
        Ok(Cmd::Job(job, collect)) => {
            // Explorer starts one process per selected file; merge them into a single job.
            let job = if collect { match z::collect::merge(job) { Some(j) => j, None => return Ok(()) } } else { job };
            Startup::Job(job)
        }
        Err(e) => Startup::Error(e.to_string()),
    };
    let job_mode = matches!(startup, Startup::Job(_));
    let (w, h) = if job_mode { (460.0, 210.0) } else { (1040.0, 660.0) };
    let rgba = include_bytes!("../../../assets/icons/z-64.rgba").to_vec();
    let mut vp = eframe::egui::ViewportBuilder::default()
        .with_inner_size([w, h]).with_title("Z").with_drag_and_drop(true)
        .with_icon(Arc::new(eframe::egui::IconData { rgba, width: 64, height: 64 }));
    if job_mode { vp = vp.with_resizable(false).with_maximize_button(false); } else { vp = vp.with_min_inner_size([640.0, 380.0]); }
    let options = eframe::NativeOptions { viewport: vp, centered: true, ..Default::default() };
    eframe::run_native("Z", options, Box::new(move |cc| Ok(Box::new(ZApp::new(cc, startup)))))
}
