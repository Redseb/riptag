mod cover;
mod diagnose;
mod lyrics;
mod profile;
mod repair;
mod scan;
mod track;

pub use lyrics::{LyricsReport, fetch_lyrics, lrclib_lookup};
pub use profile::ECHO_MINI;
pub use repair::{RepairProgress, RepairReport, repair_library};
pub use scan::{ScanResult, scan_library};
