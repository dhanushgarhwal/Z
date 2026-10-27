use std::sync::Arc;
use std::time::Duration;
use z::cli::{self, Cmd};
use zcore::{fsutil, Progress};

fn main() {
    let code = match real_main() { Ok(()) => 0, Err(e) => { eprintln!("zc: {e}"); 1 } };
    std::process::exit(code);
}

fn real_main() -> anyhow::Result<()> {
    match cli::parse(std::env::args_os().skip(1).collect())? {
        Cmd::Help | Cmd::Gui(None) => print!("{}", cli::HELP),
        Cmd::Version => println!("Z {}", env!("CARGO_PKG_VERSION")),
        Cmd::Gui(Some(p)) => { eprintln!("zc: '{}' is not a command. Use `zc l` to list or `zc x` to extract.", p.display()); std::process::exit(2); }
        Cmd::List(p, pw) => {
            let entries = zcore::list(&p, pw.as_deref())?;
            let (mut files, mut bytes) = (0u64, 0u64);
            for e in &entries {
                let (y, m, d, hh, mm, _) = fsutil::unix_to_civil(e.modified);
                let when = if e.modified > 0 { format!("{y:04}-{m:02}-{d:02} {hh:02}:{mm:02}") } else { "                ".into() };
                println!("{when} {:>12}  {}{}", if e.is_dir { "<dir>".into() } else { e.size.to_string() }, e.path, if e.encrypted { "  *" } else { "" });
                if !e.is_dir { files += 1; bytes += e.size; }
            }
            println!("{files} file(s), {}", fsutil::human_size(bytes));
        }
        Cmd::Job(job, _) => {
            let p = Arc::new(Progress::new());
            let (p2, j2) = (p.clone(), job.clone());
            let h = std::thread::spawn(move || z::jobs::run(&j2, &p2).map_err(|e| e.to_string()));
            while !h.is_finished() {
                let pct = (p.fraction() * 100.0) as u32;
                eprint!("\r{pct:>3}%  {:<60.60}", p.current());
                std::thread::sleep(Duration::from_millis(100));
            }
            eprint!("\r{:<70}\r", "");
            match h.join().map_err(|_| anyhow::anyhow!("worker crashed"))? { Ok(msg) => println!("{msg}"), Err(e) => anyhow::bail!(e) }
        }
    }
    Ok(())
}
