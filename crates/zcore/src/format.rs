use std::fs::File;
use std::io::Read;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format { Zip, SevenZ, Tar, TarGz, TarBz2, TarXz, TarZst, Gz, Bz2, Xz, Zst, Rar }

impl Format {
    pub fn can_write(self) -> bool { !matches!(self, Format::Rar) }   // RAR creation is proprietary
    pub fn extension(self) -> &'static str {
        match self {
            Format::Zip => "zip", Format::SevenZ => "7z", Format::Tar => "tar",
            Format::TarGz => "tar.gz", Format::TarBz2 => "tar.bz2", Format::TarXz => "tar.xz",
            Format::TarZst => "tar.zst", Format::Gz => "gz", Format::Bz2 => "bz2",
            Format::Xz => "xz", Format::Zst => "zst", Format::Rar => "rar",
        }
    }
    pub fn from_name(name: &str) -> Option<Format> {
        let n = name.to_ascii_lowercase();
        let ends = |s: &str| n.ends_with(s);
        Some(if ends(".tar.gz") || ends(".tgz") { Format::TarGz }
        else if ends(".tar.bz2") || ends(".tbz2") || ends(".tbz") { Format::TarBz2 }
        else if ends(".tar.xz") || ends(".txz") { Format::TarXz }
        else if ends(".tar.zst") || ends(".tzst") { Format::TarZst }
        else if ends(".tar") { Format::Tar }
        else if ends(".zip") || ends(".jar") || ends(".apk") || ends(".docx") || ends(".xlsx") || ends(".pptx") || ends(".epub") || ends(".odt") { Format::Zip }
        else if ends(".7z") { Format::SevenZ }
        else if ends(".gz") { Format::Gz } else if ends(".bz2") { Format::Bz2 }
        else if ends(".xz") { Format::Xz } else if ends(".zst") { Format::Zst }
        else if ends(".rar") { Format::Rar }
        else { return None })
    }

    /// Magic bytes first (a renamed file is still detected), file name second.
    pub fn detect(path: &Path) -> Option<Format> {
        let mut head = [0u8; 512];
        let n = File::open(path).ok()?.read(&mut head).ok()?;
        let h = &head[..n];
        let by_name = path.file_name().and_then(|s| s.to_str()).and_then(Format::from_name);
        if h.starts_with(b"PK\x03\x04") || h.starts_with(b"PK\x05\x06") { return Some(Format::Zip); }
        if h.starts_with(&[0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C]) { return Some(Format::SevenZ); }
        if h.starts_with(b"Rar!\x1A\x07") { return Some(Format::Rar); }
        if n >= 262 && &h[257..262] == b"ustar" { return Some(Format::Tar); }
        let tar_name = matches!(by_name, Some(Format::TarGz | Format::TarBz2 | Format::TarXz | Format::TarZst | Format::Tar));
        if h.starts_with(&[0x1F, 0x8B]) { return Some(if tar_name { Format::TarGz } else { Format::Gz }); }
        if h.starts_with(b"BZh")        { return Some(if tar_name { Format::TarBz2 } else { Format::Bz2 }); }
        if h.starts_with(&[0xFD, b'7', b'z', b'X', b'Z', 0]) { return Some(if tar_name { Format::TarXz } else { Format::Xz }); }
        if h.starts_with(&[0x28, 0xB5, 0x2F, 0xFD]) { return Some(if tar_name { Format::TarZst } else { Format::Zst }); }
        by_name
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn names() {
        assert_eq!(Format::from_name("a.TAR.GZ"), Some(Format::TarGz));
        assert_eq!(Format::from_name("a.tgz"), Some(Format::TarGz));
        assert_eq!(Format::from_name("a.7z"), Some(Format::SevenZ));
        assert_eq!(Format::from_name("a.txt"), None);
    }
}
