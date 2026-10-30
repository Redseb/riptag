use super::cover::{CoverInfo, inspect_cover};
use lofty::config::ParseOptions;
use lofty::file::{AudioFile, FileType};
use lofty::flac::FlacFile;
use lofty::id3::v2::Id3v2Version;
use lofty::mpeg::MpegFile;
use lofty::prelude::*;
use std::fs::File;
use std::path::Path;

#[derive(Debug, Default, Clone)]
pub struct TrackMeta {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album_artist: Option<String>,
    pub album: Option<String>,
    pub year: Option<String>,
    pub track: Option<u32>,
    pub disc: Option<u32>,
    pub disc_total: Option<u32>,
    pub duration_secs: u64,
    pub cover: Option<CoverInfo>,
    /// An embedded picture exists but couldn't be read as an image.
    pub cover_unreadable: bool,
    /// Native field names. Only read for FLAC and MP3 so far.
    pub vorbis_fields: Vec<String>,
    pub id3_frames: Vec<String>,
    pub id3v24: bool,
}

/// Opens the file for reading only.
pub fn read_track(path: &Path) -> Result<TrackMeta, String> {
    let tagged = lofty::read_from_path(path).map_err(|e| e.to_string())?;
    let mut meta = TrackMeta {
        duration_secs: tagged.properties().duration().as_secs(),
        ..TrackMeta::default()
    };

    if let Some(tag) = tagged.primary_tag().or_else(|| tagged.first_tag()) {
        meta.title = tag.title().map(|s| s.into_owned());
        meta.artist = tag.artist().map(|s| s.into_owned());
        meta.album = tag.album().map(|s| s.into_owned());
        meta.album_artist = tag.get_string(ItemKey::AlbumArtist).map(str::to_owned);
        meta.year = tag
            .get_string(ItemKey::RecordingDate)
            .or_else(|| tag.get_string(ItemKey::Year))
            .and_then(year_of);
        meta.track = tag.track();
        meta.disc = tag.disk();
        meta.disc_total = tag.disk_total();
        if let Some(picture) = tag.pictures().first() {
            meta.cover = inspect_cover(picture.data());
            meta.cover_unreadable = meta.cover.is_none();
        }
    }

    let options = ParseOptions::new().read_properties(false);
    match tagged.file_type() {
        FileType::Flac => {
            let flac = FlacFile::read_from(&mut open(path)?, options).map_err(|e| e.to_string())?;
            if let Some(comments) = flac.vorbis_comments() {
                meta.vorbis_fields = comments.items().map(|(k, _)| k.to_uppercase()).collect();
            }
        }
        FileType::Mpeg => {
            let mp3 = MpegFile::read_from(&mut open(path)?, options).map_err(|e| e.to_string())?;
            if let Some(tag) = mp3.id3v2() {
                meta.id3v24 = tag.original_version() == Id3v2Version::V4;
                meta.id3_frames = tag.into_iter().map(|f| f.id_str().to_owned()).collect();
            }
        }
        _ => {}
    }

    Ok(meta)
}

fn open(path: &Path) -> Result<File, String> {
    File::open(path).map_err(|e| e.to_string())
}

fn year_of(date: &str) -> Option<String> {
    let year = date.get(..4)?;
    year.chars()
        .all(|c| c.is_ascii_digit())
        .then(|| year.to_owned())
}
