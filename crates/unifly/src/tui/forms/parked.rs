//! The sessions connection tests park on disk, and cleaning up after a
//! process that died holding one.
//!
//! A test's cookie waits in a session cache of its own until the profile
//! is saved (see [`super::probe`]). The form drops it when it is not
//! needed, but a process that is killed, loses its terminal, or aborts on
//! a panic drops nothing, and would leave a live cookie in the cache
//! directory. So every process that parks a session holds a lock on a
//! marker file for as long as it runs, and the next launch removes parked
//! sessions whose marker nobody holds.

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

/// What sets a parked session's cache name apart from a profile's.
const MARK: &str = ".setup-";

/// Numbers the sessions this process parks.
static PARKED: AtomicU64 = AtomicU64::new(1);

/// This process's hold on its marker file, taken the first time it parks
/// a session and kept until it exits.
static CLAIM: OnceLock<Option<File>> = OnceLock::new();

/// A cache name for a session this process is about to park for
/// `profile`, never equal to a profile's own.
pub(super) fn next_name(profile: &str) -> String {
    // Unit tests keep out of the user's cache directory.
    if !cfg!(test) {
        CLAIM.get_or_init(|| {
            unifly_api::session::session_cache::cache_dir()
                .and_then(|dir| claim(&dir, std::process::id()))
        });
    }
    format!(
        "{profile}{MARK}{}-{}",
        std::process::id(),
        PARKED.fetch_add(1, Ordering::Relaxed)
    )
}

/// Remove the parked sessions the launches before this one left behind.
/// Called once, before this process parks anything.
pub(crate) fn sweep_at_launch() {
    if let Some(dir) = unifly_api::session::session_cache::cache_dir() {
        let removed = sweep(&dir, std::process::id());
        if removed > 0 {
            tracing::info!(
                removed,
                "removed parked test sessions left by an earlier run"
            );
        }
    }
}

fn marker(dir: &Path, pid: u32) -> PathBuf {
    dir.join(format!("setup-{pid}.lock"))
}

/// Take the marker that says process `pid` is alive and may have sessions
/// parked in `dir`. The returned file holds the lock until it is dropped.
fn claim(dir: &Path, pid: u32) -> Option<File> {
    fs::create_dir_all(dir).ok()?;
    let path = marker(dir, pid);
    // A sweep that ran between the create and the lock may have removed
    // the file as a dead owner's, leaving a lock on nothing: take it again.
    for _ in 0..3 {
        let file = File::options()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .ok()?;
        file.try_lock().ok()?;
        if path.exists() {
            return Some(file);
        }
    }
    None
}

/// The process that parked the cache file called `file_name`, when it is
/// a parked session.
fn owner_of(file_name: &str) -> Option<u32> {
    let after = &file_name[file_name.rfind(MARK)? + MARK.len()..];
    let (pid, rest) = after.split_once('-')?;
    // The counter and the address hash follow.
    rest.contains('_').then(|| pid.parse().ok()).flatten()
}

/// Whether process `pid` is gone: nobody holds its marker in `dir`.
/// `own` has parked nothing yet, so a session under its number was left
/// by an earlier process that had the same one.
fn is_gone(dir: &Path, pid: u32, own: u32) -> bool {
    if pid == own {
        return true;
    }
    match File::options().write(true).open(marker(dir, pid)) {
        // Held by a live process. Anything else that keeps the lock from
        // being taken is not proof the owner is gone.
        Ok(file) => file.try_lock().is_ok(),
        Err(error) => error.kind() == std::io::ErrorKind::NotFound,
    }
}

/// Remove from `dir` every parked session whose owner is gone, and the
/// markers of owners that are. Returns how many sessions went.
fn sweep(dir: &Path, own: u32) -> usize {
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    let names: Vec<String> = entries
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .collect();

    let mut removed = 0;
    for name in &names {
        if let Some(pid) = owner_of(name)
            && is_gone(dir, pid, own)
            && fs::remove_file(dir.join(name)).is_ok()
        {
            removed += 1;
        }
    }
    for name in &names {
        let pid = name
            .strip_prefix("setup-")
            .and_then(|rest| rest.strip_suffix(".lock"))
            .and_then(|pid| pid.parse::<u32>().ok());
        if let Some(pid) = pid
            && pid != own
            && is_gone(dir, pid, own)
        {
            let _ = fs::remove_file(dir.join(name));
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::{claim, marker, next_name, owner_of, sweep};

    #[test]
    fn a_parked_name_tells_its_owner_and_a_profile_cache_is_not_one() {
        let name = next_name("home");
        assert!(name.starts_with("home.setup-"), "{name}");
        assert_eq!(
            owner_of(&format!("{name}_00000000deadbeef.json")),
            Some(std::process::id())
        );
        assert_eq!(
            owner_of("home.setup-4242-7_00000000deadbeef.json"),
            Some(4242)
        );
        // A staged write of one is its owner's too.
        assert_eq!(
            owner_of(".home.setup-4242-7_00000000deadbeef.json.4242.0.12.tmp"),
            Some(4242)
        );

        for other in [
            "home_00000000deadbeef.json",
            "setup-4242.lock",
            "home.setup-x-7_00000000deadbeef.json",
        ] {
            assert_eq!(owner_of(other), None, "{other}");
        }
    }

    /// A process that died with a session parked left its cookie on disk.
    /// The next launch removes it, and leaves alone both the profiles'
    /// own caches and whatever a process that is still running parked.
    #[test]
    fn a_launch_removes_parked_sessions_whose_owner_is_gone() {
        let dir = tempfile::tempdir().expect("tempdir");
        let at = |name: &str| dir.path().join(name);
        let write = |name: &str| std::fs::write(at(name), "{}").expect("write");
        let (own, alive, dead) = (100, 200, 300);

        write("home_00000000deadbeef.json");
        // Alive: its marker is held.
        write("home.setup-200-1_00000000deadbeef.json");
        let held = claim(dir.path(), alive).expect("claim");
        // Dead: its marker is there, and nobody holds it.
        write("home.setup-300-1_00000000deadbeef.json");
        write("work.setup-300-2_00000000deadbeef.json");
        drop(claim(dir.path(), dead).expect("claim"));
        // Gone without a marker at all.
        write("home.setup-400-9_00000000deadbeef.json");
        // Left by an earlier process that had this one's number.
        write("home.setup-100-3_00000000deadbeef.json");

        assert_eq!(sweep(dir.path(), own), 4);

        assert!(
            at("home_00000000deadbeef.json").exists(),
            "a profile's cache"
        );
        assert!(at("home.setup-200-1_00000000deadbeef.json").exists());
        assert!(marker(dir.path(), alive).exists());
        for gone in [
            "home.setup-300-1_00000000deadbeef.json",
            "work.setup-300-2_00000000deadbeef.json",
            "home.setup-400-9_00000000deadbeef.json",
            "home.setup-100-3_00000000deadbeef.json",
        ] {
            assert!(!at(gone).exists(), "{gone}");
        }
        assert!(!marker(dir.path(), dead).exists(), "its marker goes too");

        // Once the live owner exits, the next launch takes its leftovers.
        drop(held);
        assert_eq!(sweep(dir.path(), own), 1);
        assert!(!marker(dir.path(), alive).exists());
    }

    #[test]
    fn a_missing_cache_directory_is_nothing_to_sweep() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(sweep(&dir.path().join("absent"), 1), 0);
    }
}
