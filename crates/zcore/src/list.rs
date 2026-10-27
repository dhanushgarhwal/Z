use crate::format::Format;
use crate::fsutil::zip_time;
use anyhow::{anyhow, bail, Context, Result};
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

#[derive(Clone, Debug, Default)]
pub struct Entry {
    /// Forward-slash path inside the archive, no trailing slash.
    pub path: String,
    pub size: u64,
    pub packed: u64,
    /// Unix seconds, 0 when unknown.
    pub modified: i64,
    pub is_dir: bool,
    pub encrypted: bool,
}

/// Open any supported archive as a decompressing reader (single-stream and tar.* formats).
pub fn stream_reader<'a>(f: Format, src: Box<dyn Read + 'a>) -> Result<Box<dyn Read + 'a>> {
    let br = BufReader::with_capacity(256 * 1024, src);
    Ok(match f {
        Format::Tar => Box::new(br),
        Format::TarGz | Format::Gz => Box::new(flate2::read::MultiGzDecoder::new(br)),
        Format::TarBz2 | Format::Bz2 => Box::new(bzip2::read::MultiBzDecoder::new(br)),
        Format::TarXz | Format::Xz => Box::new(xz2::read::XzDecoder::new_multi_decoder(br)),
        Format::TarZst | Format::Zst => Box::new(zstd::stream::read::Decoder::with_buffer(br)?),
        other => return Err(anyhow!("{other:?} is not a stream format")),
    })
}

pub fn is_tar_family(f: Format) -> bool {
    matches!(f, Format::Tar | Format::TarGz | Format::TarBz2 | Format::TarXz | Format::TarZst)
}

pub fn list(path: &Path, password: Option<&str>) -> Result<Vec<Entry>> {
    let fmt = Format::detect(path).ok_or_else(|| anyhow!("Unrecognised archive format"))?;
    list_as(path, fmt, password)
}

pub fn list_as(path: &Path, fmt: Format, password: Option<&str>) -> Result<Vec<Entry>> {
    match fmt {
        Format::Zip => list_zip(path, password),
        Format::SevenZ => list_7z(path, password),
        Format::Rar => list_rar(path),
        f if is_tar_family(f) => list_tar(path, f),
        f => list_single(path, f),
    }
}

fn list_zip(path: &Path, password: Option<&str>) -> Result<Vec<Entry>> {
    let mut ar = zip::ZipArchive::new(BufReader::new(File::open(path)?)).context("Not a valid ZIP")?;
    let mut out = Vec::with_capacity(ar.len());
    let mut first_encrypted: Option<usize> = None;
    for i in 0..ar.len() {
        let f = ar.by_index_raw(i)?;              // raw read of the central directory only
        if f.encrypted() && !f.is_dir() && first_encrypted.is_none() { first_encrypted = Some(i); }
        out.push(Entry {
            path: f.name().trim_end_matches('/').replace('\\', "/"),
            size: f.size(),
            packed: f.compressed_size(),
            modified: zip_time(f.last_modified()),
            is_dir: f.is_dir(),
            encrypted: f.encrypted(),
        });
    }
    // An encrypted archive is never shown (or opened) without the right password.
    if let Some(i) = first_encrypted {
        let Some(pw) = password else { bail!("Password required") };
        let mut f = ar.by_index_decrypt(i, pw.as_bytes()).map_err(|_| anyhow!("Wrong password"))?;
        let mut probe = [0u8; 16];                // AES is verified on open, classic ZipCrypto only on read
        let _ = f.read(&mut probe).map_err(|_| anyhow!("Wrong password"))?;
    }
    Ok(out)
}

fn list_7z(path: &Path, password: Option<&str>) -> Result<Vec<Entry>> {
    use sevenz_rust::{Password, SevenZReader};
    let pw = password.map(Password::from).unwrap_or_else(Password::empty);
    let r = SevenZReader::open(path, pw).map_err(|e| {
        let m = e.to_string();
        let l = m.to_lowercase();
        if l.contains("password") || l.contains("aes") || l.contains("decrypt") {
            if password.is_some() { anyhow!("Wrong password") } else { anyhow!("Password required") }
        } else { anyhow!("7z: {m}") }
    })?;
    Ok(r.archive().files.iter().map(|f| Entry {
        path: f.name().trim_end_matches('/').replace('\\', "/"),
        size: f.size(),
        packed: 0,                  // 7z packs per solid block, not per file
        modified: 0,
        is_dir: f.is_directory(),
        encrypted: false,
    }).collect())
}

fn list_tar(path: &Path, f: Format) -> Result<Vec<Entry>> {
    let rd = stream_reader(f, Box::new(File::open(path)?))?;
    let mut ar = tar::Archive::new(rd);
    let mut out = Vec::new();
    for e in ar.entries()? {
        let e = e?;
        let h = e.header();
        out.push(Entry {
            path: e.path()?.to_string_lossy().trim_end_matches('/').replace('\\', "/"),
            size: h.size().unwrap_or(0),
            packed: 0,
            modified: h.mtime().unwrap_or(0) as i64,
            is_dir: h.entry_type().is_dir(),
            encrypted: false,
        });
    }
    Ok(out)
}

fn list_single(path: &Path, f: Format) -> Result<Vec<Entry>> {
    let meta = std::fs::metadata(path)?;
    let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "data".into());
    let mut size = 0;
    if f == Format::Gz {                                        // gzip trailer stores ISIZE (mod 2^32)
        use std::io::{Seek, SeekFrom};
        let mut fh = File::open(path)?;
        if meta.len() > 4 { fh.seek(SeekFrom::End(-4))?; let mut b = [0u8; 4]; fh.read_exact(&mut b)?; size = u32::from_le_bytes(b) as u64; }
    }
    Ok(vec![Entry { path: name, size, packed: meta.len(), modified: 0, is_dir: false, encrypted: false }])
}

#[cfg(feature = "rar")]
fn list_rar(path: &Path) -> Result<Vec<Entry>> {
    let ar = unrar::Archive::new(path).open_for_listing().map_err(|e| anyhow!("rar: {e}"))?;
    let mut out = Vec::new();
    for h in ar {
        let h = h.map_err(|e| anyhow!("rar: {e}"))?;
        out.push(Entry {
            path: h.filename.to_string_lossy().replace('\\', "/"),
            size: h.unpacked_size, packed: 0, modified: 0, is_dir: h.is_directory(), encrypted: false,
        });
    }
    Ok(out)
}
#[cfg(not(feature = "rar"))]
fn list_rar(_: &Path) -> Result<Vec<Entry>> {
    Err(anyhow!("This build of Z was compiled without RAR extraction (cargo feature `rar`)."))
}
