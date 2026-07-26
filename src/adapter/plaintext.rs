//! Plain text (.txt) and Markdown (.md) adapters — AiNiee TxtReader/MdReader
//! equivalents.
//!
//! Line-granular: each source-language line becomes one unit (Japanese novels
//! use one line per paragraph). Markdown additionally skips fenced code blocks
//! and keeps leading syntax (`#`, `>`, list markers) out of the model's input.
//!
//! Input encoding is auto-detected (UTF-8 / Shift-JIS / GBK / UTF-16 BOM);
//! output is always UTF-8.

use super::{FormatAdapter, OutputFile, output_sibling};
use crate::model::{ItemType, TextUnit, Translation, needs_translation};
use crate::textio;
use anyhow::Result;
use std::collections::BTreeMap;
use std::path::Path;

pub struct TxtAdapter;
pub struct MdAdapter;

impl FormatAdapter for TxtAdapter {
    fn id(&self) -> &'static str {
        "txt"
    }
    fn label(&self) -> &'static str {
        "Plain text / novel"
    }
    fn extensions(&self) -> &'static [&'static str] {
        &["txt"]
    }
    fn extract(&self, input: &Path, source_lang: &str) -> Result<Vec<TextUnit>> {
        extract_lines(input, source_lang, self.id(), false)
    }
    fn writeback(
        &self,
        input: &Path,
        target_lang: &str,
        units: &[TextUnit],
        translations: &BTreeMap<String, Translation>,
    ) -> Result<Vec<OutputFile>> {
        writeback_lines(input, target_lang, "txt", units, translations)
    }
}

impl FormatAdapter for MdAdapter {
    fn id(&self) -> &'static str {
        "md"
    }
    fn label(&self) -> &'static str {
        "Markdown document"
    }
    fn extensions(&self) -> &'static [&'static str] {
        &["md", "markdown"]
    }
    fn extract(&self, input: &Path, source_lang: &str) -> Result<Vec<TextUnit>> {
        extract_lines(input, source_lang, self.id(), true)
    }
    fn writeback(
        &self,
        input: &Path,
        target_lang: &str,
        units: &[TextUnit],
        translations: &BTreeMap<String, Translation>,
    ) -> Result<Vec<OutputFile>> {
        writeback_lines(input, target_lang, "md", units, translations)
    }
}

/// Leading markdown syntax to keep out of the translation payload.
fn md_prefix(line: &str) -> usize {
    let re = regex::Regex::new(r"^\s{0,3}(?:#{1,6}\s+|>\s*|[-*+]\s+|\d{1,3}[.)]\s+)+").unwrap();
    re.find(line).map(|m| m.end()).unwrap_or(0)
}

fn read_utf8(input: &Path) -> Result<String> {
    textio::read_text(input)
}

fn extract_lines(
    input: &Path,
    source_lang: &str,
    engine: &str,
    markdown: bool,
) -> Result<Vec<TextUnit>> {
    let body = read_utf8(input)?;
    let mut units = Vec::new();
    let mut in_fence = false;
    for (i, raw_line) in body.lines().enumerate() {
        let line = raw_line.trim_end_matches('\r');
        if markdown {
            let t = line.trim_start();
            if t.starts_with("```") || t.starts_with("~~~") {
                in_fence = !in_fence;
                continue;
            }
            if in_fence {
                continue;
            }
        }
        let start = if markdown { md_prefix(line) } else { 0 };
        let text = &line[start..];
        if text.trim().is_empty() || !needs_translation(text, source_lang) {
            continue;
        }
        let location = format!("L{:06}", i + 1);
        let lines = vec![text.to_string()];
        units.push(TextUnit {
            id: TextUnit::compute_id(engine, &location, &lines),
            engine: engine.into(),
            domain: "text".into(),
            location,
            item_type: ItemType::ShortText,
            role: String::new(),
            original_lines: lines,
            source_line_paths: vec![],
            // group ~50 consecutive lines per batch context
            context: format!("s{:06}", i / 50),
            payload: start.to_string(), // md prefix byte offset
        });
    }
    Ok(units)
}

fn writeback_lines(
    input: &Path,
    target_lang: &str,
    ext: &str,
    units: &[TextUnit],
    translations: &BTreeMap<String, Translation>,
) -> Result<Vec<OutputFile>> {
    let body = read_utf8(input)?;
    let crlf = body.contains("\r\n");
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
        // ShortText units carry one line; join defensively if the model split.
        *slot = format!("{prefix}{}", tr.translation_lines.join(""));
    }
    let sep = if crlf { "\r\n" } else { "\n" };
    let mut out = lines.join(sep);
    if body.ends_with('\n') {
        out.push_str(sep);
    }
    Ok(vec![OutputFile::text(
        output_sibling(input, target_lang, ext),
        out,
    )])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Translation;

    fn fake_tr(units: &[TextUnit]) -> BTreeMap<String, Translation> {
        units
            .iter()
            .map(|u| {
                (
                    u.id.clone(),
                    Translation {
                        unit_id: u.id.clone(),
                        translation_lines: u
                            .original_lines
                            .iter()
                            .map(|l| format!("译:{l}"))
                            .collect(),
                        source_hash: TextUnit::source_hash(&u.original_lines),
                        passthrough: false,
                    },
                )
            })
            .collect()
    }

    #[test]
    fn txt_roundtrip() {
        let dir = super::super::test_dir("txt");
        let input = dir.join("novel.txt");
        std::fs::write(&input, "第一段です。\n\nSecond ascii line\n第二段です。\n").unwrap();
        let units = TxtAdapter.extract(&input, "ja").unwrap();
        assert_eq!(units.len(), 2);
        let outs = TxtAdapter
            .writeback(&input, "zh", &units, &fake_tr(&units))
            .unwrap();
        let body = String::from_utf8(outs[0].bytes.clone()).unwrap();
        assert_eq!(
            body,
            "译:第一段です。\n\nSecond ascii line\n译:第二段です。\n"
        );
        assert!(outs[0].path.ends_with("novel.zh.txt"));
    }

    #[test]
    fn md_skips_fences_keeps_prefix() {
        let dir = super::super::test_dir("md");
        let input = dir.join("doc.md");
        std::fs::write(
            &input,
            "# 見出しです\n```\nコード内は無視\n```\n- 箇条書きです\n",
        )
        .unwrap();
        let units = MdAdapter.extract(&input, "ja").unwrap();
        assert_eq!(units.len(), 2);
        assert_eq!(units[0].original_lines, ["見出しです"]);
        let outs = MdAdapter
            .writeback(&input, "zh", &units, &fake_tr(&units))
            .unwrap();
        let body = String::from_utf8(outs[0].bytes.clone()).unwrap();
        assert!(body.contains("# 译:見出しです"), "{body}");
        assert!(body.contains("コード内は無視"), "{body}");
        assert!(body.contains("- 译:箇条書きです"), "{body}");
    }
}
