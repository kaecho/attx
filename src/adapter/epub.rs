//! EPUB adapter (AiNiee EpubReader/Writer equivalent).
//!
//! An EPUB is a zip of XHTML chapters plus metadata. Extraction walks every
//! chapter and yields one unit per *leaf block element* (`<p>`, `<h1>`…) —
//! block-level granularity gives the model whole sentences (ruby readings
//! `<rt>` are dropped from the source text). Writeback replaces the block's
//! inner content with the translated text, keeping the tag and attributes; all
//! other zip entries (images, css, fonts) are copied through untouched.
//!
//! Output: `<stem>.<target_lang>.epub` beside the input. The original file is
//! never modified.

use super::xmllite::{self, Node};
use super::{DetectHit, FormatAdapter, OutputFile, output_sibling};
use crate::model::{ItemType, TextUnit, Translation, needs_translation};
use crate::textio;
use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::Path;

pub struct EpubAdapter;
/// Standalone HTML/XHTML files — same block-level extraction as EPUB chapters.
pub struct HtmlAdapter;

/// Block-level tags: a leaf occurrence of one of these is a translation unit.
const BLOCK_TAGS: &[&str] = &[
    "p",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "li",
    "dt",
    "dd",
    "th",
    "td",
    "caption",
    "figcaption",
    "blockquote",
    "div",
];
/// Subtrees skipped during text collection (ruby readings, code).
const SKIP_TAGS: &[&str] = &["rt", "rp", "script", "style"];

impl FormatAdapter for EpubAdapter {
    fn id(&self) -> &'static str {
        "epub"
    }
    fn label(&self) -> &'static str {
        "EPUB e-book"
    }
    fn extensions(&self) -> &'static [&'static str] {
        &["epub"]
    }

    fn detect(&self, input: &Path) -> Option<DetectHit> {
        super::detect_by_extension(self.id(), self.label(), self.extensions(), input)
    }

    fn extract(&self, input: &Path, source_lang: &str) -> Result<Vec<TextUnit>> {
        let file = std::fs::File::open(input).with_context(|| format!("{}", input.display()))?;
        let mut zip = zip::ZipArchive::new(file).context("open epub zip")?;
        let mut units = Vec::new();
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i)?;
            let name = entry.name().to_string();
            let Some(tags) = doc_block_tags(&name) else {
                continue;
            };
            let mut body = String::new();
            entry
                .read_to_string(&mut body)
                .with_context(|| name.clone())?;
            let tree = xmllite::parse(&body).with_context(|| format!("parse {name}"))?;
            let mut blocks = Vec::new();
            collect_leaf_blocks(&tree, tags, &mut Vec::new(), &mut blocks);
            for (idx, (_path, text)) in blocks.iter().enumerate() {
                let lines: Vec<String> = text
                    .split('\n')
                    .map(|l| l.trim().to_string())
                    .filter(|l| !l.is_empty())
                    .collect();
                if lines.is_empty() || !lines.iter().any(|l| needs_translation(l, source_lang)) {
                    continue;
                }
                let location = format!("{name}#b{idx:05}");
                units.push(TextUnit {
                    id: TextUnit::compute_id("epub", &location, &lines),
                    engine: "epub".into(),
                    domain: "ebook".into(),
                    location,
                    item_type: ItemType::LongText,
                    role: String::new(),
                    original_lines: lines,
                    source_line_paths: vec![],
                    context: name.clone(),
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
        // entry name → (block idx → translated lines)
        let mut per_entry: BTreeMap<String, BTreeMap<usize, Vec<String>>> = BTreeMap::new();
        for u in units {
            let Some(tr) = translations.get(&u.id) else {
                continue;
            };
            let Some((entry, idx)) = split_location(&u.location) else {
                continue;
            };
            per_entry
                .entry(entry)
                .or_default()
                .insert(idx, tr.translation_lines.clone());
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
                let is_opf = name.to_ascii_lowercase().ends_with(".opf");
                if let Some(blocks) = per_entry.get(&name) {
                    let mut body = String::new();
                    zip.by_index(i)?.read_to_string(&mut body)?;
                    let mut rewritten = rewrite_document(&body, &name, blocks)?;
                    if is_opf {
                        rewritten = set_opf_language(&rewritten, target_lang);
                    }
                    writer.start_file(name, options)?;
                    writer.write_all(rewritten.as_bytes())?;
                } else if is_opf {
                    let mut body = String::new();
                    zip.by_index(i)?.read_to_string(&mut body)?;
                    let rewritten = set_opf_language(&body, target_lang);
                    writer.start_file(name, options)?;
                    writer.write_all(rewritten.as_bytes())?;
                } else {
                    // raw copy preserves compression — including the spec-required
                    // stored (uncompressed) `mimetype` first entry.
                    writer.raw_copy_file(zip.by_index_raw(i)?)?;
                }
            }
            writer.finish()?;
        }
        Ok(vec![OutputFile {
            path: output_sibling(input, target_lang, "epub"),
            bytes: buf.into_inner(),
        }])
    }
}

impl FormatAdapter for HtmlAdapter {
    fn id(&self) -> &'static str {
        "html"
    }
    fn label(&self) -> &'static str {
        "HTML page"
    }
    fn extensions(&self) -> &'static [&'static str] {
        &["html", "htm", "xhtml"]
    }

    fn extract(&self, input: &Path, source_lang: &str) -> Result<Vec<TextUnit>> {
        let body = textio::read_text(input)?;
        let name = input
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "input.html".into());
        let tree = xmllite::parse(&body).with_context(|| format!("parse {name}"))?;
        let mut blocks = Vec::new();
        collect_leaf_blocks(&tree, HTML_TAGS, &mut Vec::new(), &mut blocks);
        let mut units = Vec::new();
        for (idx, (_path, text)) in blocks.iter().enumerate() {
            let lines: Vec<String> = text
                .split('\n')
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect();
            if lines.is_empty() || !lines.iter().any(|l| needs_translation(l, source_lang)) {
                continue;
            }
            let location = format!("{name}#b{idx:05}");
            units.push(TextUnit {
                id: TextUnit::compute_id("html", &location, &lines),
                engine: "html".into(),
                domain: "html".into(),
                location,
                item_type: ItemType::LongText,
                role: String::new(),
                original_lines: lines,
                source_line_paths: vec![],
                context: format!("s{:04}", idx / 30),
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
        let mut blocks: BTreeMap<usize, Vec<String>> = BTreeMap::new();
        for u in units {
            let Some(tr) = translations.get(&u.id) else {
                continue;
            };
            let Some((_, idx)) = split_location(&u.location) else {
                continue;
            };
            blocks.insert(idx, tr.translation_lines.clone());
        }
        let rewritten = rewrite_blocks(&body, "html input", HTML_TAGS, &blocks)?;
        let ext = input
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("html")
            .to_ascii_lowercase();
        Ok(vec![OutputFile::text(
            output_sibling(input, target_lang, &ext),
            rewritten,
        )])
    }
}

/// HTML pages also translate `<title>`.
const HTML_TAGS: &[&str] = &[
    "p",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "li",
    "dt",
    "dd",
    "th",
    "td",
    "caption",
    "figcaption",
    "blockquote",
    "div",
    "title",
];

/// Which block tags apply to a zip entry, or None if not a text document.
fn doc_block_tags(name: &str) -> Option<&'static [&'static str]> {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".xhtml") || lower.ends_with(".html") || lower.ends_with(".htm") {
        Some(BLOCK_TAGS)
    } else if lower.ends_with(".ncx") {
        Some(&["text"]) // EPUB2 table of contents labels
    } else if lower.ends_with(".opf") {
        Some(&["dc:title"]) // book title metadata
    } else {
        None
    }
}

fn split_location(location: &str) -> Option<(String, usize)> {
    let (entry, idx) = location.rsplit_once("#b")?;
    Some((entry.to_string(), idx.parse().ok()?))
}

/// Depth-first walk collecting leaf blocks: candidate elements whose subtree
/// contains no further candidate. Index order must match between extract and
/// writeback — both go through this function.
fn collect_leaf_blocks(
    nodes: &[Node],
    tags: &[&str],
    path: &mut Vec<usize>,
    out: &mut Vec<(xmllite::NodePath, String)>,
) {
    for (i, node) in nodes.iter().enumerate() {
        if let Node::Elem { name, children, .. } = node {
            path.push(i);
            if is_tag(name, tags) && !subtree_has_tag(children, tags) {
                let mut text = String::new();
                collect_text(children, &mut text);
                out.push((path.clone(), text));
            } else {
                collect_leaf_blocks(children, tags, path, out);
            }
            path.pop();
        }
    }
}

fn is_tag(name: &str, tags: &[&str]) -> bool {
    tags.iter().any(|t| t.eq_ignore_ascii_case(name))
}

fn subtree_has_tag(nodes: &[Node], tags: &[&str]) -> bool {
    nodes.iter().any(|n| match n {
        Node::Elem { name, children, .. } => is_tag(name, tags) || subtree_has_tag(children, tags),
        _ => false,
    })
}

fn collect_text(nodes: &[Node], out: &mut String) {
    for n in nodes {
        match n {
            Node::Text(t) => out.push_str(&xmllite::unescape(t)),
            Node::Elem { name, children, .. } => {
                if !is_tag(name, SKIP_TAGS) {
                    collect_text(children, out);
                }
            }
            Node::SelfClosing { name, .. } => {
                if name.eq_ignore_ascii_case("br") {
                    out.push('\n');
                }
            }
            Node::Misc(_) => {}
        }
    }
}

/// Replace the inner content of translated blocks; leave everything else as-is.
fn rewrite_document(
    body: &str,
    name: &str,
    blocks: &BTreeMap<usize, Vec<String>>,
) -> Result<String> {
    let tags = doc_block_tags(name).unwrap_or(BLOCK_TAGS);
    rewrite_blocks(body, name, tags, blocks)
}

fn rewrite_blocks(
    body: &str,
    name: &str,
    tags: &[&str],
    blocks: &BTreeMap<usize, Vec<String>>,
) -> Result<String> {
    let mut tree = xmllite::parse(body).with_context(|| format!("reparse {name}"))?;
    let mut located = Vec::new();
    collect_leaf_blocks(&tree, tags, &mut Vec::new(), &mut located);
    for (idx, lines) in blocks {
        let Some((path, _)) = located.get(*idx) else {
            eprintln!("epub: block {idx} missing in {name} (file changed since extract?)");
            continue;
        };
        let Some(Node::Elem { children, .. }) = xmllite::node_at_mut(&mut tree, path) else {
            continue;
        };
        let mut new_children = Vec::new();
        for (i, line) in lines.iter().enumerate() {
            if i > 0 {
                new_children.push(Node::SelfClosing {
                    name: "br".into(),
                    raw: "<br/>".into(),
                });
            }
            new_children.push(Node::Text(xmllite::escape(line)));
        }
        *children = new_children;
    }
    let mut out = String::new();
    xmllite::serialize(&tree, &mut out);
    Ok(out)
}

/// Point `<dc:language>` at the translation target.
fn set_opf_language(body: &str, target_lang: &str) -> String {
    let Some(start) = body.find("<dc:language") else {
        return body.to_string();
    };
    let Some(gt) = body[start..].find('>') else {
        return body.to_string();
    };
    let content_start = start + gt + 1;
    let Some(end) = body[content_start..].find("</dc:language>") else {
        return body.to_string();
    };
    format!(
        "{}{}{}",
        &body[..content_start],
        target_lang,
        &body[content_start + end..]
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    const CHAPTER: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml"><head><title>ch1</title></head>
<body><div class="main">
<p>これは<ruby>直哉<rt>なおや</rt></ruby>の物語。</p>
<p><img src="a.png"/></p>
<p>二行目<br/>三行目</p>
</div></body></html>"#;

    fn build_epub(dir: &Path) -> std::path::PathBuf {
        let path = dir.join("book.epub");
        let f = std::fs::File::create(&path).unwrap();
        let mut w = zip::ZipWriter::new(f);
        let stored = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        let deflated = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        w.start_file("mimetype", stored).unwrap();
        w.write_all(b"application/epub+zip").unwrap();
        w.start_file("content.opf", deflated).unwrap();
        w.write_all(
            r#"<package><metadata><dc:title>負けた</dc:title><dc:language>ja</dc:language></metadata></package>"#.as_bytes(),
        )
        .unwrap();
        w.start_file("ch1.xhtml", deflated).unwrap();
        w.write_all(CHAPTER.as_bytes()).unwrap();
        w.finish().unwrap();
        path
    }

    #[test]
    fn epub_roundtrip() {
        let dir = super::super::test_dir("epub");
        let input = build_epub(&dir);
        let units = EpubAdapter.extract(&input, "ja").unwrap();
        // dc:title + 2 text paragraphs (image-only block skipped)
        assert_eq!(units.len(), 3, "{units:#?}");
        assert!(
            units
                .iter()
                .any(|u| u.original_lines == ["これは直哉の物語。"])
        );
        assert!(
            units
                .iter()
                .any(|u| u.original_lines == ["二行目", "三行目"])
        );

        let mut tr = BTreeMap::new();
        for u in &units {
            tr.insert(
                u.id.clone(),
                Translation {
                    unit_id: u.id.clone(),
                    translation_lines: u.original_lines.iter().map(|l| format!("译:{l}")).collect(),
                    source_hash: TextUnit::source_hash(&u.original_lines),
                    passthrough: false,
                },
            );
        }
        let outs = EpubAdapter.writeback(&input, "zh", &units, &tr).unwrap();
        assert_eq!(outs.len(), 1);
        assert!(outs[0].path.to_string_lossy().ends_with("book.zh.epub"));
        std::fs::write(&outs[0].path, &outs[0].bytes).unwrap();

        let mut zip = zip::ZipArchive::new(std::fs::File::open(&outs[0].path).unwrap()).unwrap();
        // mimetype must stay stored & first
        assert_eq!(zip.by_index(0).unwrap().name(), "mimetype");
        let mut body = String::new();
        zip.by_name("ch1.xhtml")
            .unwrap()
            .read_to_string(&mut body)
            .unwrap();
        assert!(body.contains("译:これは直哉の物語。"), "{body}");
        assert!(body.contains("译:二行目<br/>译:三行目"), "{body}");
        assert!(body.contains(r#"<p><img src="a.png"/></p>"#), "{body}");
        let mut opf = String::new();
        zip.by_name("content.opf")
            .unwrap()
            .read_to_string(&mut opf)
            .unwrap();
        assert!(opf.contains("<dc:language>zh</dc:language>"), "{opf}");
        assert!(opf.contains("译:負けた"), "{opf}");
    }

    #[test]
    fn html_roundtrip() {
        let dir = super::super::test_dir("html");
        let input = dir.join("page.html");
        std::fs::write(
            &input,
            "<html><head><title>物語</title></head><body><p>これは本文。</p><p>ascii only</p></body></html>",
        )
        .unwrap();
        let units = HtmlAdapter.extract(&input, "ja").unwrap();
        assert_eq!(units.len(), 2, "title + jp paragraph: {units:#?}");
        let mut tr = BTreeMap::new();
        for u in &units {
            tr.insert(
                u.id.clone(),
                Translation {
                    unit_id: u.id.clone(),
                    translation_lines: vec![format!("译:{}", u.original_lines[0])],
                    source_hash: TextUnit::source_hash(&u.original_lines),
                    passthrough: false,
                },
            );
        }
        let outs = HtmlAdapter.writeback(&input, "zh", &units, &tr).unwrap();
        let body = String::from_utf8(outs[0].bytes.clone()).unwrap();
        assert!(body.contains("<title>译:物語</title>"), "{body}");
        assert!(body.contains("<p>译:これは本文。</p>"), "{body}");
        assert!(body.contains("<p>ascii only</p>"), "{body}");
        assert!(outs[0].path.to_string_lossy().ends_with("page.zh.html"));
    }
}
