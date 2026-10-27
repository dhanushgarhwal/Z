//! Windows launches a classic shell verb once *per selected file*. To get one archive out of a
//! multi-selection, each process appends its path to a shared list; the first one becomes the
//! leader, waits briefly for the rest, then runs the job for everybody. (A COM shell extension,
//! planned for phase 2, removes the need for this.)
use crate::jobs::Job;
use std::collections::hash_map::DefaultHasher;
use std::collections::BTreeSet;
use std::hash::{Hash, Hasher};
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

fn gather(tag: &str, first: &PathBuf) -> Option<Vec<PathBuf>> {
    let mut h = DefaultHasher::new();
    tag.hash(&mut h);
    first.parent().hash(&mut h);
    let key = format!("{:016x}", h.finish());
    let dir = std::env::temp_dir();
    let (list, lock) = (dir.join(format!("z-{key}.lst")), dir.join(format!("z-{key}.lock")));

    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&list).ok()?;
    writeln!(f, "{}", first.display()).ok()?;
    drop(f);
    // Only one process wins the lock and becomes the leader.
    if std::fs::OpenOptions::new().write(true).create_new(true).open(&lock).is_err() {
        // Stale lock from a crash? Take over if it is old.
        let stale = std::fs::metadata(&lock).and_then(|m| m.modified()).ok()
            .and_then(|t| t.elapsed().ok()).map(|e| e > Duration::from_secs(30)).unwrap_or(false);
        if !stale { return None; }
        let _ = std::fs::remove_file(&lock);
        std::fs::OpenOptions::new().write(true).create_new(true).open(&lock).ok()?;
    }
    std::thread::sleep(Duration::from_millis(700));
    let text = std::fs::read_to_string(&list).unwrap_or_default();
    let _ = std::fs::remove_file(&list);
    let _ = std::fs::remove_file(&lock);
    let set: BTreeSet<PathBuf> = text.lines().filter(|l| !l.is_empty()).map(PathBuf::from).collect();
    Some(set.into_iter().collect())
}

/// Returns None when this process is a follower and should simply exit.
pub fn merge(job: Job) -> Option<Job> {
    Some(match job {
        Job::Add { inputs, out, opts, dialog } => Job::Add { inputs: gather("a", inputs.first()?)?, out, opts, dialog },
        Job::Extract { archives, dest, password, overwrite, only } =>
            Job::Extract { archives: gather("x", archives.first()?)?, dest, password, overwrite, only },
        Job::Test { archives, password } => Job::Test { archives: gather("t", archives.first()?)?, password },
    })
}
