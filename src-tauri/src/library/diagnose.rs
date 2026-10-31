use super::cover::CoverInfo;
use super::profile::DeviceProfile;
use super::track::TrackMeta;
use serde::Serialize;
use std::collections::BTreeSet;
use unicode_normalization::{UnicodeNormalization, is_nfc};

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    pub code: &'static str,
    pub problem: &'static str,
    pub fix: &'static str,
    /// The affected file, relative to the album. `None` for the album itself.
    pub file: Option<String>,
    pub detail: Option<String>,
}

fn issue(code: &'static str, problem: &'static str, fix: &'static str) -> Issue {
    Issue {
        code,
        problem,
        fix,
        file: None,
        detail: None,
    }
}

// Together these keep a generated path under the profile's limit.
const MAX_FOLDER_CHARS: usize = 60;
const MAX_TITLE_CHARS: usize = 50;

const FAT_ILLEGAL: &[char] = &['<', '>', ':', '"', '/', '\\', '|', '?', '*'];

/// Makes a single path component safe for FAT32/exFAT.
pub fn sanitize_component(name: &str) -> String {
    let cleaned: String = name
        .nfc()
        .filter(|c| !FAT_ILLEGAL.contains(c) && !c.is_control())
        .collect();
    let trimmed = cleaned.trim_matches(|c| c == ' ' || c == '.');
    if trimmed.is_empty() {
        "_".to_owned()
    } else {
        trimmed.to_owned()
    }
}

/// Cuts a sanitised name to `max` characters, keeping it safe for FAT.
fn shorten(name: String, max: usize) -> String {
    if name.chars().count() <= max {
        return name;
    }
    let cut: String = name.chars().take(max).collect();
    cut.trim_end_matches([' ', '.']).to_owned()
}

/// The path a track should have, or `None` if its tags can't say.
pub fn expected_path(meta: &TrackMeta, extension: &str) -> Option<Vec<String>> {
    let artist = meta.album_artist.as_ref().or(meta.artist.as_ref())?;
    let album = meta.album.as_ref()?;
    let title = meta.title.as_ref()?;
    let track = meta.track?;

    let album_folder = match &meta.year {
        Some(year) => format!("{year} - {album}"),
        None => album.clone(),
    };
    let mut path = vec![
        shorten(sanitize_component(artist), MAX_FOLDER_CHARS),
        shorten(sanitize_component(&album_folder), MAX_FOLDER_CHARS),
    ];
    if meta.disc_total.unwrap_or(1) > 1 {
        path.push(format!("CD{}", meta.disc.unwrap_or(1)));
    }
    path.push(format!(
        "{track:02} {}.{extension}",
        shorten(sanitize_component(title), MAX_TITLE_CHARS)
    ));
    Some(path)
}

fn cover_issues(cover: &CoverInfo, profile: &DeviceProfile, embedded: bool) -> Vec<Issue> {
    let mut issues = Vec::new();
    let mut add = |embedded_issue: Issue, folder_issue: Issue| {
        issues.push(if embedded {
            embedded_issue
        } else {
            folder_issue
        });
    };
    if !cover.is_jpeg {
        add(
            issue(
                "cover_not_jpeg",
                "Embedded cover isn't a JPEG",
                "Re-encode as JPEG",
            ),
            issue(
                "folder_cover_not_jpeg",
                "cover.jpg isn't a JPEG",
                "Re-encode as JPEG",
            ),
        );
    }
    if cover.progressive {
        add(
            issue(
                "cover_progressive",
                "Embedded cover is progressive",
                "Re-encode as baseline",
            ),
            issue(
                "folder_cover_progressive",
                "cover.jpg is progressive",
                "Re-encode as baseline",
            ),
        );
    }
    if cover.width != cover.height {
        add(
            issue(
                "cover_not_square",
                "Embedded cover isn't square",
                "Centre-crop to square",
            ),
            issue(
                "folder_cover_not_square",
                "cover.jpg isn't square",
                "Centre-crop to square",
            ),
        );
    }
    if cover.width.max(cover.height) > profile.cover_size {
        add(
            issue(
                "cover_too_large",
                "Embedded cover is too large",
                "Resize to fit the device",
            ),
            issue(
                "folder_cover_too_large",
                "cover.jpg is too large",
                "Resize to fit the device",
            ),
        );
    }
    let dimensions = format!("{}×{}", cover.width, cover.height);
    for found in &mut issues {
        found.detail = Some(dimensions.clone());
    }
    issues
}

fn path_issues(components: &[String], profile: &DeviceProfile) -> Vec<Issue> {
    let mut issues = Vec::new();
    let illegal = components.iter().find(|c| {
        c.chars()
            .any(|ch| FAT_ILLEGAL.contains(&ch) || ch.is_control())
            || c.ends_with(['.', ' '])
    });
    if let Some(name) = illegal {
        let mut found = issue(
            "path_illegal_chars",
            "Name has characters FAT32/exFAT can't store",
            "Remove them from the name",
        );
        found.detail = Some(name.clone());
        issues.push(found);
    }
    if components.iter().any(|c| !is_nfc(c)) {
        issues.push(issue(
            "path_not_normalized",
            "Name uses decomposed Unicode",
            "Normalise to NFC",
        ));
    }
    let length: usize = components.iter().map(|c| c.chars().count() + 1).sum();
    if length > profile.max_path_chars {
        let mut found = issue("path_too_long", "Path is too long", "Shorten the path");
        found.detail = Some(format!("{length} characters"));
        issues.push(found);
    }
    issues
}

/// `components` is the track's path relative to the scanned folder.
pub fn diagnose_track(
    profile: &DeviceProfile,
    components: &[String],
    extension: &str,
    meta: &TrackMeta,
    has_lyrics: bool,
) -> Vec<Issue> {
    let mut issues = path_issues(components, profile);

    if !profile.playable_extensions.contains(&extension) {
        let mut found = issue(
            "format_unsupported",
            "Format doesn't play on the device",
            "Convert to FLAC",
        );
        found.detail = Some(extension.to_owned());
        issues.push(found);
    }

    let missing: Vec<&str> = [
        ("title", meta.title.is_none()),
        (
            "artist",
            meta.artist.is_none() && meta.album_artist.is_none(),
        ),
        ("album", meta.album.is_none()),
        ("track number", meta.track.is_none()),
    ]
    .iter()
    .filter(|(_, is_missing)| *is_missing)
    .map(|(name, _)| *name)
    .collect();
    if !missing.is_empty() {
        let mut found = issue(
            "tags_missing",
            "Required tags are missing",
            "Fill in the tags",
        );
        found.detail = Some(missing.join(", "));
        issues.push(found);
    }

    let extras: BTreeSet<String> = meta
        .vorbis_fields
        .iter()
        .filter(|f| !profile.vorbis_fields.contains(&f.as_str()))
        .chain(
            meta.id3_frames
                .iter()
                .filter(|f| !profile.id3_frames.contains(&f.as_str())),
        )
        .cloned()
        .chain(
            meta.other_items
                .iter()
                .filter(|key| !profile.tag_items.contains(key))
                .map(|key| format!("{key:?}")),
        )
        .collect();
    if !extras.is_empty() {
        let mut found = issue(
            "tags_extra",
            "Tags the device doesn't need can hide others",
            "Keep whitelisted tags only",
        );
        found.detail = Some(extras.into_iter().collect::<Vec<_>>().join(", "));
        issues.push(found);
    }

    if meta.id3v24 {
        issues.push(issue(
            "id3_v24",
            "ID3v2.4 reportedly breaks cover art",
            "Rewrite as ID3v2.3",
        ));
    }

    match &meta.cover {
        Some(cover) => issues.extend(cover_issues(cover, profile, true)),
        None if meta.cover_unreadable => issues.push(issue(
            "cover_unreadable",
            "Embedded cover can't be read",
            "Replace the cover",
        )),
        None => issues.push(issue(
            "cover_missing",
            "No embedded cover",
            "Embed the album cover",
        )),
    }

    if let Some(expected) = expected_path(meta, extension) {
        let nfc = |s: &String| s.nfc().collect::<String>();
        let differs = components
            .iter()
            .rev()
            .zip(expected.iter().rev())
            .any(|(actual, wanted)| nfc(actual) != nfc(wanted));
        if differs {
            let mut found = issue(
                "layout_mismatch",
                "Doesn't follow the folder layout",
                "Move to Artist/Year - Album/NN Title",
            );
            found.detail = Some(expected.join("/"));
            issues.push(found);
        }
    }

    if !has_lyrics {
        issues.push(issue(
            "lyrics_missing",
            "No .lrc lyrics file",
            "Fetch from LRCLIB",
        ));
    }

    issues
}

/// `folder_cover` is the outer `None` when the album has no cover.jpg, and the
/// inner `None` when it has one that can't be read.
pub fn diagnose_album(
    profile: &DeviceProfile,
    folder_cover: Option<Option<CoverInfo>>,
) -> Vec<Issue> {
    match folder_cover {
        None => vec![issue(
            "folder_cover_missing",
            "No cover.jpg in the album folder",
            "Write cover.jpg",
        )],
        Some(None) => vec![issue(
            "folder_cover_unreadable",
            "cover.jpg can't be read",
            "Replace cover.jpg",
        )],
        Some(Some(cover)) => cover_issues(&cover, profile, false),
    }
}

#[cfg(test)]
mod tests {
    use super::super::profile::ECHO_MINI;
    use super::*;

    fn good_cover() -> CoverInfo {
        CoverInfo {
            is_jpeg: true,
            progressive: false,
            width: 600,
            height: 600,
        }
    }

    fn good_meta() -> TrackMeta {
        TrackMeta {
            title: Some("Song".into()),
            artist: Some("Artist".into()),
            album: Some("Album".into()),
            year: Some("1999".into()),
            track: Some(3),
            cover: Some(good_cover()),
            ..TrackMeta::default()
        }
    }

    fn components(path: &str) -> Vec<String> {
        path.split('/').map(str::to_owned).collect()
    }

    fn codes(issues: &[Issue]) -> Vec<&'static str> {
        issues.iter().map(|i| i.code).collect()
    }

    fn diagnose(path: &str, meta: &TrackMeta) -> Vec<&'static str> {
        let issues = diagnose_track(&ECHO_MINI, &components(path), "flac", meta, true);
        codes(&issues)
    }

    #[test]
    fn compliant_track_has_no_issues() {
        let found = diagnose("Artist/1999 - Album/03 Song.flac", &good_meta());
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn layout_only_compares_folders_inside_the_scan() {
        assert!(diagnose("03 Song.flac", &good_meta()).is_empty());
        assert!(diagnose("Song.flac", &good_meta()).contains(&"layout_mismatch"));
    }

    #[test]
    fn multi_disc_albums_expect_a_disc_folder() {
        let meta = TrackMeta {
            disc: Some(2),
            disc_total: Some(2),
            ..good_meta()
        };
        assert!(diagnose("Artist/1999 - Album/CD2/03 Song.flac", &meta).is_empty());
        assert!(diagnose("Artist/1999 - Album/03 Song.flac", &meta).contains(&"layout_mismatch"));
    }

    #[test]
    fn flags_each_cover_defect() {
        let meta = TrackMeta {
            cover: Some(CoverInfo {
                is_jpeg: true,
                progressive: true,
                width: 1200,
                height: 1000,
            }),
            ..good_meta()
        };
        let found = diagnose("Artist/1999 - Album/03 Song.flac", &meta);
        assert_eq!(
            found,
            ["cover_progressive", "cover_not_square", "cover_too_large"]
        );
    }

    #[test]
    fn flags_tags_outside_the_whitelist() {
        let meta = TrackMeta {
            vorbis_fields: vec!["TITLE".into(), "MOOD".into()],
            ..good_meta()
        };
        let issues = diagnose_track(
            &ECHO_MINI,
            &components("Artist/1999 - Album/03 Song.flac"),
            "flac",
            &meta,
            true,
        );
        assert_eq!(codes(&issues), ["tags_extra"]);
        assert_eq!(issues[0].detail.as_deref(), Some("MOOD"));
    }

    #[test]
    fn flags_decomposed_diacritics_but_accepts_composed_ones() {
        let meta = TrackMeta {
            title: Some("Żółć".nfc().collect()),
            ..good_meta()
        };
        let composed: String = "Artist/1999 - Album/03 Żółć.flac".nfc().collect();
        let decomposed: String = composed.nfd().collect();
        assert!(diagnose(&composed, &meta).is_empty());
        assert_eq!(diagnose(&decomposed, &meta), ["path_not_normalized"]);
    }

    #[test]
    fn sanitize_strips_what_fat_cannot_store() {
        assert_eq!(sanitize_component(" AC/DC: Live? . "), "ACDC Live");
    }

    #[test]
    fn sanitize_never_returns_an_empty_name() {
        assert_eq!(sanitize_component("???"), "_");
    }

    #[test]
    fn long_tags_produce_a_path_within_the_limit() {
        let long = "x".repeat(300);
        let meta = TrackMeta {
            title: Some(long.clone()),
            artist: Some(long.clone()),
            album: Some(long),
            track: Some(1),
            disc: Some(1),
            disc_total: Some(2),
            ..TrackMeta::default()
        };
        let path = expected_path(&meta, "flac").unwrap();
        let length: usize = path.iter().map(|c| c.chars().count() + 1).sum();
        assert!(length <= ECHO_MINI.max_path_chars, "{length}");
    }

    #[test]
    fn album_needs_a_cover_file() {
        assert_eq!(
            codes(&diagnose_album(&ECHO_MINI, None)),
            ["folder_cover_missing"]
        );
        assert!(diagnose_album(&ECHO_MINI, Some(Some(good_cover()))).is_empty());
    }
}
