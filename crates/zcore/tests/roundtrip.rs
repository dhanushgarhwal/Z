//! `cargo test -p zcore` : every writable format must round-trip byte-for-byte, and hostile paths
//! must never escape the destination.
use std::fs;
use std::path::{Path, PathBuf};
use zcore::{CreateOpts, ExtractOpts, Format, Level, Overwrite, Progress};

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ztest-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn sample(root: &Path) {
    fs::create_dir_all(root.join("src/sub")).unwrap();
    fs::write(root.join("src/a.txt"), "hello z ".repeat(5000)).unwrap();
    fs::write(root.join("src/sub/b.bin"), (0..=255u8).cycle().take(300_000).collect::<Vec<_>>()).unwrap();
    fs::write(root.join("src/empty"), b"").unwrap();
}

fn same(a: &Path, b: &Path) {
    for rel in ["a.txt", "sub/b.bin", "empty"] { assert_eq!(fs::read(a.join(rel)).unwrap(), fs::read(b.join(rel)).unwrap(), "{rel}"); }
}

fn roundtrip(fmt: Format, pw: Option<&str>) {
    // unique per (format, encrypted?) so tests running in parallel never share a folder
    let d = tmp(&format!("rt-{}-{}", fmt.extension(), if pw.is_some() { "enc" } else { "plain" }));
    sample(&d);
    let arc = d.join(format!("out.{}", fmt.extension()));
    let opts = CreateOpts { format: fmt, level: Level::Fast, password: pw.map(String::from), threads: 0 };
    zcore::create(&arc, &[d.join("src")], &opts, &Progress::new()).unwrap();
    assert_eq!(Format::detect(&arc), Some(fmt));
    let names: Vec<String> = zcore::list(&arc, pw).unwrap().into_iter().map(|e| e.path).collect();
    assert!(names.iter().any(|n| n == "src/a.txt"), "{names:?}");
    let out = d.join("out");
    let r = zcore::extract(&arc, &out, &ExtractOpts { password: pw.map(String::from), overwrite: Overwrite::Always, ..Default::default() }, &Progress::new()).unwrap();
    assert_eq!(r.files, 3);
    same(&d.join("src"), &out.join("src"));
    assert_eq!(zcore::test(&arc, pw, &Progress::new()).unwrap() > 0, true);
    let _ = fs::remove_dir_all(&d);
}

#[test] fn zip() { roundtrip(Format::Zip, None); }
#[test] fn zip_aes() { roundtrip(Format::Zip, Some("s3cret")); }
#[test] fn sevenz() { roundtrip(Format::SevenZ, None); }
#[test] fn sevenz_aes() { roundtrip(Format::SevenZ, Some("s3cret")); }
#[test] fn tar() { roundtrip(Format::Tar, None); }
#[test] fn tar_gz() { roundtrip(Format::TarGz, None); }
#[test] fn tar_bz2() { roundtrip(Format::TarBz2, None); }
#[test] fn tar_xz() { roundtrip(Format::TarXz, None); }
#[test] fn tar_zst() { roundtrip(Format::TarZst, None); }

#[test] fn selective_extract_only_writes_requested_entries() {
    let d = tmp("sel"); sample(&d);
    let arc = d.join("s.zip");
    zcore::create(&arc, &[d.join("src")], &CreateOpts::default(), &Progress::new()).unwrap();
    let out = d.join("out");
    zcore::extract(&arc, &out, &ExtractOpts { only: vec!["src/sub".into()], ..Default::default() }, &Progress::new()).unwrap();
    assert!(out.join("src/sub/b.bin").exists());
    assert!(!out.join("src/a.txt").exists());
    let _ = fs::remove_dir_all(&d);
}

#[test] fn zip_slip_cannot_escape() {
    use std::io::Write;
    let d = tmp("slip");
    let arc = d.join("evil.zip");
    {
        let mut w = zip::ZipWriter::new(fs::File::create(&arc).unwrap());
        let o = zip::write::SimpleFileOptions::default();
        for name in ["../evil.txt", "..\\evil2.txt", "/abs.txt", "ok.txt"] { w.start_file(name, o).unwrap(); w.write_all(b"x").unwrap(); }
        w.finish().unwrap();
    }
    let out = d.join("out");
    let r = zcore::extract(&arc, &out, &ExtractOpts::default(), &Progress::new()).unwrap();
    assert_eq!(r.files, 1);
    assert!(out.join("ok.txt").exists());
    assert!(!d.join("evil.txt").exists() && !d.join("evil2.txt").exists() && !Path::new("/abs.txt").exists());
    let _ = fs::remove_dir_all(&d);
}

#[test] fn overwrite_policies() {
    let d = tmp("ow"); sample(&d);
    let arc = d.join("o.zip");
    zcore::create(&arc, &[d.join("src")], &CreateOpts::default(), &Progress::new()).unwrap();
    let out = d.join("out");
    zcore::extract(&arc, &out, &ExtractOpts::default(), &Progress::new()).unwrap();
    zcore::extract(&arc, &out, &ExtractOpts { overwrite: Overwrite::Rename, ..Default::default() }, &Progress::new()).unwrap();
    assert!(out.join("src/a (1).txt").exists());
    let r = zcore::extract(&arc, &out, &ExtractOpts { overwrite: Overwrite::Skip, ..Default::default() }, &Progress::new()).unwrap();
    assert!(r.skipped.len() >= 3);
    let _ = fs::remove_dir_all(&d);
}

#[test] fn encrypted_zip_is_unreadable_without_the_password() {
    let d = tmp("pwreq"); sample(&d);
    let arc = d.join("p.zip");
    let opts = CreateOpts { format: Format::Zip, level: Level::Store, password: Some("s3cret".into()), threads: 0 };
    zcore::create(&arc, &[d.join("src")], &opts, &Progress::new()).unwrap();
    let raw = fs::read(&arc).unwrap();
    let needle = b"hello z hello z";
    assert!(!raw.windows(needle.len()).any(|w| w == needle), "plain text found inside an encrypted archive");
    assert!(zcore::list(&arc, None).is_err());
    let out = d.join("out");
    assert!(zcore::extract(&arc, &out, &ExtractOpts::default(), &Progress::new()).is_err());
    assert!(!out.join("src/a.txt").exists());
    assert!(zcore::list(&arc, Some("s3cret")).is_ok());
    let _ = fs::remove_dir_all(&d);
}
