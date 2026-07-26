//! Text file reading with encoding auto-detection.
//!
//! Fan-translation source material is frequently Shift-JIS (JP games/novels),
//! GBK (CN), or UTF-16 (KiriKiri scenarios). Every text-based adapter reads
//! through here: strict UTF-8 first, then BOM-driven UTF-16, then a chardetng
//! guess decoded via encoding_rs. Output files are always written as UTF-8.

use anyhow::{Context, Result};
use std::path::Path;

pub struct DecodedText {
    pub text: String,
    /// Canonical encoding name ("UTF-8", "Shift_JIS", "GBK", "UTF-16LE", …).
    pub encoding: &'static str,
    /// True when the decoder had to substitute replacement characters.
    pub lossy: bool,
}

pub fn read_text(path: &Path) -> Result<String> {
    Ok(read_text_detected(path)?.text)
}

pub fn read_text_detected(path: &Path) -> Result<DecodedText> {
    let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let decoded = decode_bytes(&bytes);
    if decoded.lossy {
        eprintln!(
            "warning: {} decoded as {} with replacement characters — check the source encoding",
            path.display(),
            decoded.encoding
        );
    }
    Ok(decoded)
}

pub fn decode_bytes(bytes: &[u8]) -> DecodedText {
    // Strict UTF-8 fast path (covers ASCII too).
    if let Ok(s) = std::str::from_utf8(bytes) {
        return DecodedText {
            text: strip_bom(s).to_string(),
            encoding: "UTF-8",
            lossy: false,
        };
    }
    // UTF-16 BOMs — chardetng does not detect UTF-16.
    if bytes.len() >= 2 {
        let enc = match (bytes[0], bytes[1]) {
            (0xFF, 0xFE) => Some(encoding_rs::UTF_16LE),
            (0xFE, 0xFF) => Some(encoding_rs::UTF_16BE),
            _ => None,
        };
        if let Some(enc) = enc {
            let (text, _, lossy) = enc.decode(bytes);
            return DecodedText {
                text: strip_bom(&text).to_string(),
                encoding: enc.name(),
                lossy,
            };
        }
    }
    let mut detector = chardetng::EncodingDetector::new();
    detector.feed(bytes, true);
    let enc = detector.guess(None, true);
    let (text, used, lossy) = enc.decode(bytes);
    DecodedText {
        text: strip_bom(&text).to_string(),
        encoding: used.name(),
        lossy,
    }
}

fn strip_bom(s: &str) -> &str {
    s.strip_prefix('\u{FEFF}').unwrap_or(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_passthrough() {
        let d = decode_bytes("こんにちは".as_bytes());
        assert_eq!(d.encoding, "UTF-8");
        assert_eq!(d.text, "こんにちは");
    }

    #[test]
    fn shift_jis_detected() {
        // "こんにちは" in Shift-JIS
        let sjis: &[u8] = &[0x82, 0xB1, 0x82, 0xF1, 0x82, 0xC9, 0x82, 0xBF, 0x82, 0xCD];
        let d = decode_bytes(sjis);
        assert_eq!(d.text, "こんにちは", "encoding guessed: {}", d.encoding);
        assert!(!d.lossy);
    }

    #[test]
    fn utf16le_bom() {
        let mut bytes = vec![0xFF, 0xFE];
        for u in "テスト".encode_utf16() {
            bytes.extend_from_slice(&u.to_le_bytes());
        }
        let d = decode_bytes(&bytes);
        assert_eq!(d.encoding, "UTF-16LE");
        assert_eq!(d.text, "テスト");
    }
}
