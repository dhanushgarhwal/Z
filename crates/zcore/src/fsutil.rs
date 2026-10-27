use anyhow::{bail, Result};
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Join an archive-supplied relative path onto `base`, refusing anything that could escape it
/// (`..`, absolute paths, drive letters, UNC prefixes, NUL bytes). Returns None when unsafe.
pub fn safe_join(base: &Path, rel: &str) -> Option<PathBuf> {
    if rel.contains('\0') { return None; }
    let norm = rel.replace('\\', "/");
    let mut out = base.to_path_buf();
    let mut any = false;
    for comp in Path::new(&norm).components() {
        match comp {
            Component::Normal(c) => {
                let s = c.to_string_lossy();
                if s.contains(':') { return None; }          // NTFS alternate streams / drive letters
                out.push(c); any = true;
            }
            Component::CurDir => {}
            _ => return None,                                 // ParentDir, RootDir, Prefix
        }
    }
    if any { Some(out) } else { None }
}

/// `name.ext` -> `name (1).ext`, `name (2).ext`, ... until unused.
pub fn unique_path(p: &Path) -> PathBuf {
    if !p.exists() { return p.to_path_buf(); }
    let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = p.extension().map(|s| format!(".{}", s.to_string_lossy())).unwrap_or_default();
    for i in 1..100_000 {
        let c = p.with_file_name(format!("{stem} ({i}){ext}"));
        if !c.exists() { return c; }
    }
    p.to_path_buf()
}

pub fn civil_to_unix(y: i32, m: u32, d: u32, hh: u32, mm: u32, ss: u32) -> i64 {
    // Howard Hinnant's days_from_civil
    let y: i64 = (if m <= 2 { y - 1 } else { y }) as i64;
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    days * 86_400 + hh as i64 * 3600 + mm as i64 * 60 + ss as i64
}

/// Convert a ZIP entry timestamp (absent or zeroed DOS dates -> 0 = unknown).
pub fn zip_time(dt: Option<zip::DateTime>) -> i64 {
    match dt {
        Some(dt) if dt.month() >= 1 && dt.day() >= 1 => civil_to_unix(
            dt.year() as i32, dt.month() as u32, dt.day() as u32,
            dt.hour() as u32, dt.minute() as u32, dt.second() as u32),
        _ => 0,
    }
}

pub fn unix_to_civil(t: i64) -> (i32, u32, u32, u32, u32, u32) {
    let days = t.div_euclid(86_400);
    let rem = t.rem_euclid(86_400);
    let z = days + 719_468;
    let era = (if z >= 0 { z } else { z - 146_096 }) / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let y = (if m <= 2 { y + 1 } else { y }) as i32;
    (y, m, d, (rem / 3600) as u32, (rem % 3600 / 60) as u32, (rem % 60) as u32)
}

pub fn set_mtime(path: &Path, unix: i64) {
    if unix <= 0 { return; }
    let t: SystemTime = UNIX_EPOCH + Duration::from_secs(unix as u64);
    if let Ok(f) = std::fs::OpenOptions::new().write(true).open(path) { let _ = f.set_modified(t); }
}

pub fn human_size(n: u64) -> String {
    const U: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = n as f64; let mut i = 0;
    while v >= 1024.0 && i < 4 { v /= 1024.0; i += 1; }
    if i == 0 { format!("{n} B") } else { format!("{v:.1} {}", U[i]) }
}

pub fn ensure_parent(p: &Path) -> Result<()> {
    match p.parent() {
        Some(par) if !par.as_os_str().is_empty() => { std::fs::create_dir_all(par)?; Ok(()) }
        Some(_) | None => Ok(()),
    }
}

pub fn require_dir(p: &Path) -> Result<()> {
    if p.exists() && !p.is_dir() { bail!("{} exists and is not a folder", p.display()); }
    std::fs::create_dir_all(p)?; Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn blocks_traversal() {
        let b = Path::new("/out");
        assert!(safe_join(b, "../x").is_none());
        assert!(safe_join(b, "a/../../x").is_none());
        assert!(safe_join(b, "/etc/passwd").is_none());
        assert!(safe_join(b, "C:\\Windows\\x").is_none());
        assert!(safe_join(b, "a\\..\\..\\x").is_none());
        assert!(safe_join(b, "file.txt:stream").is_none());
        assert_eq!(safe_join(b, "a/b.txt").unwrap(), Path::new("/out/a/b.txt"));
        assert_eq!(safe_join(b, "a\\b.txt").unwrap(), Path::new("/out/a/b.txt"));
    }
    #[test] fn time_roundtrip() {
        let t = civil_to_unix(2026, 10, 26, 12, 34, 56);
        assert_eq!(unix_to_civil(t), (2026, 10, 26, 12, 34, 56));
        assert_eq!(civil_to_unix(1970, 1, 1, 0, 0, 0), 0);
    }
}
