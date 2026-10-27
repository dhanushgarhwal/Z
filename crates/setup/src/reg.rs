//! Registry layout of Z: Explorer menus, file associations, Default apps capabilities and the
//! uninstall entry. Generated as .reg text and imported in one go.
use crate::install::{Config, VERSION};

/// (extension, association group index into `Config::assoc`)
pub const ASSOC: [(&str, usize); 12] = [
    ("zip", 0), ("7z", 1), ("rar", 2),
    ("tar", 3), ("tgz", 3), ("tbz2", 3), ("txz", 3), ("tzst", 3),
    ("gz", 4), ("bz2", 4), ("xz", 4), ("zst", 4),
];

const ADD: [(&str, &str); 4] = [
    ("Add to archive...", "a --dialog --collect \"%1\""),
    ("Add to .zip", "a -f zip --collect \"%1\""),
    ("Add to .7z", "a -f 7z --collect \"%1\""),
    ("Add to .tar.zst", "a -f tar.zst --collect \"%1\""),
];
const ARC: [(&str, &str); 4] = [
    ("Open with Z", "\"%1\""),
    ("Extract here", "x --here --collect \"%1\""),
    ("Extract to folder", "x --to-folder --collect \"%1\""),
    ("Extract files...", "x --ask --collect \"%1\""),
];

fn esc(s: &str) -> String { s.replace('\\', "\\\\").replace('"', "\\\"") }

pub struct RegFile { root: String, out: String }

impl RegFile {
    pub fn new(root: &str) -> Self {
        RegFile { root: root.to_string(), out: String::from("Windows Registry Editor Version 5.00\r\n\r\n") }
    }
    fn header(&mut self, sub: &str) {
        let line = format!("[{}\\{}]\r\n", self.root, sub);
        self.out.push_str(&line);
    }
    pub fn key(&mut self, sub: &str) { self.header(sub); self.out.push_str("\r\n"); }
    pub fn set(&mut self, sub: &str, name: Option<&str>, data: &str) {
        self.header(sub);
        let line = match name {
            Some(n) => format!("\"{}\"=\"{}\"\r\n\r\n", esc(n), esc(data)),
            None => format!("@=\"{}\"\r\n\r\n", esc(data)),
        };
        self.out.push_str(&line);
    }
    pub fn dword(&mut self, sub: &str, name: &str, v: u32) {
        self.header(sub);
        let line = format!("\"{}\"=dword:{:08x}\r\n\r\n", esc(name), v);
        self.out.push_str(&line);
    }
    pub fn del_key(&mut self, sub: &str) {
        let line = format!("[-{}\\{}]\r\n\r\n", self.root, sub);
        self.out.push_str(&line);
    }
    pub fn finish(self) -> String { self.out }
}

fn cascade(r: &mut RegFile, exe: &str, base: &str, items: &[(&str, &str)]) {
    r.key(base);
    r.set(base, Some("MUIVerb"), "Z");
    r.set(base, Some("Icon"), &format!("\"{}\",0", exe));
    r.set(base, Some("SubCommands"), "");
    for (i, (label, args)) in items.iter().enumerate() {
        let v = format!("{}\\shell\\{:02}", base, i);
        r.set(&v, Some("MUIVerb"), label);
        r.set(&format!("{}\\command", v), None, &format!("\"{}\" {}", exe, args));
    }
}

pub fn root_name(all_users: bool) -> &'static str {
    if all_users { "HKEY_LOCAL_MACHINE" } else { "HKEY_CURRENT_USER" }
}

pub fn uninstall_key() -> &'static str { "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Z" }

pub fn install_reg(cfg: &Config, size_kb: u32) -> String {
    let mut r = RegFile::new(root_name(cfg.all_users));
    let exe = format!("{}\\Z.exe", cfg.dir);
    if cfg.ctx_all {
        cascade(&mut r, &exe, "Software\\Classes\\*\\shell\\Z", &ADD);
        cascade(&mut r, &exe, "Software\\Classes\\Directory\\shell\\Z", &ADD);
    }
    if cfg.ctx_archive {
        for (ext, _) in ASSOC.iter() {
            cascade(&mut r, &exe, &format!("Software\\Classes\\SystemFileAssociations\\.{}\\shell\\Z", ext), &ARC);
        }
    }
    if cfg.assoc.iter().any(|a| *a) {
        let prog = "Software\\Classes\\Z.Archive";
        r.set(prog, None, "Z archive");
        r.set(&format!("{}\\DefaultIcon", prog), None, &format!("{}\\z-archive.ico", cfg.dir));
        r.set(&format!("{}\\shell\\open\\command", prog), None, &format!("\"{}\" \"%1\"", exe));
        for (ext, g) in ASSOC.iter() {
            if cfg.assoc[*g] {
                r.set(&format!("Software\\Classes\\.{}\\OpenWithProgids", ext), Some("Z.Archive"), "");
                let dotted = format!(".{}", ext);
                r.set("Software\\Z\\Capabilities\\FileAssociations", Some(dotted.as_str()), "Z.Archive");
            }
        }
        r.set("Software\\Z\\Capabilities", Some("ApplicationName"), "Z");
        r.set("Software\\Z\\Capabilities", Some("ApplicationDescription"), "Free, open-source archiver");
        r.set("Software\\Z\\Capabilities", Some("ApplicationIcon"), &format!("{},0", exe));
        r.set("Software\\RegisteredApplications", Some("Z"), "Software\\Z\\Capabilities");
    }
    let u = uninstall_key();
    r.set(u, Some("DisplayName"), "Z");
    r.set(u, Some("DisplayVersion"), VERSION);
    r.set(u, Some("Publisher"), "Z contributors");
    r.set(u, Some("DisplayIcon"), &exe);
    r.set(u, Some("InstallLocation"), &cfg.dir);
    r.set(u, Some("UninstallString"), &format!("\"{}\\Uninstall.exe\" --uninstall", cfg.dir));
    r.dword(u, "NoModify", 1);
    r.dword(u, "NoRepair", 1);
    r.dword(u, "EstimatedSize", size_kb);
    r.finish()
}

/// Makes Z the default handler of the chosen types (always per user: this is where Windows looks first).
pub fn default_reg(cfg: &Config) -> String {
    let mut r = RegFile::new("HKEY_CURRENT_USER");
    for (ext, g) in ASSOC.iter() {
        if cfg.assoc[*g] { r.set(&format!("Software\\Classes\\.{}", ext), None, "Z.Archive"); }
    }
    r.finish()
}

pub fn uninstall_reg(all_users: bool) -> String {
    let mut r = RegFile::new(root_name(all_users));
    r.del_key("Software\\Classes\\*\\shell\\Z");
    r.del_key("Software\\Classes\\Directory\\shell\\Z");
    for (ext, _) in ASSOC.iter() { r.del_key(&format!("Software\\Classes\\SystemFileAssociations\\.{}\\shell\\Z", ext)); }
    r.del_key("Software\\Classes\\Z.Archive");
    r.del_key("Software\\Z");
    r.del_key(uninstall_key());
    r.finish()
}
