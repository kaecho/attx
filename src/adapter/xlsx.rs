//! XLSX adapter — translates the shared-string table.
//!
//! An .xlsx is a zip; almost every writer stores cell text in
//! `xl/sharedStrings.xml` as `<si>` items (plain `<t>` or rich-text
//! `<r><t>…` runs). One unit per `<si>`: runs are concatenated for the model;
//! writeback puts the translation into the first `<t>` and empties the rest
//! (same strategy as docx). All other zip entries are copied through raw.
//! Inline strings (`<is>` inside sheets) are rare and currently skipped.

use super::xmllite::{self, Node};
use super::{FormatAdapter, OutputFile, output_sibling};
use crate::model::{ItemType, TextUnit, Translation, needs_translation};
use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::Path;

pub struct XlsxAdapter;

const SHARED: &str = "xl/sharedStrings.xml";

impl FormatAdapter for XlsxAdapter {
    fn id(&self) -> &'static str {
        "xlsx"
    }
    fn label(&self) -> &'static str {
        "Excel workbook (xlsx)"
    }
    fn extensions(&self) -> &'static [&'static str] {
        &["xlsx", "xlsm"]
    }

    fn extract(&self, input: &Path, source_lang: &str) -> Result<Vec<TextUnit>> {
        let file = std::fs::File::open(input).with_context(|| format!("{}", input.display()))?;
        let mut zip = zip::ZipArchive::new(file).context("open xlsx zip")?;
        let mut body = String::new();
        match zip.by_name(SHARED) {
            Ok(mut entry) => {
                entry.read_to_string(&mut body)?;
            }
            Err(_) => return Ok(vec![]), // no shared strings — nothing translatable
        }
        let tree = xmllite::parse(&body).context("parse sharedStrings.xml")?;
        let mut units = Vec::new();
        for (idx, text) in collect_si_texts(&tree) {
            let text = text.trim();
            if text.is_empty() || !needs_translation(text, source_lang) {
                continue;
            }
            let location = format!("si{idx:06}");
            let lines: Vec<String> = text.split('\n').map(str::to_string).collect();
            let item_type = if lines.len() > 1 {
                ItemType::LongText
            } else {
                ItemType::ShortText
            };
            units.push(TextUnit {
                id: TextUnit::compute_id("xlsx", &location, &lines),
                engine: "xlsx".into(),
                domain: "table".into(),
                location,
                item_type,
                role: String::new(),
                original_lines: lines,
                source_line_paths: vec![],
                context: format!("s{:04}", idx / 40),
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
        let mut by_si: BTreeMap<usize, String> = BTreeMap::new();
        for u in units {
            let Some(tr) = translations.get(&u.id) else {
                continue;
            };
            if let Ok(idx) = u.location.trim_start_matches("si").parse::<usize>() {
                by_si.insert(idx, tr.translation_lines.join("\n"));
            }
        }
        let file = std::fs::File::open(input).with_context(|| format!("{}", input.display()))?;
        let mut zip = zip::ZipArchive::new(file)?;
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut buf);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            for i in 0..zip.len() {
                let name = zip.by_index_raw(i)?.name().to_string();
                if name == SHARED && !by_si.is_empty() {
                    let mut body = String::new();
                    zip.by_index(i)?.read_to_string(&mut body)?;
                    let rewritten = rewrite_shared(&body, &by_si)?;
                    writer.start_file(name, options)?;
                    writer.write_all(rewritten.as_bytes())?;
                } else {
                    writer.raw_copy_file(zip.by_index_raw(i)?)?;
                }
            }
            writer.finish()?;
        }
        let ext = input
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("xlsx")
            .to_ascii_lowercase();
        Ok(vec![OutputFile {
            path: output_sibling(input, target_lang, &ext),
            bytes: buf.into_inner(),
        }])
    }
}

/// `(si index, concatenated <t> text)` in document order.
fn collect_si_texts(nodes: &[Node]) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut idx = 0usize;
    fn walk(nodes: &[Node], idx: &mut usize, out: &mut Vec<(usize, String)>) {
        for n in nodes {
            if let Node::Elem { name, children, .. } = n {
                if local_name(name) == "si" {
                    let mut text = String::new();
                    collect_t_text(children, &mut text);
                    out.push((*idx, text));
                    *idx += 1;
                } else {
                    walk(children, idx, out);
                }
            }
        }
    }
    walk(nodes, &mut idx, &mut out);
    out
}

fn local_name(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

fn collect_t_text(nodes: &[Node], out: &mut String) {
    for n in nodes {
        if let Node::Elem { name, children, .. } = n {
            if local_name(name) == "t" {
                for c in children {
                    if let Node::Text(t) = c {
                        out.push_str(&xmllite::unescape(t));
                    }
                }
            } else if local_name(name) != "rPh" {
                // rPh = phonetic ruby runs — reading aids, not content
                collect_t_text(children, out);
            }
        }
    }
}

fn rewrite_shared(body: &str, by_si: &BTreeMap<usize, String>) -> Result<String> {
    let mut tree = xmllite::parse(body).context("reparse sharedStrings.xml")?;
    let mut idx = 0usize;
    fn walk(nodes: &mut [Node], idx: &mut usize, by_si: &BTreeMap<usize, String>) {
        for n in nodes {
            if let Node::Elem { name, children, .. } = n {
                if local_name(name) == "si" {
                    if let Some(text) = by_si.get(idx) {
                        let mut first = true;
                        replace_t(children, text, &mut first);
                    }
                    *idx += 1;
                } else {
                    walk(children, idx, by_si);
                }
            }
        }
    }
    walk(&mut tree, &mut idx, by_si);
    let mut out = String::new();
    xmllite::serialize(&tree, &mut out);
    Ok(out)
}

fn replace_t(nodes: &mut [Node], text: &str, first: &mut bool) {
    for n in nodes {
        if let Node::Elem {
            name,
            children,
            raw_open,
            ..
        } = n
        {
            if local_name(name) == "t" {
                if *first {
                    if !raw_open.contains("xml:space") {
                        *raw_open = raw_open.trim_end_matches('>').to_string()
                            + r#" xml:space="preserve">"#;
                    }
                    *children = vec![Node::Text(xmllite::escape(text))];
                    *first = false;
                } else {
                    *children = vec![];
                }
            } else {
                replace_t(children, text, first);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHARED_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="3" uniqueCount="3"><si><t>こんにちは</t></si><si><r><rPr><b/></rPr><t>強い</t></r><r><t>スライム</t></r></si><si><t>ascii</t></si></sst>"#;

    #[test]
    fn xlsx_roundtrip() {
        let dir = super::super::test_dir("xlsx");
        let input = dir.join("b.xlsx");
        let f = std::fs::File::create(&input).unwrap();
        let mut w = zip::ZipWriter::new(f);
        let opt = zip::write::SimpleFileOptions::default();
        w.start_file("[Content_Types].xml", opt).unwrap();
        w.write_all(b"<Types/>").unwrap();
        w.start_file("xl/worksheets/sheet1.xml", opt).unwrap();
        w.write_all(
            br#"<worksheet><sheetData><row><c t="s"><v>0</v></c></row></sheetData></worksheet>"#,
        )
        .unwrap();
        w.start_file(SHARED, opt).unwrap();
        w.write_all(SHARED_XML.as_bytes()).unwrap();
        w.finish().unwrap();

        let units = XlsxAdapter.extract(&input, "ja").unwrap();
        assert_eq!(units.len(), 2, "{units:#?}");
        assert_eq!(units[1].original_lines, ["強いスライム"], "runs joined");

        let mut tr = BTreeMap::new();
        for (u, text) in units.iter().zip(["你好", "强大的史莱姆"]) {
            tr.insert(
                u.id.clone(),
                Translation {
                    unit_id: u.id.clone(),
                    translation_lines: vec![text.to_string()],
                    source_hash: TextUnit::source_hash(&u.original_lines),
                    passthrough: false,
                },
            );
        }
        let outs = XlsxAdapter.writeback(&input, "zh", &units, &tr).unwrap();
        std::fs::write(&outs[0].path, &outs[0].bytes).unwrap();
        let mut zip = zip::ZipArchive::new(std::fs::File::open(&outs[0].path).unwrap()).unwrap();
        let mut body = String::new();
        zip.by_name(SHARED)
            .unwrap()
            .read_to_string(&mut body)
            .unwrap();
        assert!(body.contains("你好"), "{body}");
        assert!(body.contains("强大的史莱姆"), "{body}");
        assert!(!body.contains("スライム"), "second run emptied: {body}");
        assert!(
            body.contains("<t>ascii</t>"),
            "untranslated si intact: {body}"
        );
        let mut sheet = String::new();
        zip.by_name("xl/worksheets/sheet1.xml")
            .unwrap()
            .read_to_string(&mut sheet)
            .unwrap();
        assert!(sheet.contains("<v>0</v>"), "sheets untouched");
    }
}
