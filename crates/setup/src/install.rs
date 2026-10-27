//! What Z Setup actually does to the computer. Std only: files are written directly, the registry
//! is changed through one `reg import`, shortcuts and PATH through PowerShell.
use crate::reg;
use anyhow::{anyhow, bail, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering::Relaxed};
use std::sync::Mutex;
use std::time::Duration;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub static Z_EXE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/Z.exe"));
pub static ZC_EXE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/zc.exe"));
static ICON: &[u8] = include_bytes!("../../../assets/icons/z-archive.ico");
static LICENSE: &[u8] = include_bytes!("../../../LICENSE");
static FONT_LICENSE: &[u8] = include_bytes!("../../../assets/font/OFL.txt");

#[derive(Clone, Debug)]
pub struct Config {
    pub dir: String,
    pub all_users: bool,
    pub cli: bool,
    pub ctx_archive: bool,
    pub ctx_all: bool,
    /// zip, 7z, rar, tar family, single-file compressed
    pub assoc: [bool; 5],
    pub set_default: bool,
    pub add_path: bool,
    pub desktop: bool,
    pub start_menu: bool,
}

impl Config {
    pub fn new(all_users: bool) -> Config {
        Config {
            dir: default_dir(all_users), all_users, cli: true, ctx_archive: true, ctx_all: true,
            assoc: [true; 5], set_default: true, add_path: true, desktop: false, start_menu: true,
        }
    }

    pub fn to_text(&self) -> String {
        let b = |v: bool| if v { "1" } else { "0" };
        let assoc: String = self.assoc.iter().map(|a| b(*a)).collect();
        format!("dir={}\nall={}\ncli={}\nctxa={}\nctxx={}\nassoc={}\ndef={}\npath={}\ndesk={}\nmenu={}\n",
            self.dir, b(self.all_users), b(self.cli), b(self.ctx_archive), b(self.ctx_all), assoc,
            b(self.set_default), b(self.add_path), b(self.desktop), b(self.start_menu))
    }

    pub fn from_text(t: &str) -> Config {
        let mut c = Config::new(false);
        for line in t.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            let on = v.trim() == "1";
            match k.trim() {
                "dir" => c.dir = v.trim().to_string(),
                "all" => c.all_users = on, "cli" => c.cli = on, "ctxa" => c.ctx_archive = on, "ctxx" => c.ctx_all = on,
                "assoc" => { for (i, ch) in v.trim().chars().enumerate().take(5) { c.assoc[i] = ch == '1'; } }
                "def" => c.set_default = on, "path" => c.add_path = on, "desk" => c.desktop = on, "menu" => c.start_menu = on,
                _ => {}
            }
        }
        c
    }
}

pub fn default_dir(all_users: bool) -> String {
    if all_users {
        let pf = std::env::var("ProgramFiles").unwrap_or_else(|_| "C:\\Program Files".into());
        format!("{}\\Z", pf)
    } else {
        let la = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| "C:\\Users\\Public\\AppData\\Local".into());
        format!("{}\\Programs\\Z", la)
    }
}

pub struct Progress { pub pct: AtomicU32, pub text: Mutex<String> }

impl Progress {
    pub fn new() -> Progress { Progress { pct: AtomicU32::new(0), text: Mutex::new(String::new()) } }
    fn set(&self, pct: u32, text: &str) {
        self.pct.store(pct, Relaxed);
        if let Ok(mut g) = self.text.lock() { g.clear(); g.push_str(text); }
    }
    pub fn fraction(&self) -> f32 { self.pct.load(Relaxed) as f32 / 100.0 }
    pub fn label(&self) -> String { self.text.lock().map(|g| g.clone()).unwrap_or_default() }
}

// ------------------------------------------------------------ process helpers

fn command(program: &str) -> Command {
    let mut c = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);                      // CREATE_NO_WINDOW
    }
    c
}

fn b64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for c in data.chunks(3) {
        let b1 = if c.len() > 1 { c[1] } else { 0 };
        let b2 = if c.len() > 2 { c[2] } else { 0 };
        let n = ((c[0] as u32) << 16) | ((b1 as u32) << 8) | (b2 as u32);
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        out.push(if c.len() > 1 { T[((n >> 6) & 63) as usize] as char } else { '=' });
        out.push(if c.len() > 2 { T[(n & 63) as usize] as char } else { '=' });
    }
    out
}

/// Runs a PowerShell script (passed encoded, so no quoting problems). True when it succeeded.
fn ps(script: &str) -> bool {
    let mut bytes: Vec<u8> = Vec::new();
    for u in script.encode_utf16() { bytes.extend_from_slice(&u.to_le_bytes()); }
    let enc = b64(&bytes);
    command("powershell")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-EncodedCommand", enc.as_str()])
        .output().map(|o| o.status.success()).unwrap_or(false)
}

fn q(s: &str) -> String { s.replace('\'', "''") }

fn reg_import(text: &str) -> Result<()> {
    let mut bytes: Vec<u8> = vec![0xFF, 0xFE];
    for u in text.encode_utf16() { bytes.extend_from_slice(&u.to_le_bytes()); }
    let file = std::env::temp_dir().join(format!("z-setup-{}.reg", std::process::id()));
    fs::write(&file, &bytes).map_err(|_| anyhow!("Could not update the registry"))?;
    let ok = command("reg").arg("import").arg(&file).output().map(|o| o.status.success()).unwrap_or(false);
    let _ = fs::remove_file(&file);
    if ok { Ok(()) } else { bail!("Could not update the registry") }
}

fn reg_delete_value(key: &str, name: &str) {
    let _ = command("reg").args(["delete", key, "/v", name, "/f"]).output();
}

pub fn is_admin() -> bool {
    command("reg").args(["query", "HKU\\S-1-5-19"]).output().map(|o| o.status.success()).unwrap_or(false)
}

/// Starts this program again with administrator rights. True when the user accepted.
pub fn relaunch_elevated(args: &str) -> bool {
    let Ok(exe) = std::env::current_exe() else { return false };
    let script = format!("$ErrorActionPreference='Stop'; Start-Process -FilePath '{}' -ArgumentList '{}' -Verb RunAs", q(&exe.display().to_string()), q(args));
    ps(&script)
}

pub fn dir_is_machine_wide(dir: &Path) -> bool {
    match std::env::var("ProgramFiles") {
        Ok(pf) => dir.starts_with(Path::new(&pf)),
        Err(_) => false,
    }
}

fn kill_z() {
    let _ = command("taskkill").args(["/F", "/IM", "Z.exe"]).output();
    std::thread::sleep(Duration::from_millis(300));
}

fn write_file(path: &Path, data: &[u8]) -> Result<()> {
    let mut last = None;
    for _ in 0..12 {
        match fs::write(path, data) {
            Ok(()) => return Ok(()),
            Err(e) => { last = Some(e); std::thread::sleep(Duration::from_millis(250)); }
        }
    }
    match last {
        Some(e) if e.kind() == std::io::ErrorKind::PermissionDenied => bail!("Access denied"),
        _ => bail!("Could not write files"),
    }
}

#[cfg(windows)]
mod sys {
    #[link(name = "user32")]
    extern "system" {
        pub fn SendMessageTimeoutW(hwnd: isize, msg: u32, wparam: usize, lparam: isize, flags: u32, timeout: u32, result: *mut usize) -> isize;
    }
    #[link(name = "shell32")]
    extern "system" {
        pub fn SHChangeNotify(event_id: i32, flags: u32, item1: *const core::ffi::c_void, item2: *const core::ffi::c_void);
    }
}

fn notify_shell() {
    #[cfg(windows)]
    unsafe {
        sys::SHChangeNotify(0x0800_0000, 0, std::ptr::null(), std::ptr::null());      // SHCNE_ASSOCCHANGED
        let env: Vec<u16> = "Environment".encode_utf16().chain(std::iter::once(0)).collect();
        let mut res: usize = 0;
        sys::SendMessageTimeoutW(0xFFFF, 0x001A, 0, env.as_ptr() as isize, 2, 3000, &mut res as *mut usize);
    }
}

fn path_script(all_users: bool, dir: &str, add: bool) -> String {
    let open = if all_users {
        "[Microsoft.Win32.Registry]::LocalMachine.OpenSubKey('SYSTEM\\CurrentControlSet\\Control\\Session Manager\\Environment', $true)"
    } else {
        "[Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment', $true)"
    };
    let tail = if add {
        "if (-not ($parts | Where-Object { $_.TrimEnd('\\') -ieq $dir.TrimEnd('\\') })) { $parts += $dir }"
    } else {
        "$parts = @($parts | Where-Object { $_.TrimEnd('\\') -ine $dir.TrimEnd('\\') })"
    };
    format!("$ErrorActionPreference='Stop'; $dir='{}'; $k={}; $cur=[string]$k.GetValue('Path','',[Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames); $parts=@($cur -split ';' | Where-Object {{ $_ -ne '' }}); {}; $k.SetValue('Path', ($parts -join ';'), [Microsoft.Win32.RegistryValueKind]::ExpandString); $k.Close()", q(dir), open, tail)
}

fn shortcut_script(cfg: &Config) -> String {
    let exe = format!("{}\\Z.exe", cfg.dir);
    let (menu, desk) = if cfg.all_users { ("CommonPrograms", "CommonDesktopDirectory") } else { ("Programs", "Desktop") };
    let mut s = String::from("$ErrorActionPreference='Stop'; $ws=New-Object -ComObject WScript.Shell; function Mk($base){ $s=$ws.CreateShortcut((Join-Path $base 'Z.lnk')); $s.TargetPath='@EXE@'; $s.WorkingDirectory='@DIR@'; $s.IconLocation='@EXE@,0'; $s.Description='Z'; $s.Save() }; ");
    if cfg.start_menu { s.push_str(&format!("Mk ([Environment]::GetFolderPath('{}')); ", menu)); }
    if cfg.desktop { s.push_str(&format!("Mk ([Environment]::GetFolderPath('{}')); ", desk)); }
    s.replace("@EXE@", &q(&exe)).replace("@DIR@", &q(&cfg.dir))
}

// ------------------------------------------------------------ install

pub fn install(cfg: &Config, p: &Progress) -> Result<()> {
    if Z_EXE.is_empty() { bail!("Setup is incomplete"); }
    let dir = PathBuf::from(&cfg.dir);
    p.set(4, "Closing Z");
    kill_z();

    p.set(12, "Copying files");
    fs::create_dir_all(&dir).map_err(|_| anyhow!("Could not create the folder"))?;
    write_file(&dir.join("Z.exe"), Z_EXE)?;
    p.set(30, "Copying files");
    if cfg.cli { write_file(&dir.join("zc.exe"), ZC_EXE)?; }
    write_file(&dir.join("z-archive.ico"), ICON)?;
    write_file(&dir.join("LICENSE"), LICENSE)?;
    write_file(&dir.join("OFL.txt"), FONT_LICENSE)?;
    let me = std::env::current_exe().map_err(|_| anyhow!("Could not write files"))?;
    let un = dir.join("Uninstall.exe");
    if me != un { fs::copy(&me, &un).map_err(|_| anyhow!("Could not write files"))?; }

    p.set(55, "Registering with Windows");
    let size_kb = ((Z_EXE.len() + if cfg.cli { ZC_EXE.len() } else { 0 }) / 1024) as u32 + 64;
    reg_import(&reg::install_reg(cfg, size_kb))?;
    if cfg.set_default && cfg.assoc.iter().any(|a| *a) {
        reg_import(&reg::default_reg(cfg))?;
        // Windows remembers an earlier "always open with" choice here; removing it lets Z take over.
        for (ext, g) in reg::ASSOC.iter() {
            if cfg.assoc[*g] {
                let key = format!("HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\FileExts\\.{}\\UserChoice", ext);
                let _ = command("reg").args(["delete", key.as_str(), "/f"]).output();
            }
        }
    }

    p.set(75, "Creating shortcuts");
    if cfg.start_menu || cfg.desktop { if !ps(&shortcut_script(cfg)) { bail!("Could not create shortcuts"); } }

    p.set(88, "Updating PATH");
    if cfg.cli && cfg.add_path { if !ps(&path_script(cfg.all_users, &cfg.dir, true)) { bail!("Could not update PATH"); } }

    p.set(96, "Finishing");
    notify_shell();
    p.set(100, "Done");
    Ok(())
}

// ------------------------------------------------------------ uninstall

pub fn uninstall(dir: &Path, all_users: bool, p: &Progress) -> Result<()> {
    let dir_s = dir.display().to_string();
    p.set(10, "Closing Z");
    kill_z();

    p.set(30, "Removing registry entries");
    let _ = reg_import(&reg::uninstall_reg(all_users));
    let root = if all_users { "HKLM" } else { "HKCU" };
    for (ext, _) in reg::ASSOC.iter() {
        reg_delete_value(&format!("{}\\Software\\Classes\\.{}\\OpenWithProgids", root, ext), "Z.Archive");
    }
    reg_delete_value(&format!("{}\\Software\\RegisteredApplications", root), "Z");

    p.set(55, "Removing shortcuts");
    let _ = ps("$ErrorActionPreference='SilentlyContinue'; foreach($f in 'Programs','CommonPrograms','Desktop','CommonDesktopDirectory'){ Remove-Item (Join-Path ([Environment]::GetFolderPath($f)) 'Z.lnk') -Force }");

    p.set(70, "Updating PATH");
    let _ = ps(&path_script(all_users, &dir_s, false));

    p.set(85, "Removing files");
    if let Ok(la) = std::env::var("LOCALAPPDATA") { let _ = fs::remove_dir_all(Path::new(&la).join("Z")); }
    notify_shell();
    // The running uninstaller cannot delete itself: a detached shell removes the folder a moment later.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = Command::new("cmd").arg("/C")
            .raw_arg(format!("ping -n 3 127.0.0.1 >nul & rmdir /s /q \"{}\"", dir_s))
            .creation_flags(0x0800_0008).spawn();
    }
    p.set(100, "Done");
    Ok(())
}
