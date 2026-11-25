//! Saving and loading, backups and the single-instance lock.
//!
//! Everything lives in one folder (default `%LOCALAPPDATA%\TinyTimeTrackLite`,
//! override with `TTT_DATA_DIR`):
//! * `data.json` - all your data. Saved atomically: written to `data.json.tmp`,
//!   flushed to disk, the previous file kept as `data.json.bak`, then swapped in.
//!   A crash mid-save can never leave a half-written file behind.
//! * `backups/` - a dated copy made once a day you use the app (newest 30 kept).
//! * `app.lock` - held while the app runs so a second copy can't overwrite data.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use chrono::{DateTime, Local, NaiveDateTime};

use crate::model::Data;

pub const DATA_FILE: &str = "data.json";
const BACKUP_PREFIX: &str = "tinytimetrack-";
const KEEP_BACKUPS: usize = 30;

pub fn default_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("TTT_DATA_DIR") {
        if !dir.trim().is_empty() {
            return PathBuf::from(dir);
        }
    }
    dirs::data_local_dir().unwrap_or_else(|| PathBuf::from(".")).join("TinyTimeTrackLite")
}

pub fn backup_dir(dir: &Path) -> PathBuf {
    dir.join("backups")
}

/// Hold `app.lock` for as long as the returned file lives.
pub fn acquire_lock(dir: &Path) -> Result<File, String> {
    fs::create_dir_all(dir).map_err(|e| format!("Can't create the data folder {}: {e}", dir.display()))?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join("app.lock"))
        .map_err(|e| format!("Can't open the lock file: {e}"))?;
    match file.try_lock() {
        Ok(()) => Ok(file),
        Err(_) => Err("Tiny Time Track is already open in another window.".into()),
    }
}

/// Load the data. A missing file means a fresh start; a damaged file is an
/// error (never silently replaced), with the previous save tried first.
pub fn load(dir: &Path) -> Result<(Data, Option<String>), String> {
    let main = dir.join(DATA_FILE);
    let bak = dir.join(format!("{DATA_FILE}.bak"));
    match read(&main) {
        Ok(Some(data)) => Ok((data, None)),
        Ok(None) => match read(&bak) {
            // data.json missing but a .bak exists: a save was interrupted mid-swap.
            Ok(Some(data)) => Ok((data, Some("Recovered your data from the last good save.".into()))),
            _ => Ok((Data::default(), None)),
        },
        Err(main_err) => match read(&bak) {
            Ok(Some(data)) => Ok((
                data,
                Some(format!("{DATA_FILE} was damaged ({main_err}); your previous save was loaded instead.")),
            )),
            _ => Err(format!(
                "Your data file {} can't be read ({main_err}). Nothing has been changed. \
                 Restore a backup from the backups folder, or move the file aside to start fresh.",
                main.display()
            )),
        },
    }
}

fn read(path: &Path) -> Result<Option<Data>, String> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice::<Data>(&bytes).map(Some).map_err(|e| e.to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

pub fn save(dir: &Path, data: &Data) -> Result<(), String> {
    let bytes = serde_json::to_vec(data).map_err(|e| e.to_string())?;
    write_atomic(dir, &bytes)
}

fn write_atomic(dir: &Path, bytes: &[u8]) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let main = dir.join(DATA_FILE);
    let tmp = dir.join(format!("{DATA_FILE}.tmp"));
    let bak = dir.join(format!("{DATA_FILE}.bak"));
    {
        let mut file = File::create(&tmp).map_err(|e| format!("Couldn't save: {e}"))?;
        file.write_all(bytes).map_err(|e| format!("Couldn't save: {e}"))?;
        file.sync_all().map_err(|e| format!("Couldn't save: {e}"))?;
    }
    if main.exists() {
        fs::copy(&main, &bak).map_err(|e| format!("Couldn't save: {e}"))?;
    }
    fs::rename(&tmp, &main).map_err(|e| format!("Couldn't save: {e}"))
}

// --------------------------------------------------------------------------
//  Backups
// --------------------------------------------------------------------------
#[derive(Clone, Debug)]
pub struct BackupInfo {
    pub path: PathBuf,
    pub name: String,
    pub size: u64,
    pub modified: NaiveDateTime,
    order: u128,
}

pub fn list_backups(dir: &Path) -> Vec<BackupInfo> {
    let Ok(entries) = fs::read_dir(backup_dir(dir)) else { return Vec::new() };
    let mut list: Vec<BackupInfo> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            if !(name.starts_with(BACKUP_PREFIX) && name.ends_with(".json")) {
                return None;
            }
            // fs::metadata, not entry.metadata(): on Windows a directory listing's
            // timestamps can lag behind the file's real ones.
            let meta = fs::metadata(entry.path()).ok()?;
            let time = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            let order = time.duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
            let modified = DateTime::<Local>::from(time).naive_local();
            Some(BackupInfo { path: entry.path(), name, size: meta.len(), modified, order })
        })
        .collect();
    // Newest first, by the creation stamp in the name plus its "-N" counter
    // (exact, unlike file times); file time only breaks ties for odd names.
    list.sort_by(|a, b| (name_order(&b.name), b.order).cmp(&(name_order(&a.name), a.order)));
    list
}

/// ("20261117-212155", 2) for "tinytimetrack-20261117-212155-2.json".
fn name_order(name: &str) -> (String, u32) {
    let core = name.trim_start_matches(BACKUP_PREFIX).trim_end_matches(".json");
    let parts: Vec<&str> = core.split('-').collect();
    match parts.as_slice() {
        [date, time, n] => (format!("{date}-{time}"), n.parse().unwrap_or(1)),
        _ => (core.to_string(), 1),
    }
}

/// Copy the current data file into `backups/` and prune to the newest 30.
pub fn make_backup(dir: &Path) -> Result<PathBuf, String> {
    let main = dir.join(DATA_FILE);
    if !main.exists() {
        return Err("There's nothing to back up yet.".into());
    }
    let folder = backup_dir(dir);
    fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let stamp = Local::now().format("%Y%m%d-%H%M%S").to_string();
    // Same-second backups get "-2", "-3", ... always counting up from the highest
    // used (never refilling a pruned gap, which would sort it as the oldest).
    let used = list_backups(dir).iter().map(|b| name_order(&b.name)).filter(|(s, _)| *s == stamp).map(|(_, n)| n).max();
    let target = match used {
        None => folder.join(format!("{BACKUP_PREFIX}{stamp}.json")),
        Some(n) => folder.join(format!("{BACKUP_PREFIX}{stamp}-{}.json", n + 1)),
    };
    // Read + write (not fs::copy, which keeps the source's timestamp on Windows)
    // so each backup is stamped with the time it was made.
    let bytes = fs::read(&main).map_err(|e| format!("Backup failed: {e}"))?;
    fs::write(&target, bytes).map_err(|e| format!("Backup failed: {e}"))?;
    for old in list_backups(dir).into_iter().skip(KEEP_BACKUPS) {
        let _ = fs::remove_file(old.path);
    }
    Ok(target)
}

/// Automatic backup at start-up, at most about once a day.
pub fn backup_if_due(dir: &Path) -> Option<PathBuf> {
    if !dir.join(DATA_FILE).exists() {
        return None;
    }
    if let Some(latest) = list_backups(dir).first() {
        if Local::now().naive_local() - latest.modified < chrono::TimeDelta::hours(20) {
            return None;
        }
    }
    make_backup(dir).ok()
}

/// Make `name` the live data: it's checked first, and a safety backup of the
/// current data is made, so a restore can itself be undone.
pub fn restore_backup(dir: &Path, name: &str) -> Result<(Data, PathBuf), String> {
    let chosen = list_backups(dir)
        .into_iter()
        .find(|b| b.name == name)
        .ok_or("That backup no longer exists.")?;
    let bytes = fs::read(&chosen.path).map_err(|e| format!("Couldn't read {name}: {e}"))?;
    let data: Data = serde_json::from_slice(&bytes).map_err(|_| format!("{name} isn't a Tiny Time Track backup."))?;
    let safety = make_backup(dir)?;
    save(dir, &data)?;
    Ok((data, safety))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Lane;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ttt-test-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sample() -> Data {
        let mut data = Data::default();
        let a = data.add_project("Alpha", "A1", "").unwrap();
        let day = chrono::NaiveDate::from_ymd_opt(2026, 11, 16).unwrap();
        data.add_block(day, Lane::Actual, a, 18, 24).unwrap();
        data
    }

    #[test]
    fn save_load_round_trip_and_fresh_start() {
        let dir = temp_dir("roundtrip");
        assert_eq!(load(&dir).unwrap().0, Data::default());
        let data = sample();
        save(&dir, &data).unwrap();
        save(&dir, &data).unwrap();
        assert!(dir.join("data.json.bak").exists());
        assert!(!dir.join("data.json.tmp").exists());
        assert_eq!(load(&dir).unwrap().0, data);
    }

    #[test]
    fn damaged_file_uses_previous_save_or_refuses() {
        let dir = temp_dir("damaged");
        let data = sample();
        save(&dir, &data).unwrap();
        save(&dir, &data).unwrap();
        fs::write(dir.join(DATA_FILE), b"{ not json").unwrap();
        let (loaded, warning) = load(&dir).unwrap();
        assert_eq!(loaded, data);
        assert!(warning.unwrap().contains("damaged"));
        fs::write(dir.join("data.json.bak"), b"garbage").unwrap();
        assert!(load(&dir).unwrap_err().contains("Nothing has been changed"));
    }

    #[test]
    fn interrupted_swap_recovers_from_bak() {
        let dir = temp_dir("swap");
        let data = sample();
        save(&dir, &data).unwrap();
        save(&dir, &data).unwrap();
        fs::remove_file(dir.join(DATA_FILE)).unwrap();
        let (loaded, warning) = load(&dir).unwrap();
        assert_eq!(loaded, data);
        assert!(warning.is_some());
    }

    #[test]
    fn backups_prune_and_restore() {
        let dir = temp_dir("backups");
        let mut data = sample();
        save(&dir, &data).unwrap();
        let first = make_backup(&dir).unwrap();
        for _ in 0..31 {
            make_backup(&dir).unwrap();
        }
        assert_eq!(list_backups(&dir).len(), 30);
        assert!(!first.exists(), "the oldest backup is pruned");
        let keep = list_backups(&dir)[0].name.clone();
        data.add_project("Later", "", "").unwrap();
        save(&dir, &data).unwrap();
        let (restored, safety) = restore_backup(&dir, &keep).unwrap();
        assert_eq!(restored.projects.len(), 1);
        assert_eq!(load(&dir).unwrap().0.projects.len(), 1);
        assert!(safety.exists());
        assert!(restore_backup(&dir, "../data.json").is_err());
        fs::write(backup_dir(&dir).join(format!("{BACKUP_PREFIX}junk.json")), b"nope").unwrap();
        assert!(restore_backup(&dir, &format!("{BACKUP_PREFIX}junk.json")).is_err());
        assert_eq!(load(&dir).unwrap().0.projects.len(), 1, "a bad restore changes nothing");
    }

    #[test]
    fn second_instance_is_refused() {
        let dir = temp_dir("lock");
        let held = acquire_lock(&dir).unwrap();
        assert!(acquire_lock(&dir).unwrap_err().contains("already open"));
        drop(held);
        assert!(acquire_lock(&dir).is_ok());
    }
}
