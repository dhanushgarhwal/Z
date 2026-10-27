use crate::format::Format;
use crate::fsutil::{ensure_parent, require_dir, safe_join, set_mtime, unique_path, zip_time};
use crate::list::{is_tar_family, stream_reader};
use crate::progress::{CountingReader, Progress};
use anyhow::{anyhow, bail, Context, Result};
use std::fs::File;
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Overwrite { Always, Skip, Rename }

#[derive(Clone, Debug)]
pub struct ExtractOpts {
    pub password: Option<String>,
    pub overwrite: Overwrite,
    /// Entry paths (files or folders) to extract. Empty = everything.
    pub only: Vec<String>,
    /// 0 = automatic.
    pub threads: usize,
}

impl Default for ExtractOpts {
    fn default() -> Self { Self { password: None, overwrite: Overwrite::Rename, only: vec![], threads: 0 } }
}

#[derive(Debug, Default, Clone)]
pub struct Report { pub files: u64, pub bytes: u64, pub skipped: Vec<String>, pub cancelled: bool }

fn selected(only: &[String], rel: &str) -> bool {
    if only.is_empty() { return true; }
    let rel = rel.trim_end_matches('/');
    only.iter().any(|o| { let o = o.trim_end_matches('/'); rel == o || (rel.len() > o.len() && rel.starts_with(o) && rel.as_bytes()[o.len()] == b'/') })
}

fn resolve_target(p: PathBuf, ow: Overwrite) -> Option<PathBuf> {
    if !p.exists() { return Some(p); }
    match ow { Overwrite::Always => Some(p), Overwrite::Skip => None, Overwrite::Rename => Some(unique_path(&p)) }
}

/// Copy while counting *uncompressed* bytes (zip and 7z, where totals are known up front).
fn copy_counted<R: Read + ?Sized, W: Write + ?Sized>(r: &mut R, w: &mut W, p: &Progress) -> Result<u64> {
    let mut buf = vec![0u8; 256 * 1024];
    let mut total = 0u64;
    loop {
        if p.cancelled() { break; }
        let n = r.read(&mut buf)?;
        if n == 0 { break; }
        w.write_all(&buf[..n])?;
        p.add(n as u64);
        total += n as u64;
    }
    Ok(total)
}

fn make_writer(path: &Path) -> Result<BufWriter<File>> {
    ensure_parent(path)?;
    Ok(BufWriter::with_capacity(256 * 1024, File::create(path).with_context(|| format!("Cannot write {}", path.display()))?))
}

pub fn extract(archive: &Path, dest: &Path, opts: &ExtractOpts, p: &Progress) -> Result<Report> {
    let fmt = Format::detect(archive).ok_or_else(|| anyhow!("Unrecognised archive format"))?;
    require_dir(dest)?;
    let res = match fmt {
        Format::Zip => extract_zip(archive, dest, opts, p),
        Format::SevenZ => extract_7z(archive, dest, opts, p),
        Format::Rar => extract_rar(archive, dest, opts, p),
        f if is_tar_family(f) => extract_tar(archive, f, dest, opts, p),
        f => extract_single(archive, f, dest, opts, p),
    };
    match res {
        Ok(mut r) => { r.cancelled = p.cancelled(); Ok(r) }
        Err(_) if p.cancelled() => Ok(Report { cancelled: true, ..Default::default() }),
        Err(e) => Err(e),
    }
}

// ---------------------------------------------------------------- ZIP (multi-threaded)

fn extract_zip(path: &Path, dest: &Path, opts: &ExtractOpts, p: &Progress) -> Result<Report> {
    // One pass over the central directory to choose entries and balance work.
    let mut jobs: Vec<(usize, u64)> = Vec::new();
    {
        let mut ar = zip::ZipArchive::new(BufReader::new(File::open(path)?)).context("Not a valid ZIP")?;
        for i in 0..ar.len() {
            let f = ar.by_index_raw(i)?;
            if selected(&opts.only, f.name()) { jobs.push((i, f.size())); }
        }
    }
    let total: u64 = jobs.iter().map(|j| j.1).sum();
    p.set_total(total);
    let auto = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2).min(8);
    let mut threads = if opts.threads == 0 { auto } else { opts.threads };
    if jobs.len() < 32 || total < 8 * 1024 * 1024 { threads = 1; }
    threads = threads.max(1);

    // Longest-processing-time-first assignment: biggest files go to the least-loaded thread.
    jobs.sort_by(|a, b| b.1.cmp(&a.1));
    let mut buckets: Vec<Vec<usize>> = vec![Vec::new(); threads];
    let mut load = vec![0u64; threads];
    for (i, sz) in jobs {
        let t = (0..threads).min_by_key(|&t| load[t]).unwrap_or(0);
        buckets[t].push(i); load[t] += sz.max(1);
    }

    let report = Mutex::new(Report::default());
    let first_err: Mutex<Option<anyhow::Error>> = Mutex::new(None);
    std::thread::scope(|s| {
        for b in &buckets {
            if b.is_empty() { continue; }
            let (report, first_err) = (&report, &first_err);
            s.spawn(move || {
                if let Err(e) = zip_worker(path, b, dest, opts, p, report) {
                    p.cancel();                                    // stop siblings
                    if let Ok(mut g) = first_err.lock() { if g.is_none() { *g = Some(e); } }
                }
            });
        }
    });
    if let Some(e) = first_err.into_inner().ok().flatten() { return Err(e); }
    Ok(report.into_inner().unwrap_or_default())
}

fn zip_worker(path: &Path, idxs: &[usize], dest: &Path, opts: &ExtractOpts, p: &Progress, rep: &Mutex<Report>) -> Result<()> {
    let mut ar = zip::ZipArchive::new(BufReader::with_capacity(64 * 1024, File::open(path)?))?;
    for &i in idxs {
        if p.cancelled() { break; }
        let mut f = match &opts.password {
            Some(pw) => ar.by_index_decrypt(i, pw.as_bytes()).map_err(|e| anyhow!("{e} (wrong password?)"))?,
            None => ar.by_index(i).map_err(|e| anyhow!("{e}"))?,
        };
        let name = f.name().to_string();
        p.set_current(&name);
        let Some(target) = safe_join(dest, &name) else { push_skip(rep, &name); continue };
        if f.is_dir() { std::fs::create_dir_all(&target)?; continue; }
        if let Some(m) = f.unix_mode() { if m & 0o170000 == 0o120000 { push_skip(rep, &name); continue; } } // never create symlinks
        let mtime = zip_time(f.last_modified());
        let Some(out) = resolve_target(target, opts.overwrite) else { push_skip(rep, &name); p.add(f.size()); continue };
        let mut w = make_writer(&out)?;
        let n = copy_counted(&mut f, &mut w, p).with_context(|| format!("{name}: corrupt data or wrong password"))?;
        w.flush()?; drop(w);
        set_mtime(&out, mtime);
        if let Ok(mut r) = rep.lock() { r.files += 1; r.bytes += n; }
    }
    Ok(())
}

fn push_skip(rep: &Mutex<Report>, name: &str) { if let Ok(mut r) = rep.lock() { r.skipped.push(name.to_string()); } }

// ---------------------------------------------------------------- TAR family

fn extract_tar(path: &Path, fmt: Format, dest: &Path, opts: &ExtractOpts, p: &Progress) -> Result<Report> {
    let file = File::open(path)?;
    p.set_total(file.metadata()?.len());
    let rd = stream_reader(fmt, Box::new(CountingReader { inner: file, p }))?;
    let mut ar = tar::Archive::new(rd);
    let mut rep = Report::default();
    for e in ar.entries()? {
        let mut e = e?;
        let rel = e.path()?.to_string_lossy().into_owned();
        if !selected(&opts.only, &rel) { continue; }
        p.set_current(&rel);
        let Some(target) = safe_join(dest, &rel) else { rep.skipped.push(rel); continue };
        let ty = e.header().entry_type();
        if ty.is_dir() { std::fs::create_dir_all(&target)?; continue; }
        if !ty.is_file() { rep.skipped.push(rel); continue; }       // symlinks, hardlinks, devices: skipped on purpose
        let mtime = e.header().mtime().unwrap_or(0) as i64;
        let Some(out) = resolve_target(target, opts.overwrite) else { rep.skipped.push(rel); continue };
        let mut w = make_writer(&out)?;
        let n = io::copy(&mut e, &mut w)?;
        w.flush()?; drop(w);
        set_mtime(&out, mtime);
        rep.files += 1; rep.bytes += n;
    }
    Ok(rep)
}

// ---------------------------------------------------------------- .gz .bz2 .xz .zst

fn extract_single(path: &Path, fmt: Format, dest: &Path, opts: &ExtractOpts, p: &Progress) -> Result<Report> {
    let file = File::open(path)?;
    p.set_total(file.metadata()?.len());
    let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "data".into());
    p.set_current(&name);
    let mut rep = Report::default();
    let Some(target) = safe_join(dest, &name) else { bail!("Unsafe output name") };
    let Some(out) = resolve_target(target, opts.overwrite) else { rep.skipped.push(name); return Ok(rep) };
    let mut rd = stream_reader(fmt, Box::new(CountingReader { inner: file, p }))?;
    let mut w = make_writer(&out)?;
    rep.bytes = io::copy(&mut rd, &mut w)?;
    w.flush()?;
    rep.files = 1;
    Ok(rep)
}

// ---------------------------------------------------------------- 7z

fn extract_7z(path: &Path, dest: &Path, opts: &ExtractOpts, p: &Progress) -> Result<Report> {
    use sevenz_rust::{Password, SevenZReader};
    let pw = opts.password.as_deref().map(Password::from).unwrap_or_else(Password::empty);
    let mut sz = SevenZReader::open(path, pw).map_err(|e| anyhow!("7z: {e}"))?;
    let total: u64 = sz.archive().files.iter().filter(|f| !f.is_directory() && selected(&opts.only, f.name())).map(|f| f.size()).sum();
    p.set_total(total);
    let mut rep = Report::default();
    sz.for_each_entries(&mut |entry: &sevenz_rust::SevenZArchiveEntry, reader: &mut dyn std::io::Read| -> Result<bool, sevenz_rust::Error> {
        if p.cancelled() { return Ok(false); }
        let name = entry.name().replace('\\', "/");
        if !selected(&opts.only, &name) { io::copy(reader, &mut io::sink())?; return Ok(true); }
        p.set_current(&name);
        let Some(target) = safe_join(dest, &name) else { io::copy(reader, &mut io::sink())?; rep.skipped.push(name); return Ok(true) };
        if entry.is_directory() { std::fs::create_dir_all(&target)?; return Ok(true); }
        let Some(out) = resolve_target(target, opts.overwrite) else { io::copy(reader, &mut io::sink())?; rep.skipped.push(name); return Ok(true) };
        let mut w = make_writer(&out).map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
        let n = copy_counted(reader, &mut w, p).map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
        w.flush()?;
        rep.files += 1; rep.bytes += n;
        Ok(true)
    }).map_err(|e| anyhow!("7z: {e} (wrong password?)"))?;
    Ok(rep)
}

// ---------------------------------------------------------------- RAR (optional feature)

#[cfg(feature = "rar")]
fn extract_rar(path: &Path, dest: &Path, opts: &ExtractOpts, p: &Progress) -> Result<Report> {
    let mut ar = match &opts.password {
        Some(pw) => unrar::Archive::with_password(path, pw.as_bytes()),
        None => unrar::Archive::new(path),
    }.open_for_processing().map_err(|e| anyhow!("rar: {e}"))?;
    let mut rep = Report::default();
    while let Some(h) = ar.read_header().map_err(|e| anyhow!("rar: {e}"))? {
        let name = h.entry().filename.to_string_lossy().replace('\\', "/");
        let ok = selected(&opts.only, &name) && safe_join(dest, &name).is_some() && h.entry().is_file();
        if p.cancelled() { break; }
        ar = if ok {
            p.set_current(&name);
            rep.files += 1; rep.bytes += h.entry().unpacked_size;
            h.extract_with_base(dest).map_err(|e| anyhow!("rar: {e}"))?
        } else { h.skip().map_err(|e| anyhow!("rar: {e}"))? };
    }
    Ok(rep)
}
#[cfg(not(feature = "rar"))]
fn extract_rar(_: &Path, _: &Path, _: &ExtractOpts, _: &Progress) -> Result<Report> {
    Err(anyhow!("This build of Z was compiled without RAR extraction (cargo feature `rar`)."))
}

// ---------------------------------------------------------------- integrity test

/// Decode everything to nowhere; CRC / MAC failures surface as errors. Returns the file count.
pub fn test(archive: &Path, password: Option<&str>, p: &Progress) -> Result<u64> {
    let fmt = Format::detect(archive).ok_or_else(|| anyhow!("Unrecognised archive format"))?;
    let mut n = 0u64;
    match fmt {
        Format::Zip => {
            let mut ar = zip::ZipArchive::new(BufReader::new(File::open(archive)?))?;
            let total: u64 = (0..ar.len()).filter_map(|i| ar.by_index_raw(i).ok().map(|f| f.size())).sum();
            p.set_total(total);
            for i in 0..ar.len() {
                if p.cancelled() { break; }
                let mut f = match password { Some(pw) => ar.by_index_decrypt(i, pw.as_bytes())?, None => ar.by_index(i)? };
                p.set_current(f.name());
                if f.is_dir() { continue; }
                let name = f.name().to_string();
                copy_counted(&mut f, &mut io::sink(), p).with_context(|| format!("{name}: failed integrity check"))?;
                n += 1;
            }
        }
        Format::SevenZ => {
            use sevenz_rust::{Password, SevenZReader};
            let pw = password.map(Password::from).unwrap_or_else(Password::empty);
            let mut sz = SevenZReader::open(archive, pw).map_err(|e| anyhow!("7z: {e}"))?;
            p.set_total(sz.archive().files.iter().map(|f| f.size()).sum());
            sz.for_each_entries(&mut |e: &sevenz_rust::SevenZArchiveEntry, r: &mut dyn std::io::Read| -> Result<bool, sevenz_rust::Error> {
                if p.cancelled() { return Ok(false); }
                p.set_current(e.name());
                copy_counted(r, &mut io::sink(), p).map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
                n += 1; Ok(true)
            }).map_err(|e| anyhow!("7z: {e}"))?;
        }
        Format::Rar => bail!("Integrity test for RAR is not implemented yet"),
        f => {
            let file = File::open(archive)?;
            p.set_total(file.metadata()?.len());
            let rd = stream_reader(f, Box::new(CountingReader { inner: file, p }))?;
            if is_tar_family(f) {
                let mut ar = tar::Archive::new(rd);
                for e in ar.entries()? { let mut e = e?; io::copy(&mut e, &mut io::sink())?; n += 1; }
            } else { let mut rd = rd; io::copy(&mut rd, &mut io::sink())?; n = 1; }
        }
    }
    Ok(n)
}
