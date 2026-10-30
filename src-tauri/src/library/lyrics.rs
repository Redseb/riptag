use super::diagnose::Issue;
use super::profile::DeviceProfile;
use super::repair::{RepairProgress, SkippedFile, ensure_separate};
use super::scan::{find_albums, lowercase_extension, relative_components, scan_library};
use super::track::read_track;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::time::Duration;

const API: &str = "https://lrclib.net/api";
const USER_AGENT: &str = concat!(
    "Riptag/",
    env!("CARGO_PKG_VERSION"),
    " (https://github.com/Redseb/riptag)"
);
const TIMEOUT: Duration = Duration::from_secs(15);
/// LRCLIB treats tracks this close in length as the same recording.
const DURATION_TOLERANCE_SECS: f64 = 2.0;
/// Gives up rather than wait out a timeout per track when offline.
const MAX_CONSECUTIVE_ERRORS: usize = 3;
/// Waits before each retry of a request that failed temporarily.
const RETRY_DELAYS: [Duration; 2] = [Duration::from_secs(2), Duration::from_secs(5)];

#[derive(Debug, Clone, PartialEq)]
pub struct LyricsQuery {
    pub artist: String,
    pub title: String,
    pub album: Option<String>,
    pub duration_secs: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LyricsResult {
    Synced(String),
    /// Only unsynced text exists. Not written, as a `.lrc` needs timestamps.
    PlainOnly,
    Instrumental,
    NotFound,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricsReport {
    pub saved: usize,
    pub already_had: usize,
    pub plain_only: usize,
    pub instrumental: usize,
    pub not_found: usize,
    /// Set when repeated errors ended the run early.
    pub stopped: bool,
    pub failed: Vec<SkippedFile>,
    pub remaining: Vec<Issue>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LrclibRecord {
    #[serde(default)]
    instrumental: bool,
    #[serde(default)]
    duration: Option<f64>,
    plain_lyrics: Option<String>,
    synced_lyrics: Option<String>,
}

fn has_text(text: &Option<String>) -> bool {
    text.as_deref().is_some_and(|t| !t.trim().is_empty())
}

fn classify(record: LrclibRecord) -> LyricsResult {
    if has_text(&record.synced_lyrics) {
        LyricsResult::Synced(record.synced_lyrics.unwrap_or_default())
    } else if record.instrumental {
        LyricsResult::Instrumental
    } else if has_text(&record.plain_lyrics) {
        LyricsResult::PlainOnly
    } else {
        LyricsResult::NotFound
    }
}

/// Picks the best search result of the right length, preferring synced lyrics.
fn best_match(records: Vec<LrclibRecord>, duration_secs: u64) -> LyricsResult {
    let mut best = LyricsResult::NotFound;
    for record in records {
        let close = record
            .duration
            .is_some_and(|d| (d - duration_secs as f64).abs() <= DURATION_TOLERANCE_SECS);
        if !close {
            continue;
        }
        match classify(record) {
            synced @ LyricsResult::Synced(_) => return synced,
            LyricsResult::NotFound => {}
            other => {
                if best == LyricsResult::NotFound {
                    best = other;
                }
            }
        }
    }
    best
}

fn is_temporary(error: &ureq::Error) -> bool {
    match error {
        ureq::Error::StatusCode(code) => *code == 429 || *code >= 500,
        ureq::Error::Timeout(_) | ureq::Error::Io(_) | ureq::Error::HostNotFound => true,
        _ => false,
    }
}

/// LRCLIB returns intermittent 503s under load, which usually clear on a retry.
fn with_retries<T>(
    mut send: impl FnMut() -> Result<T, ureq::Error>,
    mut wait: impl FnMut(Duration),
) -> Result<T, ureq::Error> {
    let mut delays = RETRY_DELAYS.iter();
    loop {
        match send() {
            Err(error) if is_temporary(&error) => match delays.next() {
                Some(delay) => wait(*delay),
                None => return Err(error),
            },
            result => return result,
        }
    }
}

/// Asks for the exact album first, then searches, as album names vary a lot.
pub fn lrclib_lookup(query: &LyricsQuery) -> Result<LyricsResult, String> {
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .user_agent(USER_AGENT)
        .build()
        .new_agent();

    if let Some(album) = &query.album {
        let response = with_retries(
            || {
                agent
                    .get(format!("{API}/get"))
                    .query("artist_name", &query.artist)
                    .query("track_name", &query.title)
                    .query("album_name", album)
                    .query("duration", query.duration_secs.to_string())
                    .call()
            },
            std::thread::sleep,
        );
        // Any failure here falls through, as search can still answer.
        if let Ok(mut response) = response
            && let Ok(record) = response.body_mut().read_json::<LrclibRecord>()
            && let found @ LyricsResult::Synced(_) = classify(record)
        {
            return Ok(found);
        }
    }

    let records: Vec<LrclibRecord> = with_retries(
        || {
            agent
                .get(format!("{API}/search"))
                .query("artist_name", &query.artist)
                .query("track_name", &query.title)
                .call()
        },
        std::thread::sleep,
    )
    .map_err(|e| e.to_string())?
    .body_mut()
    .read_json()
    .map_err(|e| e.to_string())?;
    Ok(best_match(records, query.duration_secs))
}

fn write_lyrics(path: &Path, text: &str) -> Result<(), String> {
    let temp = path.with_extension("lrc.part");
    let result = fs::write(&temp, text)
        .and_then(|()| fs::rename(&temp, path))
        .map_err(|e| e.to_string());
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

/// Adds synced `.lrc` files to `output`. `source` is the library it was repaired
/// from, so that originals can't be written to by mistake.
pub fn fetch_lyrics(
    source: &Path,
    output: &Path,
    profile: &DeviceProfile,
    mut lookup: impl FnMut(&LyricsQuery) -> Result<LyricsResult, String>,
    mut on_progress: impl FnMut(RepairProgress),
) -> Result<LyricsReport, String> {
    if !output.is_dir() {
        return Err(format!("Not a folder: {}", output.display()));
    }
    ensure_separate(source, output)?;

    let tracks: Vec<_> = find_albums(output, profile)
        .into_values()
        .flatten()
        .filter(|track| {
            lowercase_extension(track)
                .is_some_and(|ext| profile.playable_extensions.contains(&ext.as_str()))
        })
        .collect();
    let total = tracks.len();
    let mut report = LyricsReport::default();
    let mut consecutive_errors = 0;

    for (index, track) in tracks.iter().enumerate() {
        on_progress(RepairProgress {
            done: index + 1,
            total,
        });
        let lyrics_path = track.with_extension("lrc");
        if lyrics_path.exists() {
            report.already_had += 1;
            continue;
        }
        let file = relative_components(track, output).join("/");
        let mut fail = |reason: String| {
            report.failed.push(SkippedFile {
                file: file.clone(),
                reason,
            });
        };

        let meta = match read_track(track) {
            Ok(meta) => meta,
            Err(error) => {
                fail(format!("Can't be read: {error}"));
                continue;
            }
        };
        let (Some(artist), Some(title)) = (meta.artist.or(meta.album_artist), meta.title) else {
            report.not_found += 1;
            continue;
        };
        let query = LyricsQuery {
            artist,
            title,
            album: meta.album,
            duration_secs: meta.duration_secs,
        };

        match lookup(&query) {
            Ok(result) => {
                consecutive_errors = 0;
                match result {
                    LyricsResult::Synced(text) => match write_lyrics(&lyrics_path, &text) {
                        Ok(()) => report.saved += 1,
                        Err(error) => fail(format!("Couldn't write: {error}")),
                    },
                    LyricsResult::PlainOnly => report.plain_only += 1,
                    LyricsResult::Instrumental => report.instrumental += 1,
                    LyricsResult::NotFound => report.not_found += 1,
                }
            }
            Err(error) => {
                fail(error);
                consecutive_errors += 1;
                if consecutive_errors >= MAX_CONSECUTIVE_ERRORS {
                    report.stopped = true;
                    break;
                }
            }
        }
    }

    report.remaining = scan_library(output, profile)?
        .albums
        .into_iter()
        .flat_map(|album| album.issues)
        .collect();
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::super::profile::ECHO_MINI;
    use super::*;
    use std::path::PathBuf;

    const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

    fn record(duration: f64, plain: Option<&str>, synced: Option<&str>) -> LrclibRecord {
        LrclibRecord {
            instrumental: false,
            duration: Some(duration),
            plain_lyrics: plain.map(str::to_owned),
            synced_lyrics: synced.map(str::to_owned),
        }
    }

    /// A source folder and an output holding `count` tagged tracks.
    fn library(count: usize) -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let (source, output) = (dir.path().join("in"), dir.path().join("out"));
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&output).unwrap();
        for i in 0..count {
            let track = output.join(format!("{i}.flac"));
            fs::copy(format!("{FIXTURES}/sample.flac"), track).unwrap();
        }
        (dir, source, output)
    }

    fn run(
        source: &Path,
        output: &Path,
        lookup: impl FnMut(&LyricsQuery) -> Result<LyricsResult, String>,
    ) -> LyricsReport {
        fetch_lyrics(source, output, &ECHO_MINI, lookup, |_| {}).unwrap()
    }

    #[test]
    fn retries_temporary_failures_then_succeeds() {
        let mut calls = 0;
        let mut waits = 0;
        let result = with_retries(
            || {
                calls += 1;
                if calls < 3 {
                    Err(ureq::Error::StatusCode(503))
                } else {
                    Ok("ok")
                }
            },
            |_| waits += 1,
        );
        assert_eq!(result.unwrap(), "ok");
        assert_eq!((calls, waits), (3, 2));
    }

    #[test]
    fn gives_up_when_failures_persist() {
        let mut calls = 0;
        let result: Result<(), _> = with_retries(
            || {
                calls += 1;
                Err(ureq::Error::StatusCode(429))
            },
            |_| {},
        );
        assert!(result.is_err());
        assert_eq!(calls, RETRY_DELAYS.len() + 1);
    }

    #[test]
    fn does_not_retry_a_not_found() {
        let mut calls = 0;
        let result: Result<(), _> = with_retries(
            || {
                calls += 1;
                Err(ureq::Error::StatusCode(404))
            },
            |_| panic!("should not wait"),
        );
        assert!(result.is_err());
        assert_eq!(calls, 1);
    }

    #[test]
    fn search_ignores_other_lengths_and_prefers_synced() {
        let records = vec![
            record(500.0, None, Some("[00:01.00]live version")),
            record(201.0, Some("plain"), None),
            record(199.0, None, Some("[00:01.00]right")),
        ];
        assert_eq!(
            best_match(records, 200),
            LyricsResult::Synced("[00:01.00]right".into())
        );
    }

    #[test]
    fn search_reports_plain_when_nothing_is_synced() {
        let records = vec![record(200.0, Some("plain"), Some("  "))];
        assert_eq!(best_match(records, 200), LyricsResult::PlainOnly);
    }

    #[test]
    fn search_with_no_close_length_finds_nothing() {
        let records = vec![record(300.0, None, Some("[00:01.00]x"))];
        assert_eq!(best_match(records, 200), LyricsResult::NotFound);
    }

    #[test]
    fn parses_lrclib_responses() {
        let json = r#"{"id":1,"trackName":"x","duration":264.0,"instrumental":false,
            "plainLyrics":"a","syncedLyrics":"[00:01.00]a"}"#;
        let parsed: LrclibRecord = serde_json::from_str(json).unwrap();
        assert_eq!(classify(parsed), LyricsResult::Synced("[00:01.00]a".into()));
    }

    #[test]
    fn saves_synced_lyrics_beside_tracks_and_sends_their_tags() {
        let (_dir, source, output) = library(1);
        let mut seen = Vec::new();
        let report = run(&source, &output, |query| {
            seen.push(query.clone());
            Ok(LyricsResult::Synced("[00:01.00]la".into()))
        });

        assert_eq!(report.saved, 1);
        assert_eq!(
            fs::read_to_string(output.join("0.lrc")).unwrap(),
            "[00:01.00]la"
        );
        assert_eq!(seen[0].artist, "Fixture");
        assert_eq!(seen[0].title, "Sample");
        assert_eq!(seen[0].album.as_deref(), Some("Tests"));
        assert!(!report.remaining.iter().any(|i| i.code == "lyrics_missing"));
    }

    #[test]
    fn writes_nothing_without_synced_lyrics() {
        let (_dir, source, output) = library(3);
        let mut results = vec![
            LyricsResult::PlainOnly,
            LyricsResult::Instrumental,
            LyricsResult::NotFound,
        ]
        .into_iter();
        let report = run(&source, &output, |_| Ok(results.next().unwrap()));

        assert_eq!(
            (
                report.saved,
                report.plain_only,
                report.instrumental,
                report.not_found
            ),
            (0, 1, 1, 1)
        );
        assert!(!output.join("0.lrc").exists());
    }

    #[test]
    fn keeps_existing_lyrics_without_a_lookup() {
        let (_dir, source, output) = library(1);
        fs::write(output.join("0.lrc"), "mine").unwrap();
        let report = run(&source, &output, |_| panic!("should not look up"));

        assert_eq!(report.already_had, 1);
        assert_eq!(fs::read_to_string(output.join("0.lrc")).unwrap(), "mine");
    }

    #[test]
    fn stops_after_repeated_errors() {
        let (_dir, source, output) = library(5);
        let mut calls = 0;
        let report = run(&source, &output, |_| {
            calls += 1;
            Err("offline".to_owned())
        });

        assert_eq!(calls, MAX_CONSECUTIVE_ERRORS);
        assert!(report.stopped);
        assert_eq!(report.failed.len(), MAX_CONSECUTIVE_ERRORS);
    }

    #[test]
    fn one_error_does_not_stop_the_run() {
        let (_dir, source, output) = library(5);
        let mut calls = 0;
        let report = run(&source, &output, |_| {
            calls += 1;
            if calls % 2 == 1 {
                Err("blip".to_owned())
            } else {
                Ok(LyricsResult::NotFound)
            }
        });

        assert_eq!(calls, 5);
        assert!(!report.stopped);
    }

    #[test]
    fn refuses_to_write_lyrics_into_the_source() {
        let (_dir, _source, output) = library(1);
        let result = fetch_lyrics(
            &output,
            &output,
            &ECHO_MINI,
            |_| panic!("should not look up"),
            |_| {},
        );
        assert!(result.is_err());
        assert!(!output.join("0.lrc").exists());
    }
}
