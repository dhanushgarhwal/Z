use crate::format::Format;
use crate::fsutil::unix_to_civil;
use crate::progress::{CountingReader, Progress};
use anyhow::{anyhow, bail, Result};
use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;
use walkdir::WalkDir;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level { Store, Fast, Normal, Max }

#[derive(Clone, Debug)]
pub struct CreateOpts {
    pub format: Format,
    pub level: Level,
    pub password: Option<String>,
    /// 0 = automatic (used by zstd's built-in multithreading).
    pub threads: usize,
}

impl Default for CreateOpts {
    fn default() -> Self { Self { format: Format::Zip, level: Level::Normal, password: None, threads: 0 } }
}

struct Item { path: PathBuf, name: String, is_dir: bool, size: u64, mtime: i64 }

fn gather(inputs: &[PathBuf], dest: &Path) -> Result<Vec<Item>> {
    let dest_abs = fs::canonicalize(dest).ok();
    let mut out = Vec::new();
    for input in inputs {
        let base = input.parent().map(Path::to_path_buf).unwrap_or_default();
        for e in WalkDir::new(input).follow_links(false).sort_by_file_name() {
            let e = e.map_err(|e| anyhow!("{e}"))?;
            if e.path_is_symlink() { continue; }                         // never follow or store links
            if let (Some(d), Ok(c)) = (&dest_abs, fs::canonicalize(e.path())) { if &c == d { continue; } }
            let rel = e.path().strip_prefix(&base).unwrap_or(e.path());
            let name = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/");
            if name.is_empty() { continue; }
            let meta = e.metadata().map_err(|e| anyhow!("{e}"))?;
            let mtime = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64).unwrap_or(0);
            out.push(Item { path: e.path().to_path_buf(), name, is_dir: meta.is_dir(), size: if meta.is_dir() { 0 } else { meta.len() }, mtime });
        }
    }
    Ok(out)
}

/// A sensible default archive name next to the input, the way WinRAR / 7-Zip name them.
pub fn suggest_name(inputs: &[PathBuf], fmt: Format) -> PathBuf {
    let first = &inputs[0];
    let dir = first.parent().map(Path::to_path_buf).unwrap_or_default();
    let stem = if inputs.len() == 1 {
        if first.is_dir() { first.file_name() } else { first.file_stem() }
            .map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Archive".into())
    } else {
        dir.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Archive".into())
    };
    dir.join(format!("{stem}.{}", fmt.extension()))
}

pub fn create(dest: &Path, inputs: &[PathBuf], opts: &CreateOpts, p: &Progress) -> Result<PathBuf> {
    if !opts.format.can_write() { bail!("Z cannot create .{} archives (the RAR compressor is proprietary).", opts.format.extension()); }
    if inputs.is_empty() { bail!("Nothing to add"); }
    let items = gather(inputs, dest)?;
    if items.is_empty() { bail!("Nothing to add"); }
    p.set_total(items.iter().map(|i| i.size).sum());
    crate::fsutil::ensure_parent(dest)?;
    let tmp = PathBuf::from(format!("{}.part", dest.display()));       // never leave half-written archives behind
    let res = create_inner(&tmp, &items, opts, p);
    match res {
        Ok(()) if !p.cancelled() => {
            if dest.exists() { fs::remove_file(dest)?; }
            fs::rename(&tmp, dest)?;
            Ok(dest.to_path_buf())
        }
        Ok(()) => { let _ = fs::remove_file(&tmp); bail!("Cancelled") }
        Err(e) => { let _ = fs::remove_file(&tmp); Err(e) }
    }
}

fn lvl(level: Level, fast: u32, normal: u32, max: u32) -> u32 {
    match level { Level::Store | Level::Fast => fast, Level::Normal => normal, Level::Max => max }
}

fn create_inner(tmp: &Path, items: &[Item], o: &CreateOpts, p: &Progress) -> Result<()> {
    let file = File::create(tmp)?;
    let w = BufWriter::with_capacity(1 << 20, file);
    let threads = if o.threads == 0 { std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2) as u32 } else { o.threads as u32 };
    match o.format {
        Format::Zip => zip_create(w, items, o, p),
        Format::SevenZ => { drop(w); sevenz_create(tmp, items, o, p) }
        Format::Tar => { let mut w = write_tar(w, items, p)?; w.flush()?; Ok(()) }
        Format::TarGz => {
            let enc = flate2::write::GzEncoder::new(w, flate2::Compression::new(lvl(o.level, 1, 6, 9)));
            let mut w = write_tar(enc, items, p)?.finish()?; w.flush()?; Ok(())
        }
        Format::TarBz2 => {
            let enc = bzip2::write::BzEncoder::new(w, bzip2::Compression::new(lvl(o.level, 1, 6, 9)));
            let mut w = write_tar(enc, items, p)?.finish()?; w.flush()?; Ok(())
        }
        Format::TarXz => {
            let enc = xz2::write::XzEncoder::new(w, lvl(o.level, 1, 6, 9));
            let mut w = write_tar(enc, items, p)?.finish()?; w.flush()?; Ok(())
        }
        Format::TarZst => {
            let mut enc = zstd::Encoder::new(w, lvl(o.level, 1, 6, 19) as i32)?;
            enc.multithread(threads)?;
            let mut w = write_tar(enc, items, p)?.finish()?; w.flush()?; Ok(())
        }
        Format::Gz | Format::Bz2 | Format::Xz | Format::Zst => single_create(w, items, o, threads, p),
        Format::Rar => unreachable!(),
    }
}

fn open_counted<'a>(path: &Path, p: &'a Progress) -> Result<CountingReader<'a, BufReader<File>>> {
    Ok(CountingReader { inner: BufReader::with_capacity(256 * 1024, File::open(path)?), p })
}

fn write_tar<W: Write>(w: W, items: &[Item], p: &Progress) -> Result<W> {
    let mut b = tar::Builder::new(w);
    b.follow_symlinks(false);
    for it in items {
        if p.cancelled() { break; }
        p.set_current(&it.name);
        if it.is_dir { b.append_dir(&it.name, &it.path)?; continue; }
        let meta = fs::metadata(&it.path)?;
        let mut h = tar::Header::new_gnu();
        h.set_metadata(&meta);
        let mut rd = open_counted(&it.path, p)?;
        b.append_data(&mut h, &it.name, &mut rd)?;
    }
    Ok(b.into_inner()?)
}

fn zip_create<W: Write + io::Seek>(w: W, items: &[Item], o: &CreateOpts, p: &Progress) -> Result<()> {
    use zip::{write::SimpleFileOptions, CompressionMethod, DateTime, ZipWriter};
    let mut zw = ZipWriter::new(w);
    let (method, level) = match o.level {
        Level::Store => (CompressionMethod::Stored, None),
        Level::Fast => (CompressionMethod::Deflated, Some(1)),
        Level::Normal => (CompressionMethod::Deflated, Some(6)),
        Level::Max => (CompressionMethod::Deflated, Some(9)),
    };
    for it in items {
        if p.cancelled() { break; }
        p.set_current(&it.name);
        let (y, mo, d, h, mi, s) = unix_to_civil(it.mtime.max(315_532_800));
        let mut opt = SimpleFileOptions::default().compression_method(method).large_file(it.size >= 0xFFFF_FFFF);
        if let Ok(dt) = DateTime::from_date_and_time(y as u16, mo as u8, d as u8, h as u8, mi as u8, s as u8) { opt = opt.last_modified_time(dt); }
        if level.is_some() { opt = opt.compression_level(level); }
        if let Some(pw) = &o.password { opt = opt.with_aes_encryption(zip::AesMode::Aes256, pw); }
        if it.is_dir { zw.add_directory(&it.name, opt)?; continue; }
        zw.start_file(&it.name, opt)?;
        let mut rd = open_counted(&it.path, p)?;
        io::copy(&mut rd, &mut zw)?;
    }
    zw.finish()?.flush()?;
    Ok(())
}

fn sevenz_create(tmp: &Path, items: &[Item], o: &CreateOpts, p: &Progress) -> Result<()> {
    use sevenz_rust::{lzma::LZMA2Options, AesEncoderOptions, Password, SevenZArchiveEntry, SevenZWriter};
    let mut w = SevenZWriter::create(tmp).map_err(|e| anyhow!("7z: {e}"))?;
    let lz = LZMA2Options::with_preset(lvl(o.level, 1, 5, 9));
    match &o.password {
        Some(pw) => w.set_content_methods(vec![AesEncoderOptions::new(Password::from(pw.as_str())).into(), lz.into()]),
        None => w.set_content_methods(vec![lz.into()]),
    };
    if o.password.is_some() { w.set_encrypt_header(true); }     // file names are encrypted too
    for it in items {
        if p.cancelled() { break; }
        p.set_current(&it.name);
        let entry = SevenZArchiveEntry::from_path(&it.path, it.name.clone());
        if it.is_dir { w.push_archive_entry::<&[u8]>(entry, None).map_err(|e| anyhow!("7z: {e}"))?; }
        else { w.push_archive_entry(entry, Some(open_counted(&it.path, p)?)).map_err(|e| anyhow!("7z: {e}"))?; }
    }
    w.finish().map_err(|e| anyhow!("7z: {e}"))?;
    Ok(())
}

fn single_create(w: BufWriter<File>, items: &[Item], o: &CreateOpts, threads: u32, p: &Progress) -> Result<()> {
    let files: Vec<&Item> = items.iter().filter(|i| !i.is_dir).collect();
    if files.len() != 1 || items.len() != 1 {
        bail!(".{} holds exactly one file. Choose tar.{} (or zip / 7z) for folders and multiple files.", o.format.extension(), o.format.extension());
    }
    let mut rd = open_counted(&files[0].path, p)?;
    let mut w = match o.format {
        Format::Gz => { let mut e = flate2::write::GzEncoder::new(w, flate2::Compression::new(lvl(o.level, 1, 6, 9))); io::copy(&mut rd, &mut e)?; e.finish()? }
        Format::Bz2 => { let mut e = bzip2::write::BzEncoder::new(w, bzip2::Compression::new(lvl(o.level, 1, 6, 9))); io::copy(&mut rd, &mut e)?; e.finish()? }
        Format::Xz => { let mut e = xz2::write::XzEncoder::new(w, lvl(o.level, 1, 6, 9)); io::copy(&mut rd, &mut e)?; e.finish()? }
        _ => { let mut e = zstd::Encoder::new(w, lvl(o.level, 1, 6, 19) as i32)?; e.multithread(threads)?; io::copy(&mut rd, &mut e)?; e.finish()? }
    };
    w.flush()?;
    Ok(())
}
