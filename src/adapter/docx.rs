//! DOCX adapter (AiNiee DocxReader equivalent).
//!
//! A .docx is a zip; body text lives in `word/document.xml` (plus foot/end
//! notes) as `<w:p>` paragraphs containing `<w:t>` runs. Word splits sentences
//! across runs arbitrarily (spell-check artifacts), so extraction concatenates
//! all runs of a paragraph into one unit; writeback puts the translation into
//! the first run and empties the rest — standard practice for machine docx
//! translation, keeps paragraph-level styling.
//
// ponytail: w:br/w:tab become spaces and per-run character styling inside a
// paragraph collapses to the first run's style; build real run splitting if
// mixed-style paragraphs matter.

use super::xmllite::{self, Node};
use super::{FormatAdapter, OutputFile, output_sibling};
use crate::model::{ItemType, TextUnit, Translation, needs_translation};
use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::Path;

pub struct DocxAdapter;

const DOC_ENTRIES: &[&str] = &[
    "word/document.xml",
    "word/footnotes.xml",
    "word/endnotes.xml",
];

impl FormatAdapter for DocxAdapter {
    fn id(&self) -> &'static str {
        "docx"
    }
    fn label(&self) -> &'static str {
        "Word document (docx)"
    }
    fn extensions(&self) -> &'static [&'static str] {
        &["docx"]
    }

    fn extract(&self, input: &Path, source_lang: &str) -> Result<Vec<TextUnit>> {
        let file = std::fs::File::open(input).with_context(|| format!("{}", input.display()))?;
        let mut zip = zip::ZipArchive::new(file).context("open docx zip")?;
        let mut units = Vec::new();
        for entry_name in DOC_ENTRIES {
            let Ok(mut entry) = zip.by_name(entry_name) else {
                continue;
            };
            let mut body = String::new();
            entry.read_to_string(&mut body)?;
            drop(entry);
            let tree = xmllite::parse(&body).with_context(|| entry_name.to_string())?;
            let mut paras = Vec::new();
            collect_paragraphs(&tree, &mut Vec::new(), &mut paras);
            for (idx, (_path, text)) in paras.iter().enumerate() {
                let text = text.trim();
                if text.is_empty() || !needs_translation(text, source_lang) {
                    continue;
                }
                let location = format!("{entry_name}#b{idx:05}");
                let lines = vec![text.to_string()];
                units.push(TextUnit {
                    id: TextUnit::compute_id("docx", &location, &lines),
                    engine: "docx".into(),
                    domain: "document".into(),
                    location,
                    item_type: ItemType::LongText,
                    role: String::new(),
                    original_lines: lines,
                    source_line_paths: vec![],
                    context: format!("{entry_name}/s{:04}", idx / 30),
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
        let mut per_entry: BTreeMap<String, BTreeMap<usize, String>> = BTreeMap::new();
        for u in units {
            let Some(tr) = translations.get(&u.id) else {
                continue;
            };
            let Some((entry, idx)) = u.location.rsplit_once("#b") else {
                continue;
            };
            let Ok(idx) = idx.parse::<usize>() else {
                continue;
            };
            let glue = if target_lang.starts_with("zh") || target_lang.starts_with("ja") {
                ""
            } else {
                " "
            };
            per_entry
                .entry(entry.to_string())
                .or_default()
                .insert(idx, tr.translation_lines.join(glue));
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
                if let Some(blocks) = per_entry.get(&name) {
                    let mut body = String::new();
                    zip.by_index(i)?.read_to_string(&mut body)?;
                    let rewritten = rewrite_document_xml(&body, &name, blocks)?;
                    writer.start_file(name, options)?;
                    writer.write_all(rewritten.as_bytes())?;
                } else {
                    writer.raw_copy_file(zip.by_index_raw(i)?)?;
                }
            }
            writer.finish()?;
        }
        Ok(vec![OutputFile {
            path: output_sibling(input, target_lang, "docx"),
            bytes: buf.into_inner(),
        }])
    }
}

/// Leaf `<w:p>` paragraphs with their concatenated `<w:t>` text.
fn collect_paragraphs(
    nodes: &[Node],
    path: &mut Vec<usize>,
    out: &mut Vec<(xmllite::NodePath, String)>,
) {
    for (i, node) in nodes.iter().enumerate() {
        if let Node::Elem { name, children, .. } = node {
            path.push(i);
            if name == "w:p" && !subtree_has_paragraph(children) {
                let mut text = String::new();
                collect_wt_text(children, &mut text);
                out.push((path.clone(), text));
            } else {
                collect_paragraphs(children, path, out);
            }
            path.pop();
        }
    }
}

fn subtree_has_paragraph(nodes: &[Node]) -> bool {
    nodes.iter().any(|n| match n {
        Node::Elem { name, children, .. } => name == "w:p" || subtree_has_paragraph(children),
        _ => false,
    })
}

fn collect_wt_text(nodes: &[Node], out: &mut String) {
    for n in nodes {
        match n {
            Node::Elem { name, children, .. } if name == "w:t" => {
                for c in children {
                    if let Node::Text(t) = c {
                        out.push_str(&xmllite::unescape(t));
                    }
                }
            }
            Node::Elem { name, children, .. } => {
                if name == "w:br" || name == "w:tab" {
                    out.push(' ');
                }
                collect_wt_text(children, out);
            }
            Node::SelfClosing { name, .. } if name == "w:br" || name == "w:tab" => out.push(' '),
            _ => {}
        }
    }
}

/// Set the paragraph's first `w:t` to the translation, blank the others.
fn rewrite_document_xml(
    body: &str,
    name: &str,
    blocks: &BTreeMap<usize, String>,
) -> Result<String> {
    let mut tree = xmllite::parse(body).with_context(|| format!("reparse {name}"))?;
    let mut paras = Vec::new();
    collect_paragraphs(&tree, &mut Vec::new(), &mut paras);
    for (idx, text) in blocks {
        let Some((path, _)) = paras.get(*idx) else {
            eprintln!("docx: paragraph {idx} missing in {name}");
            continue;
        };
        let Some(Node::Elem { children, .. }) = xmllite::node_at_mut(&mut tree, path) else {
            continue;
        };
        let mut first = true;
        replace_wt(children, text, &mut first);
    }
    let mut out = String::new();
    xmllite::serialize(&tree, &mut out);
    Ok(out)
}

fn replace_wt(nodes: &mut [Node], text: &str, first: &mut bool) {
    for n in nodes {
        if let Node::Elem {
            name,
            children,
            raw_open,
            ..
        } = n
        {
            if name == "w:t" {
                if *first {
                    // keep whitespace-significant text intact in Word
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
                replace_wt(children, text, first);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC_XML: &str = r#"<?xml version="1.0"?><w:document xmlns:w="ns"><w:body>
<w:p><w:r><w:rPr/><w:t>これは</w:t></w:r><w:r><w:t>テストです。</w:t></w:r></w:p>
<w:p><w:r><w:t>ascii only</w:t></w:r></w:p>
</w:body></w:document>"#;

    #[test]
    fn docx_roundtrip() {
        let dir = super::super::test_dir("docx");
        let input = dir.join("d.docx");
        let f = std::fs::File::create(&input).unwrap();
        let mut w = zip::ZipWriter::new(f);
        let opt = zip::write::SimpleFileOptions::default();
        w.start_file("[Content_Types].xml", opt).unwrap();
        w.write_all(b"<Types/>").unwrap();
        w.start_file("word/document.xml", opt).unwrap();
        w.write_all(DOC_XML.as_bytes()).unwrap();
        w.finish().unwrap();

        let units = DocxAdapter.extract(&input, "ja").unwrap();
        assert_eq!(units.len(), 1);
        assert_eq!(units[0].original_lines, ["これはテストです。"]);
        let mut tr = BTreeMap::new();
        tr.insert(
            units[0].id.clone(),
            Translation {
                unit_id: units[0].id.clone(),
                translation_lines: vec!["这是测试。".into()],
                source_hash: TextUnit::source_hash(&units[0].original_lines),
            },
        );
        let outs = DocxAdapter.writeback(&input, "zh", &units, &tr).unwrap();
        std::fs::write(&outs[0].path, &outs[0].bytes).unwrap();
        let mut zip = zip::ZipArchive::new(std::fs::File::open(&outs[0].path).unwrap()).unwrap();
        let mut body = String::new();
        zip.by_name("word/document.xml")
            .unwrap()
            .read_to_string(&mut body)
            .unwrap();
        assert!(body.contains("这是测试。"), "{body}");
        assert!(!body.contains("テストです"), "second run emptied: {body}");
        assert!(body.contains("ascii only"), "{body}");
    }
}
