use lofty::config::{ParseOptions, WriteOptions};
use lofty::file::AudioFile;
use lofty::flac::FlacFile;
use lofty::ogg::tag::VorbisComments;
use lofty::prelude::*;
use std::env;
use std::fs::OpenOptions;
use std::io::Seek;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn find_in(dirs: impl IntoIterator<Item = PathBuf>, name: &str) -> Option<PathBuf> {
    let file = format!("{name}{}", env::consts::EXE_SUFFIX);
    dirs.into_iter()
        .map(|dir| dir.join(&file))
        .find(|path| path.is_file())
}

/// Looks beside the app first, where bundled sidecars end up, then on the PATH.
fn find_tool(name: &str) -> Option<PathBuf> {
    let beside_app = env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf));
    let on_path = env::var_os("PATH")
        .map(|paths| env::split_paths(&paths).collect::<Vec<_>>())
        .unwrap_or_default();
    find_in(beside_app.into_iter().chain(on_path), name)
}

fn ffmpeg(args: &[&std::ffi::OsStr]) -> Result<Output, String> {
    let tool = find_tool("ffmpeg").ok_or("ffmpeg wasn't found")?;
    let output = Command::new(tool)
        .args(["-nostdin", "-v", "error"])
        .args(args)
        .output()
        .map_err(|e| e.to_string())?;
    if output.status.success() {
        Ok(output)
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
    }
}

/// A checksum of the decoded samples. Decoded to 32 bits, as ffmpeg's default
/// of 16 would hide differences in higher bit depths.
pub(super) fn audio_md5(path: &Path) -> Result<String, String> {
    let output = ffmpeg(&[
        "-i".as_ref(),
        path.as_os_str(),
        "-map".as_ref(),
        "0:a:0".as_ref(),
        "-c:a".as_ref(),
        "pcm_s32le".as_ref(),
        "-f".as_ref(),
        "md5".as_ref(),
        "-".as_ref(),
    ])?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn copy_tags(source: &Path, dest: &Path) -> Result<(), String> {
    let tagged = lofty::read_from_path(source).map_err(|e| e.to_string())?;
    let Some(tag) = tagged.primary_tag().or_else(|| tagged.first_tag()) else {
        return Ok(());
    };
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(dest)
        .map_err(|e| e.to_string())?;
    let options = ParseOptions::new().read_properties(false);
    let mut flac = FlacFile::read_from(&mut file, options).map_err(|e| e.to_string())?;
    flac.set_vorbis_comments(VorbisComments::from(tag.clone()));
    file.rewind().map_err(|e| e.to_string())?;
    flac.save_to(&mut file, WriteOptions::new())
        .map_err(|e| e.to_string())
}

/// Encodes `source` as FLAC at `dest`, replacing whatever is there, and carries
/// its tags over. Fails unless the FLAC decodes to exactly the source's samples,
/// which also stops formats FLAC can't hold, like floating point, losing quality.
pub(super) fn convert_to_flac(source: &Path, dest: &Path) -> Result<(), String> {
    ffmpeg(&[
        "-y".as_ref(),
        "-i".as_ref(),
        source.as_os_str(),
        "-map".as_ref(),
        "0:a:0".as_ref(),
        "-map_metadata".as_ref(),
        "-1".as_ref(),
        "-c:a".as_ref(),
        "flac".as_ref(),
        "-compression_level".as_ref(),
        "8".as_ref(),
        // The destination is a temporary name, so the format can't be inferred.
        "-f".as_ref(),
        "flac".as_ref(),
        dest.as_os_str(),
    ])?;
    if audio_md5(source)? != audio_md5(dest)? {
        return Err("Converted audio didn't match the source".to_owned());
    }
    copy_tags(source, dest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};

    const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

    fn sample() -> PathBuf {
        Path::new(FIXTURES).join("sample.aiff")
    }

    /// Re-encodes the fixture's audio as an AIFF with the given sample format.
    fn aiff_with_codec(codec: &str, dest: &Path) {
        ffmpeg(&[
            "-i".as_ref(),
            sample().as_os_str(),
            "-c:a".as_ref(),
            codec.as_ref(),
            "-f".as_ref(),
            "aiff".as_ref(),
            dest.as_os_str(),
        ])
        .unwrap();
    }

    #[test]
    fn finds_a_tool_only_where_it_exists() {
        let dir = tempfile::tempdir().unwrap();
        let file = format!("tool{}", env::consts::EXE_SUFFIX);
        fs::write(dir.path().join(&file), "").unwrap();
        let dirs = || [dir.path().join("empty"), dir.path().to_path_buf()];
        assert_eq!(find_in(dirs(), "tool"), Some(dir.path().join(&file)));
        assert_eq!(find_in(dirs(), "other"), None);
    }

    #[test]
    fn converted_audio_is_identical_and_tags_carry_over() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("out.part");

        convert_to_flac(&sample(), &dest).unwrap();

        assert_eq!(audio_md5(&sample()).unwrap(), audio_md5(&dest).unwrap());
        let options = ParseOptions::new().read_properties(false);
        let flac = FlacFile::read_from(&mut File::open(&dest).unwrap(), options).unwrap();
        let comments = flac.vorbis_comments().unwrap();
        assert_eq!(comments.title().as_deref(), Some("Sample"));
        assert_eq!(comments.track(), Some(1));
        assert_eq!(comments.track_total(), Some(2));
        assert_eq!(comments.get("ALBUMARTIST"), Some("Fixture"));
    }

    #[test]
    fn keeps_24_bit_audio_intact() {
        let dir = tempfile::tempdir().unwrap();
        let (source, dest) = (dir.path().join("in.aiff"), dir.path().join("out.flac"));
        aiff_with_codec("pcm_s24be", &source);

        convert_to_flac(&source, &dest).unwrap();

        let properties = lofty::read_from_path(&dest).unwrap();
        assert_eq!(properties.properties().bit_depth(), Some(24));
    }

    #[test]
    fn checksum_tells_different_audio_apart() {
        let dir = tempfile::tempdir().unwrap();
        let quieter = dir.path().join("quieter.flac");
        ffmpeg(&[
            "-i".as_ref(),
            sample().as_os_str(),
            "-af".as_ref(),
            "volume=0.5".as_ref(),
            quieter.as_os_str(),
        ])
        .unwrap();
        assert_ne!(audio_md5(&sample()).unwrap(), audio_md5(&quieter).unwrap());
    }

    #[test]
    fn refuses_audio_flac_cannot_hold() {
        let dir = tempfile::tempdir().unwrap();
        let (source, dest) = (dir.path().join("in.aiff"), dir.path().join("out.flac"));
        // Scaling leaves detail finer than the 24 bits FLAC can hold.
        ffmpeg(&[
            "-i".as_ref(),
            sample().as_os_str(),
            "-af".as_ref(),
            "volume=0.3337".as_ref(),
            "-c:a".as_ref(),
            "pcm_f32be".as_ref(),
            source.as_os_str(),
        ])
        .unwrap();
        assert!(convert_to_flac(&source, &dest).is_err());
    }
}
