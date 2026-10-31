/// What a device can handle. The scan and repair code only reads this, so a
/// new device means a new profile, not new logic.
pub struct DeviceProfile {
    pub name: &'static str,
    /// How the device names its storage when mounted.
    pub volume_names: &'static [&'static str],
    /// File systems it can use, lowercase, as each OS names them.
    pub volume_file_systems: &'static [&'static str],
    pub playable_extensions: &'static [&'static str],
    /// Lossless formats the device can't play, which repair converts to FLAC.
    /// Lossy formats don't belong here, as FLAC would only hide their losses.
    pub convert_to_flac: &'static [&'static str],
    /// Vorbis comment fields (FLAC, OGG) the device reads, uppercase.
    pub vorbis_fields: &'static [&'static str],
    /// ID3v2 frames (MP3) the device reads.
    pub id3_frames: &'static [&'static str],
    /// Largest cover side in pixels.
    pub cover_size: u32,
    pub max_path_chars: usize,
}

pub const ECHO_MINI: DeviceProfile = DeviceProfile {
    name: "Echo Mini",
    volume_names: &["ECHO MINI"],
    volume_file_systems: &["msdos", "fat32", "vfat", "exfat"],
    playable_extensions: &[
        "mp3", "ogg", "m4a", "wma", "flac", "ape", "wav", "dsf", "dff",
    ],
    convert_to_flac: &["aif", "aiff"],
    vorbis_fields: &[
        "TITLE",
        "ARTIST",
        "ALBUMARTIST",
        "ALBUM",
        "TRACKNUMBER",
        "TRACKTOTAL",
        "DISCNUMBER",
        "DISCTOTAL",
        "DATE",
        "GENRE",
    ],
    id3_frames: &[
        "TIT2", "TPE1", "TPE2", "TALB", "TRCK", "TPOS", "TDRC", "TYER", "TCON", "APIC",
    ],
    // 452 px is confirmed on the device. 600 is still an assumption.
    cover_size: 600,
    max_path_chars: 200,
};
