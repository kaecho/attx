//! Subtitle & lyric adapters: SRT, WebVTT, LRC (AiNiee SrtReader / VttReader /
//! LrcReader equivalents). Timing lines are preserved verbatim; only cue text
//! is translated.

use super::{FormatAdapter, OutputFile, output_sibling};
use crate::model::{ItemType, TextUnit, Translation, needs_translation};
use crate::textio;
use anyhow::Result;
use std::collections::BTreeMap;
use std::path::Path;

pub struct SrtAdapter;
pub struct VttAdapter;
pub struct LrcAdapter;

/// A cue-structured subtitle file split into blank-line-separated blocks.
/// Blocks without a `-->` timing line (WEBVTT header, NOTE, STYLE…) pass
/// through untouched.
struct Cues {
    blocks: Vec<Vec<String>>,
}

fn read_utf8(input: &Path) -> Result<String> {
    textio::read_text(input)
}

fn parse_blocks(body: &str) -> Cues {
    let mut blocks = Vec::new();
    let mut cur: Vec<String> = Vec::new();
    for line in body.lines() {
        let line = line.trim_end_matches('\r');
        if line.trim().is_empty() {
            if !cur.is_empty() {
                blocks.push(std::mem::take(&mut cur));
            }
        } else {
            cur.push(line.to_string());
        }
    }
    if !cur.is_empty() {
        blocks.push(cur);
    }
    Cues { blocks }
}

fn timing_index(block: &[String]) -> Option<usize> {
    block.iter().position(|l| l.contains("-->"))
}

fn extract_cues(input: &Path, source_lang: &str, engine: &str) -> Result<Vec<TextUnit>> {
    let cues = parse_blocks(&read_utf8(input)?);
    let mut units = Vec::new();
    for (bi, block) in cues.blocks.iter().enumerate() {
        let Some(ti) = timing_index(block) else {
            continue;
        };
        let text: Vec<String> = block[ti + 1..].to_vec();
        if text.is_empty() || !text.iter().any(|l| needs_translation(l, source_lang)) {
            continue;
        }
        let location = format!("c{bi:05}");
        units.push(TextUnit {
            id: TextUnit::compute_id(engine, &location, &text),
            engine: engine.into(),
            domain: "subtitle".into(),
            location,
            item_type: ItemType::LongText,
            role: String::new(),
            original_lines: text,
            source_line_paths: vec![],
            context: format!("s{:04}", bi / 40),
            payload: String::new(),
        });
    }
    Ok(units)
}

fn writeback_cues(
    input: &Path,
    target_lang: &str,
    ext: &str,
    units: &[TextUnit],
    translations: &BTreeMap<String, Translation>,
) -> Result<Vec<OutputFile>> {
    let mut cues = parse_blocks(&read_utf8(input)?);
    for u in units {
        let Some(tr) = translations.get(&u.id) else {
            continue;
        };
        let Ok(bi) = u.location.trim_start_matches('c').parse::<usize>() else {
            continue;
        };
        let Some(block) = cues.blocks.get_mut(bi) else {
            continue;
        };
        let Some(ti) = timing_index(block) else {
            continue;
        };
        block.truncate(ti + 1);
        block.extend(tr.translation_lines.iter().cloned());
    }
    let body = cues
        .blocks
        .iter()
        .map(|b| b.join("\n"))
        .collect::<Vec<_>>()
        .join("\n\n")
        + "\n";
    Ok(vec![OutputFile::text(
        output_sibling(input, target_lang, ext),
        body,
    )])
}

impl FormatAdapter for SrtAdapter {
    fn id(&self) -> &'static str {
        "srt"
    }
    fn label(&self) -> &'static str {
        "SubRip subtitles"
    }
    fn extensions(&self) -> &'static [&'static str] {
        &["srt"]
    }
    fn extract(&self, input: &Path, source_lang: &str) -> Result<Vec<TextUnit>> {
        extract_cues(input, source_lang, self.id())
    }
    fn writeback(
        &self,
        input: &Path,
        target_lang: &str,
        units: &[TextUnit],
        translations: &BTreeMap<String, Translation>,
    ) -> Result<Vec<OutputFile>> {
        writeback_cues(input, target_lang, "srt", units, translations)
    }
}

impl FormatAdapter for VttAdapter {
    fn id(&self) -> &'static str {
        "vtt"
    }
    fn label(&self) -> &'static str {
        "WebVTT subtitles"
    }
    fn extensions(&self) -> &'static [&'static str] {
        &["vtt"]
    }
    fn extract(&self, input: &Path, source_lang: &str) -> Result<Vec<TextUnit>> {
        extract_cues(input, source_lang, self.id())
    }
    fn writeback(
        &self,
        input: &Path,
        target_lang: &str,
        units: &[TextUnit],
        translations: &BTreeMap<String, Translation>,
    ) -> Result<Vec<OutputFile>> {
        writeback_cues(input, target_lang, "vtt", units, translations)
    }
}

impl FormatAdapter for LrcAdapter {
    fn id(&self) -> &'static str {
        "lrc"
    }
    fn label(&self) -> &'static str {
        "LRC lyrics"
    }
    fn extensions(&self) -> &'static [&'static str] {
        &["lrc"]
    }

    fn extract(&self, input: &Path, source_lang: &str) -> Result<Vec<TextUnit>> {
        let body = read_utf8(input)?;
        let mut units = Vec::new();
        for (i, line) in body.lines().enumerate() {
            let line = line.trim_end_matches('\r');
            let Some((prefix, text)) = split_lrc(line) else {
                continue;
            };
            if text.trim().is_empty() || !needs_translation(text, source_lang) {
                continue;
            }
            let location = format!("L{:06}", i + 1);
            let lines = vec![text.to_string()];
            units.push(TextUnit {
                id: TextUnit::compute_id("lrc", &location, &lines),
                engine: "lrc".into(),
                domain: "lyrics".into(),
                location,
                item_type: ItemType::ShortText,
                role: String::new(),
                original_lines: lines,
                source_line_paths: vec![],
                context: "lrc".into(),
                payload: prefix.len().to_string(),
            });
        }
        Ok(units)
    }

    fn writeback(
        &self,
        input: &Path,
        target_lang: &str,
        units: &[TextUnit],
        translations: &BTreeMap<String, Translation>,
    ) -> Result<Vec<OutputFile>> {
        let body = read_utf8(input)?;
        let mut lines: Vec<String> = body
            .lines()
            .map(|l| l.trim_end_matches('\r').to_string())
            .collect();
        for u in units {
            let Some(tr) = translations.get(&u.id) else {
                continue;
            };
            let Ok(lineno) = u.location.trim_start_matches('L').parse::<usize>() else {
                continue;
            };
            let Some(slot) = lines.get_mut(lineno - 1) else {
                continue;
            };
            let prefix_len: usize = u.payload.parse().unwrap_or(0);
            let prefix = slot.get(..prefix_len).unwrap_or("").to_string();
            *slot = format!("{prefix}{}", tr.translation_lines.join(" "));
        }
        let mut out = lines.join("\n");
        out.push('\n');
        Ok(vec![OutputFile::text(
            output_sibling(input, target_lang, "lrc"),
            out,
        )])
    }
}

/// `[01:23.45][01:25.00]歌詞` → ("[01:23.45][01:25.00]", "歌詞").
/// Metadata tags like `[ti:…]` have no trailing text and are skipped.
fn split_lrc(line: &str) -> Option<(&str, &str)> {
    if !line.starts_with('[') {
        return None;
    }
    let mut end = 0usize;
    let bytes = line.as_bytes();
    while end < bytes.len() && bytes[end] == b'[' {
        let close = line[end..].find(']')? + end;
        // timestamps contain a digit + ':' — reject [ti:artist] style meta tags
        let tag = &line[end + 1..close];
        if !tag.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            return None;
        }
        end = close + 1;
    }
    Some((&line[..end], &line[end..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_tr(units: &[TextUnit]) -> BTreeMap<String, Translation> {
        units
            .iter()
            .map(|u| {
                (
                    u.id.clone(),
                    Translation {
                        unit_id: u.id.clone(),
                        translation_lines: vec!["中文字幕".to_string()],
                        source_hash: TextUnit::source_hash(&u.original_lines),
                        passthrough: false,
                    },
                )
            })
            .collect()
    }

    #[test]
    fn srt_roundtrip() {
        let dir = super::super::test_dir("srt");
        let input = dir.join("a.srt");
        std::fs::write(
            &input,
            "1\n00:00:01,000 --> 00:00:03,000\nこんにちは\n世界\n\n2\n00:00:04,000 --> 00:00:05,000\n[music]\n",
        )
        .unwrap();
        let units = SrtAdapter.extract(&input, "ja").unwrap();
        assert_eq!(units.len(), 1);
        assert_eq!(units[0].original_lines, ["こんにちは", "世界"]);
        let outs = SrtAdapter
            .writeback(&input, "zh", &units, &fake_tr(&units))
            .unwrap();
        let body = String::from_utf8(outs[0].bytes.clone()).unwrap();
        assert_eq!(
            body,
            "1\n00:00:01,000 --> 00:00:03,000\n中文字幕\n\n2\n00:00:04,000 --> 00:00:05,000\n[music]\n"
        );
    }

    #[test]
    fn vtt_preserves_header() {
        let dir = super::super::test_dir("vtt");
        let input = dir.join("a.vtt");
        std::fs::write(
            &input,
            "WEBVTT\n\nNOTE comment\n\n00:01.000 --> 00:02.000 position:50%\nテスト\n",
        )
        .unwrap();
        let units = VttAdapter.extract(&input, "ja").unwrap();
        assert_eq!(units.len(), 1);
        let outs = VttAdapter
            .writeback(&input, "zh", &units, &fake_tr(&units))
            .unwrap();
        let body = String::from_utf8(outs[0].bytes.clone()).unwrap();
        assert!(body.starts_with("WEBVTT\n\nNOTE comment\n\n"), "{body}");
        assert!(body.contains("position:50%\n中文字幕"), "{body}");
    }

    #[test]
    fn lrc_keeps_timestamps_and_meta() {
        let dir = super::super::test_dir("lrc");
        let input = dir.join("a.lrc");
        std::fs::write(&input, "[ti:曲名メタ]\n[00:12.34]歌詞です\n").unwrap();
        let units = LrcAdapter.extract(&input, "ja").unwrap();
        assert_eq!(units.len(), 1, "meta tag must be skipped");
        let outs = LrcAdapter
            .writeback(&input, "zh", &units, &fake_tr(&units))
            .unwrap();
        let body = String::from_utf8(outs[0].bytes.clone()).unwrap();
        assert_eq!(body, "[ti:曲名メタ]\n[00:12.34]中文字幕\n");
    }
}
