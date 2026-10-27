use crate::jobs::{Dest, Job};
use anyhow::{anyhow, bail, Result};
use std::ffi::OsString;
use std::path::PathBuf;
use zcore::{CreateOpts, Format, Level, Overwrite};

pub enum Cmd {
    /// Open the browser window, optionally on an archive.
    Gui(Option<PathBuf>),
    /// Run a job. The bool is `--collect` (merge Explorer multi-selection into one job).
    Job(Job, bool),
    List(PathBuf, Option<String>),
    Help,
    Version,
}

pub const HELP: &str = "\
Z  archiver

  Z [archive]                 open an archive (GUI)
  zc x [opts] <archive>...    extract         --here | --to-folder | -o <dir> | --ask
  zc a [opts] <path>...       add to archive  -f <fmt> -l <0-3> -o <file> -p <pw> --dialog
  zc t [opts] <archive>...    test integrity
  zc l <archive>              list contents

  -p <password>   -y overwrite   -s skip existing   (default: rename)
  formats: zip 7z tar tar.gz tar.bz2 tar.xz tar.zst gz bz2 xz zst
";

pub fn parse_format(s: &str) -> Result<Format> {
    Ok(match s.to_ascii_lowercase().trim_start_matches('.') {
        "zip" => Format::Zip, "7z" => Format::SevenZ, "tar" => Format::Tar,
        "tar.gz" | "tgz" => Format::TarGz, "tar.bz2" | "tbz2" => Format::TarBz2,
        "tar.xz" | "txz" => Format::TarXz, "tar.zst" | "tzst" => Format::TarZst,
        "gz" => Format::Gz, "bz2" => Format::Bz2, "xz" => Format::Xz, "zst" => Format::Zst,
        other => bail!("Unknown format '{other}'"),
    })
}

pub fn parse(args: Vec<OsString>) -> Result<Cmd> {
    let mut it = args.into_iter().peekable();
    let Some(first) = it.next() else { return Ok(Cmd::Gui(None)) };
    let f = first.to_string_lossy().into_owned();
    if matches!(f.as_str(), "-h" | "--help" | "/?" | "help") { return Ok(Cmd::Help); }
    if matches!(f.as_str(), "-V" | "--version") { return Ok(Cmd::Version); }
    if !matches!(f.as_str(), "x" | "a" | "t" | "l") { return Ok(Cmd::Gui(Some(PathBuf::from(first)))); }

    let (mut dest, mut out, mut pw, mut fmt) = (Dest::SubFolder, None::<PathBuf>, None::<String>, Format::Zip);
    let (mut ow, mut level, mut collect, mut dialog) = (Overwrite::Rename, Level::Normal, false, false);
    let mut paths: Vec<PathBuf> = Vec::new();
    let mut only_flags_done = false;
    while let Some(a) = it.next() {
        let s = a.to_string_lossy().into_owned();
        if only_flags_done || !s.starts_with('-') || s == "-" { paths.push(PathBuf::from(a)); continue; }
        let mut val = |name: &str| -> Result<String> { it.next().map(|v| v.to_string_lossy().into_owned()).ok_or_else(|| anyhow!("{name} needs a value")) };
        match s.as_str() {
            "--" => only_flags_done = true,
            "--here" => dest = Dest::Here,
            "--to-folder" => dest = Dest::SubFolder,
            "--ask" => dest = Dest::Ask,
            "--collect" => collect = true,
            "--dialog" => dialog = true,
            "-y" => ow = Overwrite::Always,
            "-s" => ow = Overwrite::Skip,
            "-p" => pw = Some(val("-p")?),
            "-f" => fmt = parse_format(&val("-f")?)?,
            "-l" => level = match val("-l")?.as_str() { "0" => Level::Store, "1" => Level::Fast, "2" => Level::Normal, "3" => Level::Max, v => bail!("Bad level '{v}' (0-3)") },
            "-o" => { let v = PathBuf::from(val("-o")?); if f == "x" { dest = Dest::Dir(v) } else { out = Some(v) } }
            other => bail!("Unknown option {other}"),
        }
    }
    if paths.is_empty() { bail!("No input given.\n\n{HELP}"); }
    Ok(match f.as_str() {
        "x" => Cmd::Job(Job::Extract { archives: paths, dest, password: pw, overwrite: ow, only: vec![] }, collect),
        "t" => Cmd::Job(Job::Test { archives: paths, password: pw }, collect),
        "l" => Cmd::List(paths.remove(0), pw),
        _ => {
            if let Some(o) = &out { if let Some(of) = Format::from_name(&o.to_string_lossy()) { fmt = of; } }
            Cmd::Job(Job::Add { inputs: paths, out, opts: CreateOpts { format: fmt, level, password: pw, threads: 0 }, dialog }, collect)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn a(v: &[&str]) -> Vec<OsString> { v.iter().map(|s| OsString::from(*s)).collect() }
    #[test] fn bare_path_opens_gui() { assert!(matches!(parse(a(&["x.zip"])).unwrap(), Cmd::Gui(Some(_)))); }
    #[test] fn extract_here() {
        match parse(a(&["x", "--here", "a.zip"])).unwrap() { Cmd::Job(Job::Extract { dest: Dest::Here, .. }, false) => {}, _ => panic!() }
    }
    #[test] fn add_with_format_from_out_name() {
        match parse(a(&["a", "-o", "b.7z", "dir"])).unwrap() { Cmd::Job(Job::Add { opts, .. }, _) => assert_eq!(opts.format, Format::SevenZ), _ => panic!() }
    }
    #[test] fn unknown_option_errors() { assert!(parse(a(&["a", "--nope", "x"])).is_err()); }
}
