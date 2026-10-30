use super::cover::{inspect_cover, is_compliant};
use super::diagnose::{Issue, expected_path, sanitize_component};
use super::profile::DeviceProfile;
use super::scan::{
    find_albums, find_folder_cover, lowercase_extension, relative_components, scan_library,
};
use super::track::read_track;
use image::imageops::FilterType;
use image::{DynamicImage, ImageReader};
use lofty::config::{ParseOptions, WriteOptions};
use lofty::file::AudioFile;
use lofty::flac::FlacFile;
use lofty::id3::v2::Id3v2Tag;
use lofty::mpeg::MpegFile;
use lofty::ogg::OggPictureStorage;
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::prelude::*;
use serde::Serialize;
use std::collections::{BTreeSet, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Cursor, Seek};
use std::path::{Path, PathBuf};
use unicode_normalization::UnicodeNormalization;

const JPEG_QUALITY: u8 = 90;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkippedFile {
    pub file: String,
    pub reason: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepairReport {
    pub output: String,
    pub written: usize,
    /// Written with new paths only, as tags are only rewritten for FLAC and MP3.
    pub tags_untouched: usize,
    pub skipped: Vec<SkippedFile>,
    /// What a scan of the output folder still flags.
    pub remaining: Vec<Issue>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepairProgress {
    pub done: usize,
    pub total: usize,
}

/// Resolves symlinks in the part of the path that exists, without creating anything.
fn resolve(path: &Path) -> Result<PathBuf, String> {
    let mut existing = path;
    let mut rest = Vec::new();
    while !existing.exists() {
        rest.push(existing.file_name().ok_or("Invalid folder")?);
        existing = existing.parent().ok_or("Invalid folder")?;
    }
    let mut resolved = existing.canonicalize().map_err(|e| e.to_string())?;
    resolved.extend(rest.iter().rev());
    Ok(resolved)
}

/// Compared loosely, because the filesystem may ignore case and normalisation.
fn comparable(path: &Path) -> Vec<String> {
    path.iter()
        .map(|c| c.to_string_lossy().nfc().collect::<String>().to_lowercase())
        .collect()
}

fn ensure_separate(source: &Path, output: &Path) -> Result<(), String> {
    let source = comparable(&resolve(source)?);
    let output = comparable(&resolve(output)?);
    if output.starts_with(&source) || source.starts_with(&output) {
        return Err("The output folder can't overlap the source folder".to_owned());
    }
    Ok(())
}

/// Centre-crops to a square and resizes down, as a baseline JPEG. A cover that
/// already complies keeps its bytes, so it isn't recompressed.
pub fn prepare_cover(bytes: &[u8], profile: &DeviceProfile) -> Result<Vec<u8>, String> {
    let info = inspect_cover(bytes).ok_or("Cover can't be read")?;
    if is_compliant(&info, profile) {
        return Ok(bytes.to_vec());
    }
    let image = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| e.to_string())?
        .decode()
        .map_err(|e| e.to_string())?;
    let side = image.width().min(image.height());
    let cropped = image.crop_imm(
        (image.width() - side) / 2,
        (image.height() - side) / 2,
        side,
        side,
    );
    let target = side.min(profile.cover_size);
    let resized = if target < side {
        cropped.resize_exact(target, target, FilterType::Lanczos3)
    } else {
        cropped
    };
    let rgb = DynamicImage::ImageRgb8(resized.to_rgb8());
    let mut out = Vec::new();
    rgb.write_with_encoder(image::codecs::jpeg::JpegEncoder::new_with_quality(
        &mut out,
        JPEG_QUALITY,
    ))
    .map_err(|e| e.to_string())?;
    Ok(out)
}

fn embedded_cover(path: &Path) -> Option<Vec<u8>> {
    let tagged = lofty::read_from_path(path).ok()?;
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag())?;
    Some(tag.pictures().first()?.data().to_vec())
}

/// The album's best cover: `cover.jpg` if it's usable, else the first embedded one.
fn album_cover(folder: &Path, tracks: &[PathBuf], profile: &DeviceProfile) -> Option<Vec<u8>> {
    let from_folder = find_folder_cover(folder)
        .and_then(|path| fs::read(path).ok())
        .and_then(|bytes| prepare_cover(&bytes, profile).ok());
    from_folder.or_else(|| {
        tracks
            .iter()
            .filter_map(|track| embedded_cover(track))
            .find_map(|bytes| prepare_cover(&bytes, profile).ok())
    })
}

fn cover_picture(cover: &[u8]) -> Picture {
    Picture::unchecked(cover.to_vec())
        .pic_type(PictureType::CoverFront)
        .mime_type(MimeType::Jpeg)
        .build()
}

fn read_only_tags() -> ParseOptions {
    ParseOptions::new().read_properties(false)
}

fn rewrite_flac(path: &Path, cover: Option<&[u8]>, profile: &DeviceProfile) -> Result<(), String> {
    let mut file = open_rw(path)?;
    let mut flac = FlacFile::read_from(&mut file, read_only_tags()).map_err(|e| e.to_string())?;
    if let Some(comments) = flac.vorbis_comments_mut() {
        let items: Vec<_> = comments.take_items().collect();
        for (key, value) in items {
            if profile.vorbis_fields.contains(&key.to_uppercase().as_str()) {
                comments.push(key, value);
            }
        }
        comments.remove_pictures();
    }
    flac.remove_pictures();
    if let Some(cover) = cover {
        flac.insert_picture(cover_picture(cover), None)
            .map_err(|e| e.to_string())?;
    }
    file.rewind().map_err(|e| e.to_string())?;
    flac.save_to(&mut file, WriteOptions::new())
        .map_err(|e| e.to_string())
}

fn rewrite_mp3(path: &Path, cover: Option<&[u8]>, profile: &DeviceProfile) -> Result<(), String> {
    let mut file = open_rw(path)?;
    let mp3 = MpegFile::read_from(&mut file, read_only_tags()).map_err(|e| e.to_string())?;
    let mut tag = mp3.id3v2().cloned().unwrap_or_else(Id3v2Tag::new);
    tag.retain(|frame| frame.id_str() != "APIC" && profile.id3_frames.contains(&frame.id_str()));
    if let Some(cover) = cover {
        tag.insert_picture(cover_picture(cover));
    }
    file.rewind().map_err(|e| e.to_string())?;
    tag.save_to(&mut file, WriteOptions::new().use_id3v23(true))
        .map_err(|e| e.to_string())
}

fn open_rw(path: &Path) -> Result<File, String> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .map_err(|e| e.to_string())
}

/// Builds the copy under a hidden name and renames it last, so an interrupted
/// run never leaves a half-written track looking finished.
fn write_copy(
    source: &Path,
    dest: &Path,
    extension: &str,
    cover: Option<&[u8]>,
    profile: &DeviceProfile,
) -> Result<(), String> {
    let parent = dest.parent().ok_or("Invalid destination")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let name = dest.file_name().ok_or("Invalid destination")?;
    let temp = parent.join(format!(".{}.part", name.to_string_lossy()));

    let result = (|| {
        fs::copy(source, &temp).map_err(|e| e.to_string())?;
        match extension {
            "flac" => rewrite_flac(&temp, cover, profile)?,
            "mp3" => rewrite_mp3(&temp, cover, profile)?,
            _ => {}
        }
        fs::rename(&temp, dest).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

fn copy_if_absent(from: &Path, to: &Path) -> Result<(), String> {
    if to.exists() {
        return Ok(());
    }
    fs::copy(from, to).map(|_| ()).map_err(|e| e.to_string())
}

/// Writes repaired copies of everything under `source` into `output`. Only ever
/// reads from `source`, and never replaces a file already in `output`.
pub fn repair_library(
    source: &Path,
    output: &Path,
    profile: &DeviceProfile,
    mut on_progress: impl FnMut(RepairProgress),
) -> Result<RepairReport, String> {
    if !source.is_dir() {
        return Err(format!("Not a folder: {}", source.display()));
    }
    ensure_separate(source, output)?;
    fs::create_dir_all(output).map_err(|e| e.to_string())?;

    let albums = find_albums(source, profile);
    let total = albums.values().map(Vec::len).sum();
    let mut done = 0;
    let mut written = 0;
    let mut tags_untouched = 0;
    let mut skipped = Vec::new();
    // On FAT, names differing only by case or normalisation are the same file.
    let mut claimed = HashSet::new();

    for (folder, tracks) in &albums {
        let cover = album_cover(folder, tracks, profile);
        let mut dest_folders = BTreeSet::new();

        for track in tracks {
            let components = relative_components(track, source);
            let mut skip = |reason: String| {
                skipped.push(SkippedFile {
                    file: components.join("/"),
                    reason,
                });
            };
            done += 1;
            on_progress(RepairProgress { done, total });

            let extension = lowercase_extension(track).unwrap_or_default();
            if !profile.playable_extensions.contains(&extension.as_str()) {
                skip(format!("{extension} needs converting, not built yet"));
                continue;
            }
            let meta = match read_track(track) {
                Ok(meta) => meta,
                Err(error) => {
                    skip(format!("Can't be read: {error}"));
                    continue;
                }
            };
            let relative = expected_path(&meta, &extension)
                .unwrap_or_else(|| components.iter().map(|c| sanitize_component(c)).collect());
            let dest: PathBuf = output.join(relative.iter().collect::<PathBuf>());

            if !claimed.insert(comparable(&dest)) {
                skip(format!(
                    "Same name as another track: {}",
                    relative.join("/")
                ));
                continue;
            }
            if dest.exists() {
                skip(format!("Already in the output: {}", relative.join("/")));
                continue;
            }
            if let Err(error) = write_copy(track, &dest, &extension, cover.as_deref(), profile) {
                skip(format!("Couldn't write: {error}"));
                continue;
            }
            written += 1;
            if extension != "flac" && extension != "mp3" {
                tags_untouched += 1;
            }

            let lyrics = track.with_extension("lrc");
            if lyrics.exists()
                && let Err(error) = copy_if_absent(&lyrics, &dest.with_extension("lrc"))
            {
                skip(format!("Lyrics couldn't be copied: {error}"));
            }
            if let Some(parent) = dest.parent() {
                dest_folders.insert(parent.to_path_buf());
            }
        }

        if let Some(cover) = &cover {
            for dest_folder in dest_folders {
                let path = dest_folder.join("cover.jpg");
                if !path.exists() {
                    fs::write(&path, cover).map_err(|e| e.to_string())?;
                }
            }
        }
    }

    let remaining = scan_library(output, profile)?
        .albums
        .into_iter()
        .flat_map(|album| album.issues)
        .collect();

    Ok(RepairReport {
        output: output.to_string_lossy().into_owned(),
        written,
        tags_untouched,
        skipped,
        remaining,
    })
}

#[cfg(test)]
mod tests {
    use super::super::profile::ECHO_MINI;
    use super::*;
    use image::{ImageFormat, RgbImage};
    use walkdir::WalkDir;

    const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

    fn image_bytes(width: u32, height: u32, format: ImageFormat) -> Vec<u8> {
        let mut bytes = Vec::new();
        RgbImage::new(width, height)
            .write_to(&mut Cursor::new(&mut bytes), format)
            .unwrap();
        bytes
    }

    fn snapshot(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
        WalkDir::new(dir)
            .sort_by_file_name()
            .into_iter()
            .flatten()
            .filter(|e| e.file_type().is_file())
            .map(|e| (e.path().to_path_buf(), fs::read(e.path()).unwrap()))
            .collect()
    }

    fn fixture_library(root: &Path) {
        let album = root.join("Some Folder");
        fs::create_dir_all(&album).unwrap();
        fs::copy(format!("{FIXTURES}/sample.flac"), album.join("a.flac")).unwrap();
        fs::copy(format!("{FIXTURES}/sample.mp3"), album.join("b.mp3")).unwrap();
        fs::write(album.join("a.lrc"), "[00:00.00]la").unwrap();
    }

    fn run(source: &Path, output: &Path) -> RepairReport {
        repair_library(source, output, &ECHO_MINI, |_| {}).unwrap()
    }

    #[test]
    fn prepares_a_square_baseline_jpeg() {
        let cover = prepare_cover(&image_bytes(1200, 800, ImageFormat::Png), &ECHO_MINI).unwrap();
        let info = inspect_cover(&cover).unwrap();
        assert!(info.is_jpeg && !info.progressive);
        assert_eq!((info.width, info.height), (600, 600));
    }

    #[test]
    fn converts_a_real_progressive_jpeg_to_baseline() {
        let bytes = fs::read(format!("{FIXTURES}/progressive.jpg")).unwrap();
        assert!(inspect_cover(&bytes).unwrap().progressive);

        let info = inspect_cover(&prepare_cover(&bytes, &ECHO_MINI).unwrap()).unwrap();
        assert!(!info.progressive);
        assert_eq!((info.width, info.height), (48, 48));
    }

    #[test]
    fn does_not_enlarge_small_covers() {
        let cover = prepare_cover(&image_bytes(100, 80, ImageFormat::Png), &ECHO_MINI).unwrap();
        let info = inspect_cover(&cover).unwrap();
        assert_eq!((info.width, info.height), (80, 80));
    }

    #[test]
    fn keeps_compliant_cover_bytes() {
        let original = image_bytes(600, 600, ImageFormat::Jpeg);
        assert_eq!(prepare_cover(&original, &ECHO_MINI).unwrap(), original);
    }

    #[test]
    fn output_fixes_everything_but_lyrics_and_source_is_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let (source, output) = (dir.path().join("in"), dir.path().join("out"));
        fixture_library(&source);
        let before = snapshot(&source);

        let report = run(&source, &output);

        assert_eq!(snapshot(&source), before);
        assert_eq!(report.written, 2);
        assert!(report.skipped.is_empty(), "{:?}", report.skipped);
        let album = output.join("Fixture/Tests");
        for name in [
            "01 Sample.flac",
            "01 Sample.mp3",
            "01 Sample.lrc",
            "cover.jpg",
        ] {
            assert!(album.join(name).exists(), "missing {name}");
        }
        // Two tracks share a base name in the fixture, so one `.lrc` serves both.
        assert!(report.remaining.is_empty(), "{:?}", report.remaining);

        let flac = read_track(&album.join("01 Sample.flac")).unwrap();
        assert_eq!(flac.title.as_deref(), Some("Sample"));
        assert_eq!(flac.track, Some(1));
        let mp3 = read_track(&album.join("01 Sample.mp3")).unwrap();
        assert_eq!(mp3.title.as_deref(), Some("Sample"));
        assert!(!mp3.id3v24);
        assert!(mp3.cover.is_some());
    }

    #[test]
    fn keeps_flac_audio_bytes_identical() {
        let dir = tempfile::tempdir().unwrap();
        let (source, output) = (dir.path().join("in"), dir.path().join("out"));
        fixture_library(&source);
        run(&source, &output);

        let original = fs::read(source.join("Some Folder/a.flac")).unwrap();
        let copy = fs::read(output.join("Fixture/Tests/01 Sample.flac")).unwrap();
        let frame_sync = |bytes: &[u8]| {
            // The last metadata block ends where the first audio frame starts.
            let mut pos = 4;
            loop {
                let last = bytes[pos] & 0x80 != 0;
                let len = u32::from_be_bytes([0, bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]]);
                pos += 4 + len as usize;
                if last {
                    return pos;
                }
            }
        };
        assert_eq!(copy[frame_sync(&copy)..], original[frame_sync(&original)..]);
    }

    #[test]
    fn refuses_overlapping_folders_without_creating_anything() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("in");
        fixture_library(&source);
        let before = snapshot(&source);

        for output in [
            source.clone(),
            source.join("fixed"),
            dir.path().to_path_buf(),
        ] {
            assert!(repair_library(&source, &output, &ECHO_MINI, |_| {}).is_err());
        }
        assert_eq!(snapshot(&source), before);
        assert!(!source.join("fixed").exists());
    }

    #[test]
    fn never_replaces_what_is_already_in_the_output() {
        let dir = tempfile::tempdir().unwrap();
        let (source, output) = (dir.path().join("in"), dir.path().join("out"));
        fixture_library(&source);
        let existing = output.join("Fixture/Tests/01 Sample.flac");
        fs::create_dir_all(existing.parent().unwrap()).unwrap();
        fs::write(&existing, "mine").unwrap();

        let report = run(&source, &output);

        assert_eq!(fs::read(&existing).unwrap(), b"mine");
        assert_eq!(report.written, 1);
        assert!(
            report.skipped[0]
                .reason
                .starts_with("Already in the output")
        );
    }

    #[test]
    fn tracks_resolving_to_one_name_do_not_overwrite_each_other() {
        let dir = tempfile::tempdir().unwrap();
        let (source, output) = (dir.path().join("in"), dir.path().join("out"));
        fs::create_dir_all(&source).unwrap();
        fs::copy(format!("{FIXTURES}/sample.flac"), source.join("a.flac")).unwrap();
        fs::copy(format!("{FIXTURES}/sample.flac"), source.join("b.flac")).unwrap();

        let report = run(&source, &output);

        assert_eq!(report.written, 1);
        assert!(report.skipped[0].reason.starts_with("Same name"));
    }

    #[test]
    fn unreadable_and_unsupported_files_are_reported_not_fatal() {
        let dir = tempfile::tempdir().unwrap();
        let (source, output) = (dir.path().join("in"), dir.path().join("out"));
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("broken.flac"), "nope").unwrap();
        fs::write(source.join("song.opus"), "nope").unwrap();

        let report = run(&source, &output);

        assert_eq!(report.written, 0);
        assert_eq!(report.skipped.len(), 2);
    }

    #[test]
    fn untagged_tracks_keep_their_folder_with_safe_names() {
        let dir = tempfile::tempdir().unwrap();
        let (source, output) = (dir.path().join("in"), dir.path().join("out"));
        // Windows can't create this name, so the sanitising only matters elsewhere.
        let album = source.join(if cfg!(windows) {
            "Odd Folder"
        } else {
            "Odd: Folder"
        });
        fs::create_dir_all(&album).unwrap();
        fs::copy(format!("{FIXTURES}/untagged.flac"), album.join("x.flac")).unwrap();

        let report = run(&source, &output);

        assert_eq!(report.written, 1, "{:?}", report.skipped);
        assert!(output.join("Odd Folder/x.flac").exists());
        assert!(report.remaining.iter().any(|i| i.code == "tags_missing"));
    }
}
