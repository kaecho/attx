//! ASS / SSA subtitle adapter (Advanced SubStation Alpha).
//!
//! Only the Text field of `Dialogue:` lines in `[Events]` is translated; all
//! styling, timing, and `Comment:` lines pass through verbatim. The field
//! order comes from the section's `Format:` line (Text is defined to be last,
//! so embedded commas are safe). Override tags `{\pos(…)}` and hard breaks
//! `\N` survive via control-code masking + the system prompt.

use super::{FormatAdapter, OutputFile, output_sibling};
use crate::model::{ItemType, TextUnit, Translation, needs_translation};
use crate::textio;
use anyhow::Result;
use std::collections::BTreeMap;
use std::path::Path;

pub struct AssAdapter;

const DEFAULT_FIELDS: usize = 10; // Layer,Start,End,Style,Name,MarginL,MarginR,MarginV,Effect,Text

struct EventLine {
    /// 1-based line number in the file.
    lineno: usize,
    role: String,
    text: String,
    /// Byte offset where the Text field starts on the line.
    text_start: usize,
}

fn parse_events(body: &str) -> Vec<EventLine> {
    let mut out = Vec::new();
    let mut in_events = false;
    let mut n_fields = DEFAULT_FIELDS;
    let mut name_idx: Option<usize> = None;
    for (i, raw) in body.lines().enumerate() {
        let line = raw.trim_end_matches('\r');
        let t = line.trim();
        if t.starts_with('[') {
            in_events = t.eq_ignore_ascii_case("[events]");
            continue;
        }
        if !in_events {
            continue;
        }
        if let Some(rest) = t.strip_prefix("Format:") {
            let fields: Vec<&str> = rest.split(',').map(str::trim).collect();
            n_fields = fields.len().max(2);
            name_idx = fields.iter().position(|f| f.eq_ignore_ascii_case("Name"));
            continue;
        }
        let Some(rest) = line.strip_prefix("Dialogue:") else {
            continue;
        };
        // Text is the last field: skip n_fields-1 commas.
        let mut commas = 0usize;
        let mut split_at = None;
        for (off, c) in rest.char_indices() {
            if c == ',' {
                commas += 1;
                if commas == n_fields - 1 {
                    split_at = Some(off + 1);
                    break;
                }
            }
        }
        let Some(split_at) = split_at else { continue };
        let fields_part = &rest[..split_at - 1];
        let text = &rest[split_at..];
        let role = name_idx
            .and_then(|idx| fields_part.split(',').nth(idx))
            .unwrap_or("")
            .trim()
            .to_string();
        out.push(EventLine {
            lineno: i + 1,
            role,
            text: text.to_string(),
            text_start: "Dialogue:".len() + split_at,
        });
    }
    out
}

/// True when the text is only override tags / whitespace (nothing to translate).
fn strip_override_tags(text: &str) -> String {
    let mut out = String::new();
    let mut depth = 0usize;
    for c in text.chars() {
        match c {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

impl FormatAdapter for AssAdapter {
    fn id(&self) -> &'static str {
        "ass"
    }
    fn label(&self) -> &'static str {
        "ASS/SSA subtitles"
    }
    fn extensions(&self) -> &'static [&'static str] {
        &["ass", "ssa"]
    }

    fn extract(&self, input: &Path, source_lang: &str) -> Result<Vec<TextUnit>> {
        let body = textio::read_text(input)?;
        let mut units = Vec::new();
        for ev in parse_events(&body) {
            let visible = strip_override_tags(&ev.text);
            if visible.trim().is_empty() || !needs_translation(&visible, source_lang) {
                continue;
            }
            let location = format!("L{:06}", ev.lineno);
            let lines = vec![ev.text.clone()];
            units.push(TextUnit {
                id: TextUnit::compute_id("ass", &location, &lines),
                engine: "ass".into(),
                domain: "subtitle".into(),
                location,
                item_type: ItemType::ShortText,
                role: ev.role,
                original_lines: lines,
                source_line_paths: vec![],
                context: format!("s{:04}", ev.lineno / 40),
                payload: String::new(),
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
        let body = textio::read_text(input)?;
        let events: BTreeMap<usize, usize> = parse_events(&body)
            .into_iter()
            .map(|e| (e.lineno, e.text_start))
            .collect();
        let mut lines: Vec<String> = body
            .lines()
            .map(|l| l.trim_end_matches('\r').to_string())
            .collect();
        let ext = input
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("ass")
            .to_ascii_lowercase();
        for u in units {
            let Some(tr) = translations.get(&u.id) else {
                continue;
            };
            let Ok(lineno) = u.location.trim_start_matches('L').parse::<usize>() else {
                continue;
            };
            let (Some(text_start), Some(slot)) = (events.get(&lineno), lines.get_mut(lineno - 1))
            else {
                continue;
            };
            // Hard line breaks in ASS are literal \N.
            let joined = tr.translation_lines.join("\\N");
            if *text_start <= slot.len() {
                *slot = format!("{}{}", &slot[..*text_start], joined);
            }
        }
        let mut out = lines.join("\n");
        if body.ends_with('\n') {
            out.push('\n');
        }
        Ok(vec![OutputFile::text(
            output_sibling(input, target_lang, &ext),
            out,
        )])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "[Script Info]\nTitle: テスト\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nComment: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,無視されるコメント\nDialogue: 0,0:00:01.00,0:00:03.00,Default,ヒロイン,0,0,0,,{\\pos(320,240)}こんにちは、世界\nDialogue: 0,0:00:04.00,0:00:05.00,Default,,0,0,0,,{\\fad(200,200)}\n";

    #[test]
    fn ass_roundtrip() {
        let dir = super::super::test_dir("ass");
        let input = dir.join("a.ass");
        std::fs::write(&input, SAMPLE).unwrap();
        let units = AssAdapter.extract(&input, "ja").unwrap();
        assert_eq!(units.len(), 1, "comment + tag-only skipped: {units:#?}");
        assert_eq!(units[0].role, "ヒロイン");
        assert!(units[0].original_lines[0].contains("{\\pos(320,240)}"));

        let mut tr = BTreeMap::new();
        tr.insert(
            units[0].id.clone(),
            Translation {
                unit_id: units[0].id.clone(),
                translation_lines: vec!["{\\pos(320,240)}你好，世界".into()],
                source_hash: TextUnit::source_hash(&units[0].original_lines),
                passthrough: false,
            },
        );
        let outs = AssAdapter.writeback(&input, "zh", &units, &tr).unwrap();
        let body = String::from_utf8(outs[0].bytes.clone()).unwrap();
        assert!(
            body.contains("Default,ヒロイン,0,0,0,,{\\pos(320,240)}你好，世界"),
            "{body}"
        );
        assert!(body.contains("無視されるコメント"), "{body}");
        assert!(body.contains("Title: テスト"), "{body}");
        assert!(outs[0].path.to_string_lossy().ends_with("a.zh.ass"));
    }

    #[test]
    fn ssa_default_format_when_missing() {
        let dir = super::super::test_dir("ssa");
        let input = dir.join("a.ssa");
        std::fs::write(
            &input,
            "[Events]\nDialogue: 0,0:00:01.00,0:00:03.00,Default,名前,0,0,0,,セリフ,続き\n",
        )
        .unwrap();
        let units = AssAdapter.extract(&input, "ja").unwrap();
        assert_eq!(units.len(), 1);
        assert_eq!(
            units[0].original_lines,
            ["セリフ,続き"],
            "commas in text kept"
        );
    }
}
