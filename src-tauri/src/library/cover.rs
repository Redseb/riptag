use image::{ImageFormat, ImageReader};
use std::io::Cursor;

#[derive(Debug, Clone, PartialEq)]
pub struct CoverInfo {
    pub is_jpeg: bool,
    pub progressive: bool,
    pub width: u32,
    pub height: u32,
}

/// Reads the image header only. Returns `None` if it isn't a readable image.
pub fn inspect_cover(bytes: &[u8]) -> Option<CoverInfo> {
    let reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let is_jpeg = reader.format() == Some(ImageFormat::Jpeg);
    let (width, height) = reader.into_dimensions().ok()?;
    Some(CoverInfo {
        is_jpeg,
        progressive: is_jpeg && is_progressive_jpeg(bytes),
        width,
        height,
    })
}

/// Walks the JPEG markers to the start-of-frame, which says how it's encoded.
fn is_progressive_jpeg(bytes: &[u8]) -> bool {
    let mut pos = 2;
    while pos + 4 <= bytes.len() {
        if bytes[pos] != 0xFF {
            return false;
        }
        let marker = bytes[pos + 1];
        match marker {
            // Padding before a marker.
            0xFF => pos += 1,
            0xC2 => return true,
            0xC0 | 0xC1 | 0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF | 0xDA => return false,
            _ => {
                let len = u16::from_be_bytes([bytes[pos + 2], bytes[pos + 3]]) as usize;
                pos += 2 + len;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_progressive_marker_after_other_segments() {
        let app0 = [0xFF, 0xE0, 0x00, 0x04, 0x00, 0x00];
        let mut jpeg = vec![0xFF, 0xD8];
        jpeg.extend(app0);
        let mut baseline = jpeg.clone();
        jpeg.extend([0xFF, 0xC2, 0x00, 0x02]);
        baseline.extend([0xFF, 0xC0, 0x00, 0x02]);
        assert!(is_progressive_jpeg(&jpeg));
        assert!(!is_progressive_jpeg(&baseline));
    }

    #[test]
    fn rejects_bytes_that_are_not_an_image() {
        assert_eq!(inspect_cover(b"not an image"), None);
    }
}
