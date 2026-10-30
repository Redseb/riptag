use super::cover::{CoverInfo, inspect_cover};
use super::diagnose::{Issue, diagnose_album, diagnose_track};
use super::profile::DeviceProfile;
use super::track::{TrackMeta, read_track};
use serde::Serialize;
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Audio we recognise but the device may not play, so it gets flagged rather
/// than skipped.
const OTHER_AUDIO_EXTENSIONS: &[&str] = &["opus", "aac", "aif", "aiff", "wv", "mpc", "m4b"];

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanResult {
    pub root: String,
    pub profile: &'static str,
    pub track_count: usize,
    pub albums: Vec<Album>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Album {
    pub path: String,
    pub name: String,
    pub track_count: usize,
    pub issues: Vec<Issue>,
}

pub(super) fn lowercase_extension(path: &Path) -> Option<String> {
    path.extension()
        .and_then(OsStr::to_str)
        .map(str::to_lowercase)
}

fn is_audio(path: &Path, profile: &DeviceProfile) -> bool {
    lowercase_extension(path).is_some_and(|ext| {
        profile.playable_extensions.contains(&ext.as_str())
            || OTHER_AUDIO_EXTENSIONS.contains(&ext.as_str())
    })
}

fn is_hidden(name: &OsStr) -> bool {
    name.to_string_lossy().starts_with('.')
}

pub(super) fn relative_components(path: &Path, root: &Path) -> Vec<String> {
    path.strip_prefix(root)
        .unwrap_or(path)
        .iter()
        .map(|c| c.to_string_lossy().into_owned())
        .collect()
}

pub(super) fn find_folder_cover(folder: &Path) -> Option<PathBuf> {
    fs::read_dir(folder)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| {
            p.file_name()
                .is_some_and(|n| n.eq_ignore_ascii_case("cover.jpg"))
        })
}

fn read_folder_cover(folder: &Path) -> Option<Option<CoverInfo>> {
    let path = find_folder_cover(folder)?;
    Some(fs::read(path).ok().and_then(|bytes| inspect_cover(&bytes)))
}

fn diagnose_file(path: &Path, root: &Path, profile: &DeviceProfile) -> Vec<Issue> {
    let components = relative_components(path, root);
    let extension = lowercase_extension(path).unwrap_or_default();
    let has_lyrics = path.with_extension("lrc").exists();
    match read_track(path) {
        Ok(meta) => diagnose_track(profile, &components, &extension, &meta, has_lyrics),
        Err(error) => {
            // Path and format checks still apply to a file we can't parse.
            let mut issues = diagnose_track(
                profile,
                &components,
                &extension,
                &TrackMeta::default(),
                has_lyrics,
            );
            issues.retain(|i| i.code.starts_with("path_") || i.code == "format_unsupported");
            issues.insert(
                0,
                Issue {
                    code: "unreadable",
                    problem: "File can't be read",
                    fix: "Check the file by hand",
                    file: None,
                    detail: Some(error),
                },
            );
            issues
        }
    }
}

/// Every folder under `root` holding audio, with its tracks in name order.
pub(super) fn find_albums(root: &Path, profile: &DeviceProfile) -> BTreeMap<PathBuf, Vec<PathBuf>> {
    let mut folders: BTreeMap<PathBuf, Vec<PathBuf>> = BTreeMap::new();
    let walker = WalkDir::new(root)
        .sort_by_file_name()
        .into_iter()
        .filter_entry(|e| e.depth() == 0 || !is_hidden(e.file_name()));
    for entry in walker.flatten() {
        let path = entry.path();
        if entry.file_type().is_file() && is_audio(path, profile) {
            let folder = path.parent().unwrap_or(root).to_path_buf();
            folders.entry(folder).or_default().push(path.to_path_buf());
        }
    }
    folders
}

/// Walks `root` without changing anything. Each folder holding audio is an album.
pub fn scan_library(root: &Path, profile: &DeviceProfile) -> Result<ScanResult, String> {
    if !root.is_dir() {
        return Err(format!("Not a folder: {}", root.display()));
    }

    let folders = find_albums(root, profile);

    let mut track_count = 0;
    let albums = folders
        .into_iter()
        .map(|(folder, tracks)| {
            track_count += tracks.len();
            let mut issues = diagnose_album(profile, read_folder_cover(&folder));
            for track in &tracks {
                let file = track.file_name().map(|n| n.to_string_lossy().into_owned());
                for mut found in diagnose_file(track, root, profile) {
                    found.file = file.clone();
                    issues.push(found);
                }
            }
            let name = relative_components(&folder, root).join("/");
            Album {
                path: folder.to_string_lossy().into_owned(),
                name: if name.is_empty() {
                    folder
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default()
                } else {
                    name
                },
                track_count: tracks.len(),
                issues,
            }
        })
        .collect();

    Ok(ScanResult {
        root: root.to_string_lossy().into_owned(),
        profile: profile.name,
        track_count,
        albums,
    })
}

#[cfg(test)]
mod tests {
    use super::super::profile::ECHO_MINI;
    use super::*;

    const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

    fn snapshot(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
        WalkDir::new(dir)
            .sort_by_file_name()
            .into_iter()
            .flatten()
            .filter(|e| e.file_type().is_file())
            .map(|e| (e.path().to_path_buf(), fs::read(e.path()).unwrap()))
            .collect()
    }

    fn copy_fixture(name: &str, to: &Path) {
        fs::create_dir_all(to.parent().unwrap()).unwrap();
        fs::copy(Path::new(FIXTURES).join(name), to).unwrap();
    }

    fn scan_codes(album: &Album, file: &str) -> Vec<&'static str> {
        album
            .issues
            .iter()
            .filter(|i| i.file.as_deref() == Some(file))
            .map(|i| i.code)
            .collect()
    }

    #[test]
    fn diagnoses_a_real_flac_and_mp3_without_touching_them() {
        let dir = tempfile::tempdir().unwrap();
        let album = dir.path().join("Some Folder");
        copy_fixture("sample.flac", &album.join("track.flac"));
        copy_fixture("sample.mp3", &album.join("track.mp3"));
        fs::write(album.join("notes.txt"), "not audio").unwrap();
        fs::write(album.join(".hidden.mp3"), "AppleDouble junk").unwrap();
        let before = snapshot(dir.path());

        let result = scan_library(dir.path(), &ECHO_MINI).unwrap();

        assert_eq!(result.track_count, 2);
        assert_eq!(result.albums.len(), 1);
        let album = &result.albums[0];
        assert_eq!(album.name, "Some Folder");
        assert!(
            album
                .issues
                .iter()
                .any(|i| i.code == "folder_cover_missing")
        );

        let flac = scan_codes(album, "track.flac");
        for code in [
            "tags_extra",
            "cover_not_jpeg",
            "cover_not_square",
            "layout_mismatch",
            "lyrics_missing",
        ] {
            assert!(flac.contains(&code), "{code} not in {flac:?}");
        }
        let mp3 = scan_codes(album, "track.mp3");
        for code in ["tags_extra", "id3_v24", "cover_missing", "lyrics_missing"] {
            assert!(mp3.contains(&code), "{code} not in {mp3:?}");
        }

        assert_eq!(snapshot(dir.path()), before);
    }

    #[test]
    fn corrupt_audio_is_flagged_not_fatal() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("broken.flac"), "not really flac").unwrap();
        let result = scan_library(dir.path(), &ECHO_MINI).unwrap();
        assert_eq!(scan_codes(&result.albums[0], "broken.flac"), ["unreadable"]);
    }

    #[test]
    fn missing_folder_is_an_error() {
        assert!(scan_library(Path::new("/definitely/not/here"), &ECHO_MINI).is_err());
    }

    #[test]
    fn unsupported_format_is_flagged() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("song.opus"), "x").unwrap();
        let result = scan_library(dir.path(), &ECHO_MINI).unwrap();
        assert!(scan_codes(&result.albums[0], "song.opus").contains(&"format_unsupported"));
    }
}
