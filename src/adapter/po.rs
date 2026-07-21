//! Gettext PO adapter (AiNiee PoReader equivalent).
//!
//! Fills `msgstr` for simple entries. Plural entries (`msgid_plural`) and the
//! header entry (`msgid ""`) pass through untouched. Rendered `msgstr` is
//! always a single quoted line — valid PO regardless of length.

use super::{FormatAdapter, OutputFile, output_sibling};
use crate::model::{ItemType, TextUnit, Translation, needs_translation};
use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::path::Path;

pub struct PoAdapter;

struct Entry {
    lines: Vec<String>,
    msgid: String,
    plural: bool,
    /// [start, end) span of the msgstr keyword line + its continuations.
    msgstr_span: Option<(usize, usize)>,
}

impl FormatAdapter for PoAdapter {
    fn id(&self) -> &'static str {
        "po"
    }
    fn label(&self) -> &'static str {
        "Gettext PO"
    }
    fn extensions(&self) -> &'static [&'static str] {
        &["po", "pot"]
    }

    fn extract(&self, input: &Path, source_lang: &str) -> Result<Vec<TextUnit>> {
        let entries = parse_entries(input)?;
        let mut units = Vec::new();
        for (idx, e) in entries.iter().enumerate() {
            if e.plural || e.msgid.is_empty() || !needs_translation(&e.msgid, source_lang) {
                continue;
            }
            let ends_nl = e.msgid.ends_with('\n');
            let lines: Vec<String> = e
                .msgid
                .trim_end_matches('\n')
                .split('\n')
                .map(str::to_string)
                .collect();
            let location = format!("e{idx:05}");
            units.push(TextUnit {
                id: TextUnit::compute_id("po", &location, &lines),
                engine: "po".into(),
                domain: "gettext".into(),
                location,
                item_type: ItemType::LongText,
                role: String::new(),
                original_lines: lines,
                source_line_paths: vec![],
                context: format!("s{:04}", idx / 40),
                payload: if ends_nl { "nl".into() } else { String::new() },
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
        let mut entries = parse_entries(input)?;
        for u in units {
            let Some(tr) = translations.get(&u.id) else {
                continue;
            };
            let Ok(idx) = u.location.trim_start_matches('e').parse::<usize>() else {
                continue;
            };
            let Some(e) = entries.get_mut(idx) else {
                continue;
            };
            let Some((start, end)) = e.msgstr_span else {
                continue;
            };
            let mut text = tr.translation_lines.join("\n");
            if u.payload == "nl" {
                text.push('\n');
            }
            let rendered = format!("msgstr \"{}\"", po_escape(&text));
            e.lines.splice(start..end, [rendered]);
        }
        let body = entries
            .iter()
            .map(|e| e.lines.join("\n"))
            .collect::<Vec<_>>()
            .join("\n\n")
            + "\n";
        Ok(vec![OutputFile::text(
            output_sibling(input, target_lang, "po"),
            body,
        )])
    }
}

fn parse_entries(input: &Path) -> Result<Vec<Entry>> {
    let body = std::fs::read_to_string(input).with_context(|| format!("{}", input.display()))?;
    let mut entries = Vec::new();
    let mut cur: Vec<String> = Vec::new();
    for line in body.lines() {
        let line = line.trim_end_matches('\r');
        if line.trim().is_empty() {
            if !cur.is_empty() {
                entries.push(build_entry(std::mem::take(&mut cur)));
            }
        } else {
            cur.push(line.to_string());
        }
    }
    if !cur.is_empty() {
        entries.push(build_entry(cur));
    }
    Ok(entries)
}

fn build_entry(lines: Vec<String>) -> Entry {
    let mut msgid = String::new();
    let mut plural = false;
    let mut msgstr_span: Option<(usize, usize)> = None;
    #[derive(PartialEq)]
    enum Section {
        None,
        Msgid,
        Msgstr,
        Other,
    }
    let mut section = Section::None;
    for (i, line) in lines.iter().enumerate() {
        let t = line.trim();
        if t.starts_with('#') {
            section = Section::None;
            continue;
        }
        if let Some(rest) = t.strip_prefix("msgid_plural") {
            let _ = rest;
            plural = true;
            section = Section::Other;
        } else if let Some(rest) = t.strip_prefix("msgid") {
            msgid.push_str(&quoted_value(rest));
            section = Section::Msgid;
        } else if t.starts_with("msgstr") {
            // covers msgstr and msgstr[n]
            if msgstr_span.is_none() {
                msgstr_span = Some((i, i + 1));
            } else if let Some((_, end)) = msgstr_span.as_mut() {
                *end = i + 1; // msgstr[1]… extend span (plural — skipped anyway)
            }
            section = Section::Msgstr;
        } else if t.starts_with("msgctxt") {
            section = Section::Other;
        } else if t.starts_with('"') {
            match section {
                Section::Msgid => msgid.push_str(&quoted_value(t)),
                Section::Msgstr => {
                    if let Some((_, end)) = msgstr_span.as_mut() {
                        *end = i + 1;
                    }
                }
                _ => {}
            }
        }
    }
    Entry {
        lines,
        msgid: po_unescape(&msgid),
        plural,
        msgstr_span,
    }
}

/// `msgid "abc"` → `abc` (still escaped); bare `"abc"` continuations too.
fn quoted_value(rest: &str) -> String {
    let t = rest.trim();
    t.strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or("")
        .to_string()
}

fn po_unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn po_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn po_roundtrip() {
        let dir = super::super::test_dir("po");
        let input = dir.join("a.po");
        std::fs::write(
            &input,
            "msgid \"\"\nmsgstr \"\"\n\"Language: ja\\n\"\n\n#: src/a.rs:1\nmsgid \"こんにちは\\n世界\"\nmsgstr \"\"\n",
        )
        .unwrap();
        let units = PoAdapter.extract(&input, "ja").unwrap();
        assert_eq!(units.len(), 1, "header skipped");
        assert_eq!(units[0].original_lines, ["こんにちは", "世界"]);
        let mut tr = BTreeMap::new();
        tr.insert(
            units[0].id.clone(),
            Translation {
                unit_id: units[0].id.clone(),
                translation_lines: vec!["你好".into(), "世界".into()],
                source_hash: TextUnit::source_hash(&units[0].original_lines),
            },
        );
        let outs = PoAdapter.writeback(&input, "zh", &units, &tr).unwrap();
        let body = String::from_utf8(outs[0].bytes.clone()).unwrap();
        assert!(body.contains("msgstr \"你好\\n世界\""), "{body}");
        assert!(
            body.contains("\"Language: ja\\n\""),
            "header intact: {body}"
        );
    }
}
