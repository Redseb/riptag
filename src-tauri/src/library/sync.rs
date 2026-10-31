use super::profile::DeviceProfile;
use super::repair::{RepairProgress, SkippedFile, ensure_separate};
use super::scan::relative_components;
use serde::Serialize;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use sysinfo::Disks;
use walkdir::WalkDir;

const BUFFER_BYTES: usize = 1024 * 1024;
/// A generous FAT cluster, so the free space check allows for rounding up.
const CLUSTER_BYTES: u64 = 64 * 1024;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerVolume {
    pub name: String,
    pub path: PathBuf,
    pub free_bytes: u64,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncReport {
    pub device: String,
    pub copied: usize,
    pub already_there: Vec<String>,
    pub failed: Vec<SkippedFile>,
    /// Set when the device went away during the run.
    pub stopped: bool,
}

fn is_player(name: &str, file_system: &str, profile: &DeviceProfile) -> bool {
    profile.volume_names.contains(&name)
        && profile
            .volume_file_systems
            .contains(&file_system.to_lowercase().as_str())
}

/// Mounted volumes that look like this device. Only reads the list of disks.
pub fn find_player_volumes(profile: &DeviceProfile) -> Result<Vec<PlayerVolume>, String> {
    let disks = Disks::new_with_refreshed_list().map_err(|e| e.to_string())?;
    Ok(disks
        .iter()
        .filter(|disk| {
            is_player(
                &disk.name().to_string_lossy(),
                &disk.file_system().to_string_lossy(),
                profile,
            )
        })
        .map(|disk| PlayerVolume {
            name: disk.name().to_string_lossy().into_owned(),
            path: disk.mount_point().to_path_buf(),
            free_bytes: disk.available_space(),
        })
        .collect())
}

fn files_to_copy(folder: &Path) -> Vec<PathBuf> {
    WalkDir::new(folder)
        .sort_by_file_name()
        .into_iter()
        .filter_entry(|e| e.depth() == 0 || !e.file_name().to_string_lossy().starts_with('.'))
        .flatten()
        .filter(|e| e.file_type().is_file())
        .map(|e| e.into_path())
        .collect()
}

/// Creates each missing folder below `root`, never `root` itself, as an unplugged
/// device must not turn into a new folder at its old mount point.
fn create_folders(root: &Path, relative: &Path, created: &mut Vec<PathBuf>) -> Result<(), String> {
    let mut current = root.to_path_buf();
    for part in relative {
        current.push(part);
        if !current.is_dir() {
            fs::create_dir(&current).map_err(|e| e.to_string())?;
            created.push(current.clone());
        }
    }
    Ok(())
}

fn same_contents(a: &Path, b: &Path) -> io::Result<bool> {
    let (mut a, mut b) = (File::open(a)?, File::open(b)?);
    if a.metadata()?.len() != b.metadata()?.len() {
        return Ok(false);
    }
    let (mut left, mut right) = (vec![0; BUFFER_BYTES], vec![0; BUFFER_BYTES]);
    loop {
        let count = a.read(&mut left)?;
        if count == 0 {
            return Ok(true);
        }
        b.read_exact(&mut right[..count])?;
        if left[..count] != right[..count] {
            return Ok(false);
        }
    }
}

/// Written by hand, as `fs::copy` also copies extended attributes, which FAT can
/// only store as extra files. Renamed last, so a track is whole or isn't there.
fn copy_verified(from: &Path, to: &Path) -> Result<(), String> {
    let parent = to.parent().ok_or("Invalid destination")?;
    let name = to.file_name().ok_or("Invalid destination")?;
    let temp = parent.join(format!(".{}.part", name.to_string_lossy()));

    let result = (|| {
        let mut output = File::create(&temp).map_err(|e| e.to_string())?;
        io::copy(
            &mut File::open(from).map_err(|e| e.to_string())?,
            &mut output,
        )
        .map_err(|e| e.to_string())?;
        output.flush().map_err(|e| e.to_string())?;
        output.sync_all().map_err(|e| e.to_string())?;
        drop(output);
        if !same_contents(from, &temp).map_err(|e| e.to_string())? {
            return Err("The copy didn't match the original".to_owned());
        }
        fs::rename(&temp, to).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
        remove_apple_double(&temp);
    }
    result
}

/// macOS keeps attributes of files on FAT in a hidden `._name` beside them, which
/// a player may list as a broken track.
fn remove_apple_double(path: &Path) {
    if let (Some(parent), Some(name)) = (path.parent(), path.file_name()) {
        let _ = fs::remove_file(parent.join(format!("._{}", name.to_string_lossy())));
    }
}

/// Copies everything in `folder` to `volume`, keeping its layout. Never replaces
/// or removes what's already on the device, so running it again only adds what's
/// missing.
pub fn sync_to_device(
    folder: &Path,
    volume: &PlayerVolume,
    mut on_progress: impl FnMut(RepairProgress),
) -> Result<SyncReport, String> {
    if !folder.is_dir() {
        return Err(format!("Not a folder: {}", folder.display()));
    }
    if !volume.path.is_dir() {
        return Err(format!("{} isn't connected", volume.name));
    }
    ensure_separate(folder, &volume.path)?;

    let mut report = SyncReport {
        device: volume.name.clone(),
        ..SyncReport::default()
    };
    let mut to_copy = Vec::new();
    let mut needed = 0;
    for file in files_to_copy(folder) {
        let relative = file.strip_prefix(folder).map_err(|e| e.to_string())?;
        if volume.path.join(relative).exists() {
            report
                .already_there
                .push(relative_components(&file, folder).join("/"));
        } else {
            needed += fs::metadata(&file)
                .map_err(|e| e.to_string())?
                .len()
                .next_multiple_of(CLUSTER_BYTES);
            to_copy.push(file);
        }
    }
    if needed > volume.free_bytes {
        return Err(format!(
            "Not enough space on {}: needs {} MB, {} MB free",
            volume.name,
            needed.div_ceil(1_000_000),
            volume.free_bytes / 1_000_000
        ));
    }

    let total = to_copy.len();
    let mut created_folders = Vec::new();
    let mut created_files = Vec::new();
    for (index, file) in to_copy.iter().enumerate() {
        on_progress(RepairProgress {
            done: index + 1,
            total,
        });
        if !volume.path.is_dir() {
            report.stopped = true;
            break;
        }
        let relative = file.strip_prefix(folder).map_err(|e| e.to_string())?;
        let dest = volume.path.join(relative);
        let result = relative
            .parent()
            .ok_or_else(|| "Invalid path".to_owned())
            .and_then(|parent| create_folders(&volume.path, parent, &mut created_folders))
            .and_then(|()| copy_verified(file, &dest));
        match result {
            Ok(()) => {
                report.copied += 1;
                created_files.push(dest);
            }
            Err(reason) => report.failed.push(SkippedFile {
                file: relative_components(file, folder).join("/"),
                reason,
            }),
        }
    }

    // Only for what this run created, as anything else isn't ours to remove.
    for path in created_files.iter().chain(&created_folders) {
        remove_apple_double(path);
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::super::profile::ECHO_MINI;
    use super::*;

    fn volume(path: &Path, free_bytes: u64) -> PlayerVolume {
        PlayerVolume {
            name: "ECHO MINI".to_owned(),
            path: path.to_path_buf(),
            free_bytes,
        }
    }

    /// A repaired album, and an empty device, under a temporary folder.
    fn setup(dir: &Path) -> (PathBuf, PathBuf) {
        let (output, device) = (dir.join("out"), dir.join("device"));
        let album = output.join("Artist/2014 - Album");
        fs::create_dir_all(&album).unwrap();
        fs::write(album.join("01 Song.flac"), "audio").unwrap();
        fs::write(album.join("01 Song.lrc"), "[00:00.00]la").unwrap();
        fs::write(album.join("cover.jpg"), "jpeg").unwrap();
        fs::write(album.join(".01 Song.flac.part"), "leftover").unwrap();
        fs::create_dir_all(&device).unwrap();
        (output, device)
    }

    fn run(output: &Path, device: &Path) -> SyncReport {
        sync_to_device(output, &volume(device, u64::MAX), |_| {}).unwrap()
    }

    fn files_on(device: &Path) -> Vec<String> {
        WalkDir::new(device)
            .sort_by_file_name()
            .into_iter()
            .flatten()
            .filter(|e| e.file_type().is_file())
            .map(|e| relative_components(e.path(), device).join("/"))
            .collect()
    }

    #[test]
    fn recognises_the_player_by_name_and_file_system() {
        assert!(is_player("ECHO MINI", "msdos", &ECHO_MINI));
        assert!(is_player("ECHO MINI", "exFAT", &ECHO_MINI));
        assert!(!is_player("ECHO MINI", "apfs", &ECHO_MINI));
        assert!(!is_player("Backup", "msdos", &ECHO_MINI));
    }

    #[test]
    fn copies_the_layout_but_not_hidden_files() {
        let dir = tempfile::tempdir().unwrap();
        let (output, device) = setup(dir.path());

        let report = run(&output, &device);

        assert_eq!(report.copied, 3);
        assert!(!report.stopped);
        assert_eq!(
            files_on(&device),
            [
                "Artist/2014 - Album/01 Song.flac",
                "Artist/2014 - Album/01 Song.lrc",
                "Artist/2014 - Album/cover.jpg",
            ]
        );
        let copy = device.join("Artist/2014 - Album/01 Song.flac");
        assert_eq!(fs::read(copy).unwrap(), b"audio");
    }

    #[test]
    fn never_replaces_or_removes_what_is_on_the_device() {
        let dir = tempfile::tempdir().unwrap();
        let (output, device) = setup(dir.path());
        let existing = device.join("Artist/2014 - Album/01 Song.flac");
        fs::create_dir_all(existing.parent().unwrap()).unwrap();
        fs::write(&existing, "mine").unwrap();
        fs::write(device.join("older.mp3"), "other").unwrap();

        let report = run(&output, &device);

        assert_eq!(fs::read(&existing).unwrap(), b"mine");
        assert_eq!(fs::read(device.join("older.mp3")).unwrap(), b"other");
        assert_eq!(report.copied, 2);
        assert_eq!(report.already_there, ["Artist/2014 - Album/01 Song.flac"]);
    }

    #[test]
    fn running_again_copies_nothing_new() {
        let dir = tempfile::tempdir().unwrap();
        let (output, device) = setup(dir.path());
        run(&output, &device);

        let report = run(&output, &device);

        assert_eq!(report.copied, 0);
        assert_eq!(report.already_there.len(), 3);
    }

    #[test]
    fn refuses_to_start_without_enough_space() {
        let dir = tempfile::tempdir().unwrap();
        let (output, device) = setup(dir.path());

        let result = sync_to_device(&output, &volume(&device, CLUSTER_BYTES), |_| {});

        assert!(result.unwrap_err().starts_with("Not enough space"));
        assert!(files_on(&device).is_empty());
    }

    #[test]
    fn counts_only_what_still_needs_copying_as_space_needed() {
        let dir = tempfile::tempdir().unwrap();
        let (output, device) = setup(dir.path());
        run(&output, &device);

        assert!(sync_to_device(&output, &volume(&device, 0), |_| {}).is_ok());
    }

    #[test]
    fn refuses_a_missing_device_without_creating_it() {
        let dir = tempfile::tempdir().unwrap();
        let (output, device) = setup(dir.path());
        let gone = device.join("gone");

        assert!(sync_to_device(&output, &volume(&gone, u64::MAX), |_| {}).is_err());
        assert!(!gone.exists());
    }

    #[test]
    fn refuses_a_device_overlapping_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        let (output, _) = setup(dir.path());
        assert!(sync_to_device(&output, &volume(&output, u64::MAX), |_| {}).is_err());
    }

    #[test]
    fn stops_when_the_device_goes_away() {
        let dir = tempfile::tempdir().unwrap();
        let (output, device) = setup(dir.path());

        let report = sync_to_device(&output, &volume(&device, u64::MAX), |progress| {
            if progress.done == 2 {
                fs::remove_dir_all(&device).unwrap();
            }
        })
        .unwrap();

        assert!(report.stopped);
        assert_eq!(report.copied, 1);
        assert!(!device.exists());
    }

    #[test]
    fn detects_differing_contents() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b, c) = (
            dir.path().join("a"),
            dir.path().join("b"),
            dir.path().join("c"),
        );
        fs::write(&a, "same").unwrap();
        fs::write(&b, "same").unwrap();
        fs::write(&c, "diff").unwrap();
        assert!(same_contents(&a, &b).unwrap());
        assert!(!same_contents(&a, &c).unwrap());
    }

    #[test]
    fn cleans_up_only_the_attribute_file_of_a_given_path() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("._new.flac"), "x").unwrap();
        fs::write(dir.path().join("._older.flac"), "x").unwrap();

        remove_apple_double(&dir.path().join("new.flac"));

        assert!(!dir.path().join("._new.flac").exists());
        assert!(dir.path().join("._older.flac").exists());
    }
}
