//! CSV / TSV adapter.
//!
//! Every source-language cell becomes a unit (`needs_translation` filters out
//! ids, numbers, and already-target-language cells). Records are parsed
//! RFC4180-style (quoted fields, doubled quotes, embedded newlines); on
//! writeback only records containing translated cells are re-rendered — all
//! other bytes pass through verbatim.

use super::{FormatAdapter, OutputFile, output_sibling};
use crate::model::{ItemType, TextUnit, Translation, needs_translation};
use crate::textio;
use anyhow::Result;
use std::collections::BTreeMap;
use std::path::Path;

pub struct CsvAdapter;

struct Record {
    fields: Vec<String>,
    /// Byte span of the record in the source (excluding the record separator).
    span: (usize, usize),
}

fn delimiter_for(input: &Path) -> char {
    match input
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
    {
        Some(ref e) if e == "tsv" => '\t',
        _ => ',',
    }
}

/// RFC4180-ish parse with byte spans. Handles quoted fields ("" escapes) and
/// embedded newlines inside quotes; both \n and \r\n record separators.
fn parse_records(body: &str, delim: char) -> Vec<Record> {
    let bytes = body.as_bytes();
    let mut records = Vec::new();
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut rec_start = 0usize;
    let mut in_quotes = false;
    let mut i = 0usize;
    let d = delim as u8;
    while i < bytes.len() {
        let c = bytes[i];
        if in_quotes {
            if c == b'"' {
                if bytes.get(i + 1) == Some(&b'"') {
                    field.push('"');
                    i += 2;
                    continue;
                }
                in_quotes = false;
                i += 1;
                continue;
            }
            // multi-byte chars are copied via str slice below
            let ch_len = utf8_len(c);
            field.push_str(&body[i..i + ch_len]);
            i += ch_len;
            continue;
        }
        match c {
            b'"' if field.is_empty() => {
                in_quotes = true;
                i += 1;
            }
            _ if c == d => {
                fields.push(std::mem::take(&mut field));
                i += 1;
            }
            b'\r' if bytes.get(i + 1) == Some(&b'\n') => {
                fields.push(std::mem::take(&mut field));
                records.push(Record {
                    fields: std::mem::take(&mut fields),
                    span: (rec_start, i),
                });
                i += 2;
                rec_start = i;
            }
            b'\n' => {
                fields.push(std::mem::take(&mut field));
                records.push(Record {
                    fields: std::mem::take(&mut fields),
                    span: (rec_start, i),
                });
                i += 1;
                rec_start = i;
            }
            _ => {
                let ch_len = utf8_len(c);
                field.push_str(&body[i..i + ch_len]);
                i += ch_len;
            }
        }
    }
    if !field.is_empty() || !fields.is_empty() {
        fields.push(field);
        records.push(Record {
            fields,
            span: (rec_start, bytes.len()),
        });
    }
    records
}

fn utf8_len(first_byte: u8) -> usize {
    match first_byte {
        b if b < 0x80 => 1,
        b if b >= 0xF0 => 4,
        b if b >= 0xE0 => 3,
        _ => 2,
    }
}

fn render_field(s: &str, delim: char) -> String {
    if s.contains(delim) || s.contains('"') || s.contains('\n') || s.contains('\r') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

impl FormatAdapter for CsvAdapter {
    fn id(&self) -> &'static str {
        "csv"
    }
    fn label(&self) -> &'static str {
        "CSV / TSV table"
    }
    fn extensions(&self) -> &'static [&'static str] {
        &["csv", "tsv"]
    }

    fn extract(&self, input: &Path, source_lang: &str) -> Result<Vec<TextUnit>> {
        let body = textio::read_text(input)?;
        let delim = delimiter_for(input);
        let mut units = Vec::new();
        for (ri, rec) in parse_records(&body, delim).iter().enumerate() {
            for (fi, cell) in rec.fields.iter().enumerate() {
                if cell.trim().is_empty() || !needs_translation(cell, source_lang) {
                    continue;
                }
                let location = format!("r{ri:06}/{fi}");
                let lines: Vec<String> = cell.split('\n').map(str::to_string).collect();
                let item_type = if lines.len() > 1 {
                    ItemType::LongText
                } else {
                    ItemType::ShortText
                };
                units.push(TextUnit {
                    id: TextUnit::compute_id("csv", &location, &lines),
                    engine: "csv".into(),
                    domain: "table".into(),
                    location,
                    item_type,
                    role: String::new(),
                    original_lines: lines,
                    source_line_paths: vec![],
                    context: format!("s{:04}", ri / 40),
                    payload: String::new(),
                });
            }
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
        let delim = delimiter_for(input);
        let mut records = parse_records(&body, delim);
        // record index -> set of (field index, translation)
        let mut changed: BTreeMap<usize, Vec<(usize, String)>> = BTreeMap::new();
        for u in units {
            let Some(tr) = translations.get(&u.id) else {
                continue;
            };
            let Some((r, f)) = u.location.split_once('/') else {
                continue;
            };
            let (Ok(ri), Ok(fi)) = (
                r.trim_start_matches('r').parse::<usize>(),
                f.parse::<usize>(),
            ) else {
                continue;
            };
            changed
                .entry(ri)
                .or_default()
                .push((fi, tr.translation_lines.join("\n")));
        }
        let mut out = String::with_capacity(body.len());
        let mut cursor = 0usize;
        for (ri, rec) in records.iter_mut().enumerate() {
            let Some(cells) = changed.get(&ri) else {
                continue;
            };
            for (fi, text) in cells {
                if let Some(slot) = rec.fields.get_mut(*fi) {
                    *slot = text.clone();
                }
            }
            out.push_str(&body[cursor..rec.span.0]);
            let rendered: Vec<String> = rec.fields.iter().map(|f| render_field(f, delim)).collect();
            out.push_str(&rendered.join(&delim.to_string()));
            cursor = rec.span.1;
        }
        out.push_str(&body[cursor..]);
        let ext = input
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("csv")
            .to_ascii_lowercase();
        Ok(vec![OutputFile::text(
            output_sibling(input, target_lang, &ext),
            out,
        )])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tr_for(units: &[TextUnit]) -> BTreeMap<String, Translation> {
        units
            .iter()
            .map(|u| {
                (
                    u.id.clone(),
                    Translation {
                        unit_id: u.id.clone(),
                        translation_lines: vec![format!("译:{}", u.original_lines.join("|"))],
                        source_hash: TextUnit::source_hash(&u.original_lines),
                        passthrough: false,
                    },
                )
            })
            .collect()
    }

    #[test]
    fn csv_roundtrip_quotes_kept() {
        let dir = super::super::test_dir("csv");
        let input = dir.join("t.csv");
        std::fs::write(
            &input,
            "id,name,memo\n1,勇者,\"強い, とても\"\n2,Slime,weak\n",
        )
        .unwrap();
        let units = CsvAdapter.extract(&input, "ja").unwrap();
        assert_eq!(units.len(), 2, "{units:#?}");
        let outs = CsvAdapter
            .writeback(&input, "zh", &units, &tr_for(&units))
            .unwrap();
        let body = String::from_utf8(outs[0].bytes.clone()).unwrap();
        assert_eq!(
            body,
            "id,name,memo\n1,译:勇者,\"译:強い, とても\"\n2,Slime,weak\n"
        );
    }

    #[test]
    fn tsv_delimiter() {
        let dir = super::super::test_dir("tsv");
        let input = dir.join("t.tsv");
        std::fs::write(&input, "台詞\tめも\n").unwrap();
        let units = CsvAdapter.extract(&input, "ja").unwrap();
        assert_eq!(units.len(), 2);
        assert_eq!(units[0].location, "r000000/0");
    }

    #[test]
    fn quoted_newline_cell() {
        let dir = super::super::test_dir("csvnl");
        let input = dir.join("n.csv");
        std::fs::write(&input, "a,\"一行目\n二行目\"\n").unwrap();
        let units = CsvAdapter.extract(&input, "ja").unwrap();
        assert_eq!(units.len(), 1);
        assert_eq!(units[0].original_lines, ["一行目", "二行目"]);
        let outs = CsvAdapter
            .writeback(&input, "zh", &units, &tr_for(&units))
            .unwrap();
        let body = String::from_utf8(outs[0].bytes.clone()).unwrap();
        assert_eq!(body, "a,译:一行目|二行目\n");
    }
}
