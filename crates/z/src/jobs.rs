use anyhow::{anyhow, Result};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering::Relaxed;
use zcore::{fsutil, CreateOpts, ExtractOpts, Overwrite, Progress};

#[derive(Clone, Debug)]
pub enum Dest { Here, SubFolder, Dir(PathBuf), Ask }

#[derive(Clone, Debug)]
pub enum Job {
    Extract { archives: Vec<PathBuf>, dest: Dest, password: Option<String>, overwrite: Overwrite, only: Vec<String> },
    Add { inputs: Vec<PathBuf>, out: Option<PathBuf>, opts: CreateOpts, dialog: bool },
    Test { archives: Vec<PathBuf>, password: Option<String> },
}

/// `photos.tar.gz` -> `photos`, `a.b.zip` -> `a.b`
pub fn archive_stem(p: &Path) -> String {
    let name = p.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let lower = name.to_ascii_lowercase();
    for ext in [".tar.gz", ".tar.bz2", ".tar.xz", ".tar.zst"] {
        if lower.ends_with(ext) { return name[..name.len() - ext.len()].to_string(); }
    }
    p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or(name)
}

pub fn run(job: &Job, p: &Progress) -> Result<String> {
    match job {
        Job::Extract { archives, dest, password, overwrite, only } => {
            let (mut files, mut bytes, mut skipped, mut last) = (0u64, 0u64, 0usize, PathBuf::new());
            for a in archives {
                p.done.store(0, Relaxed);
                let parent = a.parent().map(Path::to_path_buf).unwrap_or_default();
                let out = match dest {
                    Dest::Here => parent,
                    Dest::SubFolder => parent.join(archive_stem(a)),
                    Dest::Dir(d) => d.clone(),
                    Dest::Ask => return Err(anyhow!("No destination chosen")),
                };
                let opts = ExtractOpts { password: password.clone(), overwrite: *overwrite, only: only.clone(), threads: 0 };
                let r = zcore::extract(a, &out, &opts, p)?;
                if r.cancelled { return Err(anyhow!("Cancelled")); }
                files += r.files; bytes += r.bytes; skipped += r.skipped.len(); last = out;
            }
            let mut s = format!("Extracted {files} file(s), {} to {}", fsutil::human_size(bytes), last.display());
            if skipped > 0 { s += &format!("\n{skipped} item(s) skipped (existing files or unsafe paths)"); }
            Ok(s)
        }
        Job::Add { inputs, out, opts, .. } => {
            let target = match out {
                Some(o) => o.clone(),
                None => fsutil::unique_path(&zcore::create::suggest_name(inputs, opts.format)),
            };
            let made = zcore::create(&target, inputs, opts, p)?;
            let sz = std::fs::metadata(&made).map(|m| m.len()).unwrap_or(0);
            Ok(format!("Created {} ({})", made.display(), fsutil::human_size(sz)))
        }
        Job::Test { archives, password } => {
            let mut lines = Vec::new();
            for a in archives {
                p.done.store(0, Relaxed);
                let n = zcore::test(a, password.as_deref(), p)?;
                lines.push(format!("OK  {} ({n} file(s))", a.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()));
            }
            Ok(lines.join("\n"))
        }
    }
}
