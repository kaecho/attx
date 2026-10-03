//! Content-sniffed translation for text formats without a dedicated adapter.
//! Byte spans keep untouched syntax intact. Directory output copies unsupported
//! files and ambiguous language-tagged assets unchanged. Metadata/generated
//! directories, backups, and symlinks are intentionally omitted and reported.
//! Scalar sniffing accepts a conservative subset, not arbitrary source code or
//! every INI/TOML/YAML document; code and delimited-file extensions stay opaque.

use super::{DetectHit, FormatAdapter, OutputFile, xmllite};
use crate::model::{ItemType, TextUnit, Translation, needs_translation};
use crate::textio;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

pub struct AutoAdapter;

/// Files that Auto can extract versus files it will only copy unchanged.
#[derive(Debug, Serialize)]
pub struct CoverageReport {
    pub supported_files: usize,
    pub copied_files: usize,
    pub unsupported_total: usize,
    pub unsupported_paths: Vec<String>,
    /// Excluded directory roots, backup files, and symlinks, not translated files.
    pub excluded_entries: usize,
    pub excluded_paths: Vec<String>,
}

pub fn coverage(input: &Path) -> Result<CoverageReport> {
    let discovered = discover_files(input)?;
    let mut report = CoverageReport {
        supported_files: 0, copied_files: 0, unsupported_total: 0,
        unsupported_paths: Vec::new(), excluded_entries: discovered.excluded_entries,
        excluded_paths: discovered.excluded_paths,
    };
    for file in discovered.files {
        if read_document(&file)?.is_some_and(|doc| !doc.spans.is_empty()) {
            report.supported_files += 1;
        } else {
            report.copied_files += 1;
            report.unsupported_total += 1;
            if report.unsupported_paths.len() < 50 {
                report.unsupported_paths.push(relative_file(input, &file)?);
            }
        }
    }
    Ok(report)
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
enum Style {
    Json,
    Xml,
    Double,
    SingleYaml,
    SingleLiteral,
    BareYaml,
    BareIni,
    Prose,
}

#[derive(Debug)]
struct Span {
    start: usize,
    end: usize,
    text: String,
    style: Style,
}

#[derive(Serialize, Deserialize)]
struct Anchor {
    file: String,
    hash: String,
    start: usize,
    end: usize,
    style: Style,
}

struct Document {
    bytes: Vec<u8>,
    text: String,
    encoding: &'static str,
    format: &'static str,
    spans: Vec<Span>,
}

impl FormatAdapter for AutoAdapter {
    fn id(&self) -> &'static str { "auto" }
    fn label(&self) -> &'static str { "Automatic structured text" }
    fn input_kind(&self) -> &'static str { "file|directory" }

    fn detect(&self, input: &Path) -> Option<DetectHit> {
        let files = candidate_files(input).ok()?;
        if !files.iter().any(|file| {
            read_document(file).ok().flatten().is_some_and(|d| !d.spans.is_empty())
        }) {
            return None;
        }
        Some(DetectHit {
            engine_id: self.id(),
            label: self.label(),
            content_root: input.canonicalize().ok()?,
        })
    }

    fn extract(&self, input: &Path, source_lang: &str) -> Result<Vec<TextUnit>> {
        let mut units = Vec::new();
        for file in candidate_files(input)? {
            let Some(doc) = read_document(&file)? else { continue };
            let rel = relative_file(input, &file)?;
            let hash = digest(&doc.bytes);
            for span in doc.spans {
                if !needs_translation(&span.text, source_lang) { continue; }
                let location = format!("{rel}#auto:{}:{}", span.start, span.end);
                let original_lines = vec![span.text];
                let anchor = Anchor {
                    file: rel.clone(), hash: hash.clone(), start: span.start,
                    end: span.end, style: span.style,
                };
                units.push(TextUnit {
                    id: TextUnit::compute_id(self.id(), &location, &original_lines),
                    engine: self.id().into(), domain: doc.format.into(), location: location.clone(),
                    item_type: ItemType::LongText, role: String::new(), original_lines,
                    source_line_paths: vec![location], context: rel.clone(),
                    payload: serde_json::to_string(&anchor)?,
                });
            }
        }
        Ok(units)
    }

    fn writeback(
        &self, input: &Path, target_lang: &str, units: &[TextUnit],
        translations: &BTreeMap<String, Translation>,
    ) -> Result<Vec<OutputFile>> {
        if target_lang.is_empty() || !target_lang.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            bail!("auto output requires a language tag containing only letters, digits, '-' or '_'");
        }
        let files = candidate_files(input)?;
        let mut grouped: BTreeMap<String, Vec<(&TextUnit, Anchor)>> = BTreeMap::new();
        for unit in units {
            if unit.engine != self.id() { bail!("foreign unit {} in auto writeback", unit.id); }
            let anchor: Anchor = serde_json::from_str(&unit.payload).context("invalid auto source anchor")?;
            validate_relative(&anchor.file)?;
            grouped.entry(anchor.file.clone()).or_default().push((unit, anchor));
        }
        let mut outputs = Vec::new();
        for file in files {
            let rel = relative_file(input, &file)?;
            let bytes = std::fs::read(&file).with_context(|| format!("read {}", file.display()))?;
            let permissions = Some(std::fs::metadata(&file)
                .with_context(|| format!("inspect source permissions {}", file.display()))?.permissions());
            let output;
            if let Some(mut file_units) = grouped.remove(&rel) {
                let hash = digest(&bytes);
                let doc = read_document_bytes_for_path(&file, bytes)?.context("auto source is no longer supported text")?;
                file_units.sort_by_key(|(_, a)| a.start);
                let mut end = 0;
                let mut edits = Vec::new();
                for (unit, anchor) in file_units {
                    if anchor.hash != hash { bail!("source changed since extraction: {}", file.display()); }
                    let span = doc.spans.binary_search_by_key(&anchor.start, |s| s.start).ok()
                        .and_then(|index| doc.spans.get(index))
                        .filter(|s| s.end == anchor.end && s.style == anchor.style)
                        .context("auto source anchor no longer matches")?;
                    if anchor.start < end || unit.original_lines.len() != 1 || unit.original_lines[0] != span.text
                        || unit.location != format!("{rel}#auto:{}:{}", anchor.start, anchor.end)
                        || unit.id != TextUnit::compute_id(self.id(), &unit.location, &unit.original_lines) {
                        bail!("auto source anchor/content mismatch: {}", unit.location);
                    }
                    end = anchor.end;
                    if let Some(tr) = translations.get(&unit.id) {
                        if tr.unit_id != unit.id || tr.source_hash != TextUnit::source_hash(&unit.original_lines) {
                            bail!("stale translation for {}", unit.location);
                        }
                        if !tr.passthrough && !tr.translation_lines.is_empty() {
                            let text = tr.translation_lines.join("\n");
                            if text != span.text {
                                edits.push((anchor.start, anchor.end, render(span.style, &text)?));
                            }
                        }
                    }
                }
                if !edits.is_empty() {
                    let mut body = doc.text;
                    for (start, end, replacement) in edits.into_iter().rev() {
                        body.replace_range(start..end, &replacement);
                    }
                    if doc.format != "prose" {
                        let hint = match doc.format {
                            "ini" => Some(Dialect::Ini), "toml" => Some(Dialect::Toml), "yaml" => Some(Dialect::Yaml), _ => None,
                        };
                        let rendered = parse_document_with_hint(&body, hint).context("translation produced unsupported or invalid structure")?;
                        let scalar_equivalent = matches!(doc.format, "ini" | "toml") && matches!(rendered.0, "ini" | "toml");
                        if rendered.0 != doc.format && !scalar_equivalent { bail!("translation changed the detected format"); }
                    }
                    output = encode(&body, doc.encoding, &doc.bytes)?;
                } else {
                    output = doc.bytes;
                }
            }
            else {
                output = bytes;
            }
            let path = if input.is_dir() {
                safe_output_path(input, target_lang, &rel)?
            } else {
                sibling(input, target_lang)
            };
            if std::fs::symlink_metadata(&path).is_ok_and(|meta| meta.file_type().is_symlink()) {
                bail!("symlink in auto output path: {}", path.display());
            }
            outputs.push(OutputFile { path, bytes: output, permissions });
        }
        if let Some((file, _)) = grouped.first_key_value() {
            bail!("auto source file missing or excluded: {file}");
        }
        Ok(outputs)
    }
}

fn digest(bytes: &[u8]) -> String { format!("{:x}", Sha256::digest(bytes)) }

fn excluded(path: &Path, directory: bool) -> bool {
    let name = path.file_name().unwrap_or_default().to_string_lossy().to_ascii_lowercase();
    name.starts_with(".attx") || name == ".git" || name == "node_modules"
        || (directory && (name.starts_with("translated-") || name == "translated" || name == "backup" || name == "backups"))
        || name.ends_with(".attxbak") || name.ends_with(".bak") || name.ends_with(".backup")
        || name.ends_with('~')
}

fn translated_name(name: &str) -> bool {
    // Ambiguous language-tagged assets are copied, never silently omitted.
    let mut parts = name.rsplit('.');
    let Some(last) = parts.next() else { return false };
    let Some(previous) = parts.next() else { return false };
    language_tag(last) || (parts.next().is_some() && language_tag(previous))
}

fn language_tag(tag: &str) -> bool {
    let base = tag.split('-').next().unwrap_or(tag);
    ["af", "ar", "az", "be", "bg", "bn", "bs", "ca", "cs", "cy", "da", "de", "el", "en", "es", "et",
        "eu", "fa", "fi", "fr", "ga", "gl", "gu", "he", "hi", "hr", "hu", "hy", "id", "is", "it", "ja",
        "ka", "kk", "km", "kn", "ko", "ku", "ky", "la", "lo", "lt", "lv", "mk", "ml", "mn", "mr", "ms",
        "mt", "my", "nb", "ne", "nl", "nn", "no", "pa", "pl", "ps", "pt", "ro", "ru", "si", "sk", "sl",
        "sq", "sr", "sv", "sw", "ta", "te", "th", "tr", "uk", "ur", "uz", "vi", "zh", "zu", "eng", "jpn", "zho"]
        .contains(&base) && (tag == base || (!tag[base.len() + 1..].is_empty()
            && tag[base.len() + 1..].bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')))
}

struct DiscoveredFiles {
    files: Vec<PathBuf>,
    excluded_entries: usize,
    excluded_paths: Vec<String>,
}

fn candidate_files(input: &Path) -> Result<Vec<PathBuf>> {
    Ok(discover_files(input)?.files)
}

fn discover_files(input: &Path) -> Result<DiscoveredFiles> {
    let meta = std::fs::symlink_metadata(input).with_context(|| format!("inspect {}", input.display()))?;
    if meta.file_type().is_symlink() { bail!("auto does not follow symlink inputs"); }
    let mut discovered = DiscoveredFiles { files: Vec::new(), excluded_entries: 0, excluded_paths: Vec::new() };
    if meta.is_file() { discovered.files.push(input.to_path_buf()); return Ok(discovered); }
    if !meta.is_dir() { bail!("auto input must be a regular file or directory"); }
    for entry in walkdir::WalkDir::new(input).follow_links(false).sort_by_file_name().into_iter()
        .filter_entry(|e| {
            let omitted = e.depth() != 0 && (excluded(e.path(), e.file_type().is_dir()) || e.file_type().is_symlink());
            if omitted {
                discovered.excluded_entries += 1;
                if discovered.excluded_paths.len() < 50 {
                    discovered.excluded_paths.push(e.path().strip_prefix(input).unwrap_or(e.path()).to_string_lossy().into_owned());
                }
            }
            !omitted
        }) {
        let entry = entry?;
        if entry.file_type().is_file() { discovered.files.push(entry.into_path()); }
    }
    Ok(discovered)
}

fn relative_file(input: &Path, file: &Path) -> Result<String> {
    let path = if input.is_dir() { file.strip_prefix(input)? } else {
        input.file_name().map(Path::new).context("input has no filename")?
    };
    let rel = path.to_str().context("auto source filenames must be valid UTF-8")?.to_string();
    validate_relative(&rel)?;
    Ok(rel)
}

fn validate_relative(rel: &str) -> Result<()> {
    if rel.is_empty() || Path::new(rel).components().any(|c| !matches!(c, Component::Normal(_))) {
        bail!("invalid relative auto source path: {rel}");
    }
    Ok(())
}

fn safe_output_path(input: &Path, target: &str, rel: &str) -> Result<PathBuf> {
    let path = input.join(format!("translated-{target}")).join(rel);
    // Refuse pre-existing output symlinks, including intermediate directories.
    let mut parent = input.to_path_buf();
    for component in path.strip_prefix(input)?.components() {
        parent.push(component.as_os_str());
        if let Ok(meta) = std::fs::symlink_metadata(&parent)
            && meta.file_type().is_symlink() {
            bail!("symlink in auto output path: {}", parent.display());
        }
    }
    Ok(path)
}

fn sibling(input: &Path, target: &str) -> PathBuf {
    let stem = input.file_stem().unwrap_or_default().to_string_lossy();
    match input.extension().and_then(|s| s.to_str()) {
        Some(ext) => input.with_file_name(format!("{stem}.{target}.{ext}")),
        None => input.with_file_name(format!("{stem}.{target}")),
    }
}

fn read_document(path: &Path) -> Result<Option<Document>> {
    read_document_bytes_for_path(path, std::fs::read(path).with_context(|| format!("read {}", path.display()))?)
}

fn scalar_hint(path: &Path) -> Option<Dialect> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "ini" | "cfg" | "conf" => Some(Dialect::Ini),
        "toml" => Some(Dialect::Toml),
        "yaml" | "yml" => Some(Dialect::Yaml),
        _ => None,
    }
}

fn copy_only(path: &Path) -> bool {
    let name = path.file_name().unwrap_or_default().to_string_lossy().to_ascii_lowercase();
    if translated_name(&name) { return true; }
    let extension = path.extension().and_then(|s| s.to_str()).unwrap_or_default().to_ascii_lowercase();
    // Code, executables, and delimited data are not safe scalar/prose inputs.
    matches!(extension.as_str(),
        "py" | "pyw" | "pyi" | "pyc" | "pyo" | "js" | "jsx" | "mjs" | "cjs" | "ts" | "tsx"
        | "rs" | "c" | "h" | "cc" | "cpp" | "cxx" | "hpp" | "cs" | "java" | "kt" | "kts"
        | "go" | "rb" | "php" | "phtml" | "lua" | "pl" | "pm" | "r" | "swift" | "scala"
        | "sc" | "dart" | "ex" | "exs" | "erl" | "hrl" | "hs" | "lhs" | "clj" | "cljs"
        | "lisp" | "lsp" | "scm" | "vb" | "vbs" | "sh" | "bash" | "zsh" | "fish" | "ps1"
        | "psm1" | "psd1" | "bat" | "cmd" | "sql" | "asm" | "s" | "vue" | "svelte"
        | "css" | "scss" | "sass" | "less" | "proto" | "jl" | "tcl" | "awk" | "sed"
        | "groovy" | "gradle" | "cmake" | "zig" | "nim" | "m" | "mm" | "f" | "f90"
        | "f95" | "for" | "pas" | "d" | "v" | "sv" | "vhd" | "vhdl"
        | "exe" | "com" | "dll" | "so" | "dylib" | "bin" | "o" | "obj" | "a" | "lib"
        | "wasm" | "class" | "jar" | "csv" | "tsv")
        || matches!(name.as_str(), "makefile" | "gnumakefile" | "dockerfile" | "rakefile" | "gemfile")
}

fn read_document_bytes_for_path(path: &Path, bytes: Vec<u8>) -> Result<Option<Document>> {
    if copy_only(path) { return Ok(None); }
    read_document_bytes_with_hint(bytes, scalar_hint(path))
}

#[cfg(test)]
fn read_document_bytes(bytes: Vec<u8>) -> Result<Option<Document>> {
    read_document_bytes_with_hint(bytes, None)
}

fn read_document_bytes_with_hint(bytes: Vec<u8>, hint: Option<Dialect>) -> Result<Option<Document>> {
    let utf16 = bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]);
    if bytes.starts_with(b"PK\x03\x04") || bytes.starts_with(b"\x7fELF") || bytes.starts_with(b"MZ")
        || bytes.starts_with(&[0x1f, 0x8b]) || bytes.starts_with(b"%PDF-")
        || (!utf16 && bytes.iter().any(|b| *b < 0x20 && !matches!(*b, b'\n' | b'\r' | b'\t'))) {
        return Ok(None);
    }
    let decoded = textio::decode_bytes(&bytes);
    if decoded.lossy || decoded.text.chars().any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t')) {
        return Ok(None);
    }
    // A guessed legacy encoding must be lossless in both directions.
    if decoded.encoding != "UTF-8"
        && encode(&decoded.text, decoded.encoding, &bytes).ok().as_deref() != Some(bytes.as_slice()) {
        return Ok(None);
    }
    let Some((format, spans)) = parse_document_with_hint(&decoded.text, hint) else { return Ok(None) };
    Ok(Some(Document { bytes, text: decoded.text, encoding: decoded.encoding, format, spans }))
}

fn encode(text: &str, encoding: &str, original: &[u8]) -> Result<Vec<u8>> {
    if encoding == "UTF-8" {
        let mut bytes = Vec::with_capacity(text.len() + 3);
        if original.starts_with(&[0xef, 0xbb, 0xbf]) { bytes.extend_from_slice(&[0xef, 0xbb, 0xbf]); }
        bytes.extend_from_slice(text.as_bytes());
        return Ok(bytes);
    }
    if encoding == "UTF-16LE" || encoding == "UTF-16BE" {
        let little = encoding == "UTF-16LE";
        let mut bytes = if little { vec![0xff, 0xfe] } else { vec![0xfe, 0xff] };
        for c in text.encode_utf16() {
            bytes.extend_from_slice(&if little { c.to_le_bytes() } else { c.to_be_bytes() });
        }
        return Ok(bytes);
    }
    let enc = encoding_rs::Encoding::for_label(encoding.as_bytes()).context("unknown source encoding")?;
    let (bytes, _, errors) = enc.encode(text);
    if errors { bail!("translation cannot be represented in source encoding {encoding}; convert the input to UTF-8 first"); }
    Ok(bytes.into_owned())
}

#[cfg(test)]
fn parse_document(text: &str) -> Option<(&'static str, Vec<Span>)> {
    parse_document_with_hint(text, None)
}

fn parse_document_with_hint(text: &str, hint: Option<Dialect>) -> Option<(&'static str, Vec<Span>)> {
    let trimmed = text.trim_start();
    if trimmed.starts_with("#!") { return None; }
    if serde_json::from_str::<serde::de::IgnoredAny>(text).is_ok() {
        return Some(("json", json_spans(text)?));
    }
    // Malformed structured documents must not fall through to prose.
    if trimmed.starts_with(['{', '"']) { return None; }
    if trimmed.starts_with('<') { return Some(("xml", xml_spans(text)?)); }
    if hint.is_none() && delimited_rows(text, b",\t") { return None; }
    if let Some(result) = scalar_spans(text, hint) { return Some(result); }
    if hint.is_some() || delimited_rows(text, b",\t;") { return None; }
    if trimmed.starts_with('[') { return None; }
    if text.lines().any(|line| separator(line).is_some()) { return None; }
    Some(("prose", prose_spans(text)?))
}

fn machine_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    let last = lower.rsplit(['.', ':', '/', '-']).next().unwrap_or(&lower);
    ["id", "uuid", "guid", "key", "type", "kind", "class", "nameid", "code", "script", "style",
        "path", "file", "filename", "directory", "dir", "url", "uri", "href", "src", "link", "email",
        "hash", "checksum", "token", "secret", "password", "regex", "pattern", "command", "format",
        "encoding", "locale", "language", "version", "date", "time", "color", "colour", "font",
        "resource", "asset", "ref", "identifier", "slug", "handle", "enum", "mime", "address"]
        .contains(&last) || last.ends_with("_id") || last.ends_with("_path") || last.ends_with("_url")
        || last.ends_with("_key") || last.ends_with("_file")
        || ["Id", "ID", "Path", "Url", "URL", "File", "Key", "Token", "Hash"].iter().any(|suffix| key.ends_with(suffix))
}

fn human_text(text: &str) -> bool {
    let t = text.trim();
    if t.is_empty() || t.chars().any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t')) { return false; }
    let lower = t.to_ascii_lowercase();
    if ["true", "false", "null", "none", "nil", "yes", "no", "on", "off", "nan", "inf"].contains(&lower.as_str())
        || t.parse::<f64>().is_ok() || t.contains("://") || t.starts_with("www.") || t.contains('@')
        || t.starts_with(['/', '\\', '.', '$', '%']) || t.contains('\\') || t.contains('/')
        || t.contains(['{', '}', '`', '<', '>']) || t.contains("${") || t.contains("{{")
        || t.contains("=>") || t.contains("==") {
        return false;
    }
    if t.len() >= 8 && t.chars().all(|c| c.is_ascii_hexdigit() || c == '-') { return false; }
    let single_word = t.split_whitespace().count() == 1;
    if single_word && t.is_ascii()
        && t.chars().any(|c| matches!(c, '_' | '-' | '.' | ':' | '=' | '+' | '[' | ']')) { return false; }
    if single_word && t.is_ascii() && t.chars().any(|c| c.is_ascii_digit()) { return false; }
    if single_word && t.is_ascii() {
        if t.chars().filter(|c| c.is_ascii_alphabetic()).count() > 1
            && t.chars().filter(|c| c.is_ascii_alphabetic()).all(|c| c.is_ascii_uppercase()) { return false; }
        if t.chars().zip(t.chars().skip(1)).any(|(a, b)| a.is_ascii_lowercase() && b.is_ascii_uppercase()) { return false; }
    }
    t.chars().any(char::is_alphabetic)
}

struct JsonScanner<'a> { source: &'a str, pos: usize, spans: Vec<Span> }
impl JsonScanner<'_> {
    fn ws(&mut self) { while self.source.as_bytes().get(self.pos).is_some_and(u8::is_ascii_whitespace) { self.pos += 1; } }
    fn string(&mut self) -> Option<(usize, usize, String)> {
        let start = self.pos;
        if self.source.as_bytes().get(start) != Some(&b'"') { return None; }
        self.pos += 1;
        while self.pos < self.source.len() {
            let b = self.source.as_bytes()[self.pos];
            self.pos += 1;
            if b == b'\\' { self.pos += 1; }
            else if b == b'"' {
                return Some((start, self.pos, serde_json::from_str(&self.source[start..self.pos]).ok()?));
            }
        }
        None
    }
    fn value(&mut self, blocked: bool, depth: usize) -> Option<()> {
        if depth > 128 { return None; }
        self.ws();
        match *self.source.as_bytes().get(self.pos)? {
            b'"' => {
                let (start, end, text) = self.string()?;
                if !blocked && human_text(&text) { self.spans.push(Span { start, end, text, style: Style::Json }); }
            }
            b'{' => {
                self.pos += 1; self.ws();
                if self.source.as_bytes().get(self.pos) == Some(&b'}') { self.pos += 1; return Some(()); }
                loop {
                    self.ws(); let (_, _, key) = self.string()?; self.ws(); self.pos += 1;
                    self.value(blocked || machine_key(&key), depth + 1)?; self.ws();
                    let b = *self.source.as_bytes().get(self.pos)?; self.pos += 1;
                    if b == b'}' { break; }
                    if b != b',' { return None; }
                }
            }
            b'[' => {
                self.pos += 1; self.ws();
                if self.source.as_bytes().get(self.pos) == Some(&b']') { self.pos += 1; return Some(()); }
                loop {
                    self.value(blocked, depth + 1)?; self.ws();
                    let b = *self.source.as_bytes().get(self.pos)?; self.pos += 1;
                    if b == b']' { break; }
                    if b != b',' { return None; }
                }
            }
            _ => {
                while self.source.as_bytes().get(self.pos).is_some_and(|b| !b.is_ascii_whitespace() && !matches!(b, b',' | b']' | b'}')) { self.pos += 1; }
            }
        }
        Some(())
    }
}
fn json_spans(text: &str) -> Option<Vec<Span>> {
    let mut parser = JsonScanner { source: text, pos: 0, spans: Vec::new() };
    parser.value(false, 0)?;
    Some(parser.spans)
}

fn trimmed_span(source: &str, start: usize, end: usize, style: Style, decoded: Option<String>) -> Option<Span> {
    let raw = &source[start..end];
    let left = raw.len() - raw.trim_start().len();
    let right = raw.trim_end().len();
    if left >= right { return None; }
    let text = decoded.unwrap_or_else(|| raw[left..right].to_string());
    if !human_text(&text) { return None; }
    Some(Span { start: start + left, end: start + right, text, style })
}

fn valid_xml_char(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r' | '\u{20}'..='\u{d7ff}' | '\u{e000}'..='\u{fffd}' | '\u{10000}'..='\u{10ffff}')
}
fn xml_entities(text: &str) -> Option<()> {
    if !text.chars().all(valid_xml_char) || text.contains("]]>") { return None; }
    let mut rest = text;
    while let Some(i) = rest.find('&') {
        rest = &rest[i + 1..];
        let end = rest.find(';')?;
        let entity = &rest[..end];
        if !["amp", "lt", "gt", "quot", "apos"].contains(&entity) {
            let number = if let Some(n) = entity.strip_prefix("#x") {
                if n.is_empty() || !n.bytes().all(|b| b.is_ascii_hexdigit()) { return None; }
                u32::from_str_radix(n, 16).ok()?
            } else {
                let n = entity.strip_prefix('#')?;
                if n.is_empty() || !n.bytes().all(|b| b.is_ascii_digit()) { return None; }
                n.parse::<u32>().ok()?
            };
            if !valid_xml_char(char::from_u32(number)?) { return None; }
        }
        rest = &rest[end + 1..];
    }
    Some(())
}
fn xml_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(xml_name_start)
        && chars.all(|c| xml_name_start(c) || matches!(c, '-' | '.' | '0'..='9' | '\u{b7}' | '\u{300}'..='\u{36f}' | '\u{203f}'..='\u{2040}'))
}

fn xml_name_start(c: char) -> bool {
    matches!(c, ':' | '_' | 'A'..='Z' | 'a'..='z' | '\u{c0}'..='\u{d6}' | '\u{d8}'..='\u{f6}'
        | '\u{f8}'..='\u{2ff}' | '\u{370}'..='\u{37d}' | '\u{37f}'..='\u{1fff}' | '\u{200c}'..='\u{200d}'
        | '\u{2070}'..='\u{218f}' | '\u{2c00}'..='\u{2fef}' | '\u{3001}'..='\u{d7ff}' | '\u{f900}'..='\u{fdcf}'
        | '\u{fdf0}'..='\u{fffd}' | '\u{10000}'..='\u{effff}')
}

fn xml_space(c: char) -> bool { matches!(c, ' ' | '\t' | '\r' | '\n') }
fn xml_tag(inner: &str) -> Option<&str> {
    let end = inner.find(xml_space).unwrap_or(inner.len());
    let name = &inner[..end];
    if !xml_name(name) { return None; }
    let mut rest = &inner[end..];
    let mut attrs = BTreeSet::new();
    while !rest.is_empty() {
        if !rest.starts_with(xml_space) { return None; }
        rest = rest.trim_start_matches(xml_space);
        if rest.is_empty() { break; }
        let end = rest.find(|c: char| xml_space(c) || c == '=')?;
        let attr = &rest[..end];
        if !xml_name(attr) || !attrs.insert(attr) { return None; }
        rest = rest[end..].trim_start_matches(xml_space).strip_prefix('=')?.trim_start_matches(xml_space);
        let quote = rest.chars().next()?;
        if !matches!(quote, '\'' | '"') { return None; }
        rest = &rest[1..];
        let end = rest.find(quote)?;
        if rest[..end].contains('<') { return None; }
        xml_entities(&rest[..end])?;
        rest = &rest[end + 1..];
    }
    Some(name)
}
fn xml_spans(source: &str) -> Option<Vec<Span>> {
    if !source.chars().all(valid_xml_char) { return None; }
    let mut pos = 0;
    let mut stack: Vec<(String, bool)> = Vec::new();
    let mut root = false;
    let mut spans = Vec::new();
    while pos < source.len() {
        let rest = &source[pos..];
        if !rest.starts_with('<') {
            let end = pos + rest.find('<').unwrap_or(rest.len());
            let text = &source[pos..end];
            xml_entities(text)?;
            if stack.is_empty() {
                if !text.chars().all(xml_space) { return None; }
            } else if !stack.last()?.1 {
                let trimmed = text.trim();
                if let Some(span) = trimmed_span(source, pos, end, Style::Xml, Some(xmllite::unescape(trimmed))) { spans.push(span); }
            }
            pos = end; continue;
        }
        if let Some(comment) = rest.strip_prefix("<!--") {
            let end = comment.find("-->")?;
            if comment[..end].contains("--") || comment[..end].ends_with('-') { return None; }
            pos += 4 + end + 3; continue;
        }
        if let Some(cdata) = rest.strip_prefix("<![CDATA[") {
            if stack.is_empty() { return None; }
            let end = cdata.find("]]>")?;
            if !cdata[..end].chars().all(valid_xml_char) { return None; }
            pos += 9 + end + 3; continue;
        }
        if let Some(pi) = rest.strip_prefix("<?") {
            let end = pi.find("?>")?;
            let target = pi[..end].split(xml_space).next()?;
            if !xml_name(target) { return None; }
            if target.eq_ignore_ascii_case("xml") {
                static DECLARATION: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
                    regex::Regex::new(r#"^xml[ \t\r\n]+version[ \t\r\n]*=[ \t\r\n]*(?:"1\.0"|'1\.0')(?:[ \t\r\n]+encoding[ \t\r\n]*=[ \t\r\n]*(?:"[A-Za-z][A-Za-z0-9._-]*"|'[A-Za-z][A-Za-z0-9._-]*'))?(?:[ \t\r\n]+standalone[ \t\r\n]*=[ \t\r\n]*(?:"(?:yes|no)"|'(?:yes|no)'))?[ \t\r\n]*$"#)
                        .expect("constant XML declaration regex")
                });
                if pos != 0 || !DECLARATION.is_match(&pi[..end]) { return None; }
            }
            pos += 2 + end + 2; continue;
        }
        if rest.starts_with("<!") { return None; } // DTDs and entity declarations need a real XML parser.
        let mut quote = None;
        let mut gt = None;
        for (offset, c) in rest.char_indices().skip(1) {
            match (quote, c) {
                (Some(q), c) if q == c => quote = None,
                (None, '\'' | '"') => quote = Some(c),
                (None, '>') => { gt = Some(offset); break; }
                (None, '<') => return None,
                _ => {}
            }
        }
        let end = gt?;
        let inner = &rest[1..end];
        if let Some(close) = inner.strip_prefix('/') {
            let name = close.trim_end_matches(xml_space);
            if !xml_name(name) || stack.pop()?.0 != name { return None; }
        } else {
            let self_closing = inner.ends_with('/');
            let name = xml_tag(if self_closing { &inner[..inner.len() - 1] } else { inner })?;
            if stack.is_empty() {
                if root { return None; }
                root = true;
            }
            let local = name.rsplit(':').next()?;
            let blocked = stack.last().is_some_and(|(_, b)| *b)
                || matches!(local.to_ascii_lowercase().as_str(), "script" | "style") || machine_key(local);
            if !self_closing {
                if stack.len() >= 128 { return None; }
                stack.push((name.to_string(), blocked));
            }
        }
        pos += end + 1;
    }
    if !root || !stack.is_empty() { return None; }
    Some(spans)
}

#[derive(Clone, Copy, PartialEq)]
enum Dialect { Ini, Toml, Yaml }

fn separator(line: &str) -> Option<(usize, u8)> {
    let mut quote = None;
    let mut escaped = false;
    for (i, b) in line.bytes().enumerate() {
        if escaped { escaped = false; continue; }
        if quote == Some(b'"') && b == b'\\' { escaped = true; continue; }
        match (quote, b) {
            (Some(q), c) if q == c => quote = None,
            (None, b'"' | b'\'') => quote = Some(b),
            (None, b'=' | b':') => return Some((i, b)),
            _ => {}
        }
    }
    None
}
fn scalar_spans(source: &str, hint: Option<Dialect>) -> Option<(&'static str, Vec<Span>)> {
    let mut delim = None;
    for line in source.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with(['#', ';', '[']) || matches!(t, "---" | "...") { continue; }
        let (index, sep) = separator(line)?;
        // A colon mapping needs the YAML separator boundary, not an arbitrary colon.
        if sep == b':' && line.as_bytes().get(index + 1).is_some_and(|b| !b.is_ascii_whitespace()) { return None; }
        if delim.is_some_and(|d| d != sep) { return None; }
        delim = Some(sep);
    }
    let dialect = match (delim?, hint) {
        (b':', None | Some(Dialect::Yaml)) => Dialect::Yaml,
        (b'=', Some(Dialect::Ini)) => Dialect::Ini,
        (b'=', Some(Dialect::Toml)) if toml::from_str::<toml::Value>(source).is_ok() => Dialect::Toml,
        (b'=', None) if scalar_data_evidence(source) => {
            if toml::from_str::<toml::Value>(source).is_ok() { Dialect::Toml } else { Dialect::Ini }
        }
        _ => return None,
    };
    let mut spans = Vec::new();
    let mut offset = 0;
    let mut section_blocked = false;
    let mut parents: Vec<(usize, bool)> = Vec::new();
    for raw in source.split_inclusive('\n') {
        let line = raw.trim_end_matches(['\r', '\n']);
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') || (dialect != Dialect::Yaml && t.starts_with(';'))
            || (dialect == Dialect::Yaml && matches!(t, "---" | "...")) {
            offset += raw.len(); continue;
        }
        if dialect != Dialect::Yaml && t.starts_with('[') {
            let close = t.find(']')?;
            let tail = t[close + 1..].trim();
            if !tail.is_empty() && !tail.starts_with(['#', ';']) { return None; }
            section_blocked = machine_key(t[1..close].trim());
            offset += raw.len(); continue;
        }
        let (sep, _) = separator(line)?;
        let key = line[..sep].trim();
        if key.is_empty() || !key.chars().all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.')) { return None; }
        let indent = line.len() - line.trim_start().len();
        if dialect == Dialect::Yaml && line[..indent].contains('\t') { return None; }
        while parents.last().is_some_and(|(n, _)| *n >= indent) { parents.pop(); }
        let blocked = section_blocked || machine_key(key) || parents.last().is_some_and(|(_, b)| *b);
        let value_start = sep + 1 + (line[sep + 1..].len() - line[sep + 1..].trim_start().len());
        let value = &line[value_start..];
        if dialect == Dialect::Yaml && (value.is_empty() || value.starts_with('#')) {
            parents.push((indent, blocked)); offset += raw.len(); continue;
        }
        let (length, text, style) = scalar_value(value, dialect)?;
        if hint.is_none() && dialect == Dialect::Ini && matches!(style, Style::BareIni)
            && text.contains(['(', ')', '[', ']', '{', '}', '`', '$']) { return None; }
        if !blocked && human_text(&text) {
            spans.push(Span { start: offset + value_start, end: offset + value_start + length, text, style });
        }
        offset += raw.len();
    }
    Some((match dialect { Dialect::Ini => "ini", Dialect::Toml => "toml", Dialect::Yaml => "yaml" }, spans))
}

fn scalar_data_evidence(source: &str) -> bool {
    // Quoted assignment alone is also valid Python/JavaScript. Require a table,
    // an INI comment, or spaced assignments with bare text / inline comments.
    if source.lines().any(|line| {
        let t = line.trim();
        t.starts_with('[') && t.contains(']') || t.starts_with(';')
    }) { return true; }
    let mut assignments = 0;
    for line in source.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') { continue; }
        let Some((sep, b'=')) = separator(line) else { return false; };
        if !line[..sep].ends_with(char::is_whitespace)
            || !line[sep + 1..].starts_with(char::is_whitespace) { return false; }
        let Some((length, value, style)) = scalar_value(line[sep + 1..].trim_start(), Dialect::Ini) else { return false; };
        let raw_value = line[sep + 1..].trim_start();
        let inline_comment = raw_value[length..].trim_start().starts_with(['#', ';']);
        if !inline_comment && (style != Style::BareIni || !human_text(&value) || value.split_whitespace().count() < 2) { return false; }
        assignments += 1;
    }
    assignments > 0
}

fn delimited_rows(source: &str, delimiters: &[u8]) -> bool {
    delimiters.iter().copied().any(|delimiter| {
        let mut width = None;
        let mut rows = 0;
        for line in source.lines().filter(|line| !line.trim().is_empty()) {
            let Some(columns) = delimited_width(line, delimiter) else { return false; };
            if columns < 2 || width.is_some_and(|w| w != columns) { return false; }
            width = Some(columns);
            rows += 1;
        }
        rows > 0
    })
}

fn delimited_width(row: &str, delimiter: u8) -> Option<usize> {
    let bytes = row.as_bytes();
    let mut pos = 0;
    let mut columns = 0;
    loop {
        columns += 1;
        if bytes.get(pos) == Some(&b'"') {
            pos += 1;
            loop {
                let b = *bytes.get(pos)?;
                pos += 1;
                if b == b'"' {
                    if bytes.get(pos) == Some(&b'"') { pos += 1; }
                    else { break; }
                }
            }
            if bytes.get(pos).is_some_and(|b| *b != delimiter) { return None; }
        } else {
            while let Some(&b) = bytes.get(pos) {
                if b == delimiter { break; }
                if b == b'"' { return None; }
                pos += 1;
            }
        }
        if pos == bytes.len() { return Some(columns); }
        pos += 1;
    }
}

fn scalar_value(value: &str, dialect: Dialect) -> Option<(usize, String, Style)> {
    if value.starts_with(['"', '\'']) {
        let quote = value.as_bytes()[0];
        if value.starts_with("\"\"\"") || value.starts_with("'''") { return None; }
        let mut pos = 1;
        let mut end = None;
        while pos < value.len() {
            let b = value.as_bytes()[pos];
            if quote == b'"' && b == b'\\' { pos += 2; continue; }
            if b == quote {
                if dialect == Dialect::Yaml && quote == b'\'' && value.as_bytes().get(pos + 1) == Some(&quote) {
                    pos += 2; continue;
                }
                end = Some(pos + 1); break;
            }
            pos += 1;
        }
        let end = end?;
        let tail = value[end..].trim();
        if !(tail.is_empty() || tail.starts_with('#') || dialect == Dialect::Ini && tail.starts_with(';')) { return None; }
        let literal = &value[..end];
        let text = if dialect == Dialect::Toml {
            toml::from_str::<toml::Value>(&format!("v = {literal}")).ok()?.get("v")?.as_str()?.to_string()
        } else if quote == b'"' { serde_json::from_str::<String>(literal).ok()? }
        else if dialect == Dialect::Yaml { literal[1..end - 1].replace("''", "'") }
        else { literal[1..end - 1].to_string() };
        let style = if quote == b'"' { Style::Double }
            else if dialect == Dialect::Yaml { Style::SingleYaml } else { Style::SingleLiteral };
        return Some((end, text, style));
    }
    let comment = value.char_indices().find_map(|(i, c)| {
        let marker = c == '#' || (dialect == Dialect::Ini && c == ';');
        if marker && (dialect == Dialect::Ini || i == 0 || value[..i].ends_with(char::is_whitespace)) { Some(i) } else { None }
    }).unwrap_or(value.len());
    let text = value[..comment].trim_end();
    if dialect == Dialect::Yaml && (text.starts_with(['[', '{', '|', '>', '&', '*', '!', '@', '`'])
        || text.contains(": ") || text.contains(" #")) { return None; }
    // Typed TOML values are preserved but never extracted as strings.
    if dialect == Dialect::Toml { return Some((text.len(), String::new(), Style::BareIni)); }
    Some((text.len(), text.to_string(), if dialect == Dialect::Yaml { Style::BareYaml } else { Style::BareIni }))
}

fn prose_spans(source: &str) -> Option<Vec<Span>> {
    let mut spans = Vec::new();
    let mut offset = 0;
    for raw in source.split_inclusive('\n') {
        let line = raw.trim_end_matches(['\r', '\n']);
        let text = line.trim();
        if !text.is_empty() {
            let lower = text.to_ascii_lowercase();
            if !human_text(text) || text.contains(['=', '<', '>', ';', '[', ']', '#', '*', '|', '$', '(', ')'])
                || ["fn ", "function ", "def ", "class ", "import ", "from ", "use ", "return ", "let ", "const ",
                    "var ", "if ", "else", "while ", "for ", "print(", "console.", "//", "/*", "--", "#!",
                    "echo ", "printf ", "select ", "insert ", "update ", "delete ", "write-host ", "export ", "set ", "call ", "rem "]
                    .iter().any(|prefix| lower.starts_with(prefix)) { return None; }
            let letters = text.chars().filter(|c| c.is_alphabetic()).count();
            let words = text.split_whitespace().count();
            let cjk = text.chars().any(|c| matches!(c, '\u{3040}'..='\u{30ff}' | '\u{3400}'..='\u{9fff}' | '\u{ac00}'..='\u{d7af}'));
            let confident = if cjk { letters >= 4 && (text.contains(['。', '！', '？', '!', '?']) || letters >= 12) }
                else { letters >= 10 && (words >= 6 || (words >= 3 && text.ends_with(['.', '!', '?']))) };
            if !confident { return None; }
            if let Some(span) = trimmed_span(source, offset, offset + line.len(), Style::Prose, None) { spans.push(span); }
        }
        offset += raw.len();
    }
    if spans.is_empty() { return None; }
    Some(spans)
}

fn render(style: Style, text: &str) -> Result<String> {
    if text.chars().any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t')) {
        bail!("translation contains unsupported control characters");
    }
    Ok(match style {
        Style::Json | Style::Double => serde_json::to_string(text)?,
        Style::Xml => {
            if !text.chars().all(valid_xml_char) { bail!("translation contains invalid XML characters"); }
            xmllite::escape(text)
        }
        Style::SingleYaml => {
            if text.contains(['\n', '\r', '\t']) { serde_json::to_string(text)? }
            else { format!("'{}'", text.replace('\'', "''")) }
        }
        Style::SingleLiteral => {
            if text.contains(['\'', '\n', '\r']) { serde_json::to_string(text)? }
            else { format!("'{text}'") }
        }
        Style::BareYaml => serde_json::to_string(text)?,
        Style::BareIni => {
            if text.contains(['\n', '\r', '\\', '#', ';', '"', '\'']) || text.trim() != text { serde_json::to_string(text)? }
            else { text.to_string() }
        }
        Style::Prose => {
            if text.contains(['\n', '\r']) { bail!("auto prose translations must preserve line boundaries"); }
            text.to_string()
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    struct Fixture(PathBuf);
    impl Fixture {
        fn new(body: &[u8]) -> (Self, PathBuf) {
            let dir = std::env::temp_dir().join(format!("attx-auto-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
            std::fs::create_dir_all(&dir).unwrap();
            let file = dir.join("source.mystery");
            std::fs::write(&file, body).unwrap();
            (Self(dir), file)
        }
    }
    impl Drop for Fixture { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); } }
    fn translate(units: &[TextUnit], replacement: &str) -> BTreeMap<String, Translation> {
        units.iter().map(|unit| (unit.id.clone(), Translation {
            unit_id: unit.id.clone(), translation_lines: vec![replacement.into()],
            source_hash: TextUnit::source_hash(&unit.original_lines), passthrough: false,
        })).collect()
    }

    #[test]
    fn unknown_json_keeps_machine_values_and_exact_empty_roundtrip() {
        let raw = br#"{ "title": "Welcome traveler", "id":"human_identifier", "path":"assets/title.txt", "url":"https://example.org", "nested":["Hello world", {"text":"Good morning", "type":"Friendly greeting"}], "secret":{"text":"Do not translate"} }"#;
        let (_fixture, file) = Fixture::new(raw);
        assert!(AutoAdapter.detect(&file).is_some());
        let units = AutoAdapter.extract(&file, "en").unwrap();
        assert_eq!(units.len(), 3);
        let empty = AutoAdapter.writeback(&file, "zh", &units, &BTreeMap::new()).unwrap();
        assert_eq!(empty[0].bytes, raw);
        let output = AutoAdapter.writeback(&file, "zh", &units, &translate(&units, "你好\"朋友\n欢迎")).unwrap();
        let json: serde_json::Value = serde_json::from_slice(&output[0].bytes).unwrap();
        assert_eq!(json["id"], "human_identifier");
        assert_eq!(json["nested"][1]["type"], "Friendly greeting");
        assert_eq!(json["title"], "你好\"朋友\n欢迎");
        assert_eq!(json["secret"]["text"], "Do not translate");
    }

    #[test]
    fn xml_escapes_text_and_preserves_markup_comments_controls() {
        let raw = br#"<?xml version="1.0"?><root attr="keep"><p> Hello &amp; goodbye </p><!-- untouched --><script>say Hello</script><style>Friendly words</style><path>Human looking path</path><x><![CDATA[Keep this text]]></x></root>"#;
        let (_fixture, file) = Fixture::new(raw);
        let units = AutoAdapter.extract(&file, "en").unwrap();
        assert_eq!(units.len(), 1);
        assert_eq!(units[0].joined_text(), "Hello & goodbye");
        assert_eq!(AutoAdapter.writeback(&file, "zh", &units, &BTreeMap::new()).unwrap()[0].bytes, raw);
        let out = AutoAdapter.writeback(&file, "zh", &units, &translate(&units, "朋友<&>欢迎")).unwrap();
        let text = String::from_utf8(out[0].bytes.clone()).unwrap();
        assert!(text.contains("<p> 朋友&lt;&amp;&gt;欢迎 </p>"));
        assert!(text.contains("<!-- untouched --><script>say Hello</script>"));
        for bad in ["<r><p>Hello world</r>", "<r>Hello &unknown;</r>", "<r a=unquoted>Hello world</r>", "<r>Hello world</r><x/>"] {
            assert!(parse_document(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn scalar_formats_preserve_quotes_comments_and_crlf() {
        for (raw, count, fragment) in [
            ("[messages]\r\ngreeting = Hello traveler ; leave comment\r\npath = assets/file.txt\r\n", 1, "greeting = 欢迎朋友 ; leave comment\r\n"),
            ("messages:\r\n  greeting: 'Hello traveler' # leave comment\r\n  label: \"Good morning\"\r\npath: assets/file.txt\r\n", 2, "greeting: '欢迎朋友' # leave comment\r\n"),
            ("[messages]\r\ngreeting = \"Hello traveler\" # leave comment\r\nliteral = 'Good morning'\r\ncount = 10\r\n", 2, "greeting = \"欢迎朋友\" # leave comment\r\n"),
        ] {
            let (_fixture, file) = Fixture::new(raw.as_bytes());
            let units = AutoAdapter.extract(&file, "en").unwrap();
            assert_eq!(units.len(), count, "{raw}");
            assert_eq!(AutoAdapter.writeback(&file, "zh", &units, &BTreeMap::new()).unwrap()[0].bytes, raw.as_bytes());
            let out = AutoAdapter.writeback(&file, "zh", &units, &translate(&units, "欢迎朋友")).unwrap();
            let text = String::from_utf8(out[0].bytes.clone()).unwrap();
            assert!(text.contains(fragment), "{text}");
            assert_eq!(text.matches('\n').count(), text.matches("\r\n").count());
        }
    }

    #[test]
    fn scalar_escaping_keeps_valid_string_values() {
        for raw in ["message: 'Hello traveler' # c\n", "message = 'Hello traveler' # c\n", "message = Hello traveler ; c\n", "message: Hello traveler # c\n"] {
            let (_fixture, file) = Fixture::new(raw.as_bytes());
            let units = AutoAdapter.extract(&file, "en").unwrap();
            let replacement = "It's a \"fine\" day # really";
            let out = AutoAdapter.writeback(&file, "en", &units, &translate(&units, replacement)).unwrap();
            let doc = read_document_bytes(out[0].bytes.clone()).unwrap().unwrap();
            assert_eq!(doc.spans[0].text, replacement);
        }
    }

    #[test]
    fn scalar_newlines_use_escaped_quotes_instead_of_breaking_structure() {
        for raw in ["message: 'Hello traveler' # c\r\n", "message = 'Hello traveler' # c\r\n"] {
            let (_fixture, file) = Fixture::new(raw.as_bytes());
            let units = AutoAdapter.extract(&file, "en").unwrap();
            let out = AutoAdapter.writeback(&file, "zh", &units, &translate(&units, "欢迎\n朋友")).unwrap();
            let doc = read_document_bytes(out[0].bytes.clone()).unwrap().unwrap();
            assert_eq!(doc.spans[0].text, "欢迎\n朋友");
            assert!(out[0].bytes.ends_with(b"# c\r\n"));
        }
    }

    #[test]
    fn changed_source_and_tampered_anchor_are_refused() {
        let (_fixture, file) = Fixture::new(br#"{"message":"Hello traveler"}"#);
        let mut units = AutoAdapter.extract(&file, "en").unwrap();
        units[0].original_lines[0] = "Different source".into();
        assert!(AutoAdapter.writeback(&file, "zh", &units, &BTreeMap::new()).is_err());
        let units = AutoAdapter.extract(&file, "en").unwrap();
        std::fs::write(&file, br#"{ "message":"Hello traveler"}"#).unwrap();
        assert!(AutoAdapter.writeback(&file, "zh", &units, &BTreeMap::new()).is_err());
    }

    #[test]
    fn binary_code_and_unsupported_yaml_are_not_prose() {
        for raw in [b"\x00Hello traveler".as_slice(), b"\x7fELFHello traveler", b"PK\x03\x04Hello traveler", b"fn main() { println!(\"Hello world\"); }", b"const text = \"Hello traveler\";", b"message: |\n  This is a longer prose paragraph.\n", b"message: &anchor Hello traveler\n", b"[broken json containing English words"] {
            let (_fixture, file) = Fixture::new(raw);
            assert!(AutoAdapter.detect(&file).is_none(), "{raw:?}");
        }
        let (_fixture, file) = Fixture::new(b"The traveler crossed the quiet valley.\r\nA new adventure was about to begin.\r\n");
        let units = AutoAdapter.extract(&file, "en").unwrap();
        assert_eq!(units.len(), 2);
        assert_eq!(AutoAdapter.writeback(&file, "zh", &units, &BTreeMap::new()).unwrap()[0].bytes, std::fs::read(&file).unwrap());
        let translated = AutoAdapter.writeback(&file, "zh", &units, &translate(&units, "欢迎朋友")).unwrap();
        assert_eq!(translated[0].bytes, "欢迎朋友\r\n欢迎朋友\r\n".as_bytes());
    }

    #[test]
    fn directory_mixes_formats_copies_binary_and_isolates_output() {
        let (fixture, file) = Fixture::new(br#"{"message":"Hello traveler"}"#);
        std::fs::create_dir_all(fixture.0.join("sub")).unwrap();
        std::fs::write(fixture.0.join("sub/config.odd"), b"message: 'Good morning'\n").unwrap();
        std::fs::write(fixture.0.join("sub/data.bin"), b"\x00\xff\x01").unwrap();
        for dir in [".git", ".attx-work", "node_modules", "translated-ja", "backups"] {
            std::fs::create_dir_all(fixture.0.join(dir)).unwrap();
            std::fs::write(fixture.0.join(dir).join("ignored.json"), br#"{"message":"Never translate this"}"#).unwrap();
        }
        std::fs::write(fixture.0.join("old.zh.odd"), br#"{"message":"Already translated"}"#).unwrap();
        std::fs::write(fixture.0.join("old.odd.bak"), br#"{"message":"Backup text"}"#).unwrap();
        let units = AutoAdapter.extract(&fixture.0, "en").unwrap();
        assert_eq!(units.len(), 2);
        assert_ne!(units[0].location, units[1].location);
        let out = AutoAdapter.writeback(&fixture.0, "zh", &units, &translate(&units, "欢迎朋友")).unwrap();
        assert_eq!(out.len(), 4);
        assert!(out.iter().all(|o| o.path.starts_with(fixture.0.join("translated-zh"))));
        assert!(out.iter().any(|o| o.path.ends_with("sub/data.bin") && o.bytes == b"\x00\xff\x01"));
        assert_eq!(std::fs::read(file).unwrap(), br#"{"message":"Hello traveler"}"#);
        let report = coverage(&fixture.0).unwrap();
        assert_eq!(report.supported_files, 2);
        assert_eq!(report.copied_files, 2);
        assert_eq!(report.unsupported_total, 2);
        assert_eq!(report.unsupported_paths, ["old.zh.odd", "sub/data.bin"]);
        assert_eq!(report.excluded_entries, 6);
        assert_eq!(report.excluded_paths.len(), 6);
    }

    #[test]
    fn language_suffixed_sources_are_copied_not_discarded() {
        let (fixture, _file) = Fixture::new(br#"{"message":"Hello traveler"}"#);
        for (name, body) in [
            ("story.en.txt", "The traveler crossed the quiet valley.\n"),
            ("dialogue.ja.json", "{\"message\":\"こんにちは\"}"),
            ("notes.en", "The traveler crossed the quiet valley.\n"),
        ] {
            std::fs::write(fixture.0.join(name), body).unwrap();
        }
        let units = AutoAdapter.extract(&fixture.0, "en").unwrap();
        assert_eq!(units.len(), 1);
        let outputs = AutoAdapter.writeback(&fixture.0, "zh", &units, &translate(&units, "欢迎朋友")).unwrap();
        assert_eq!(outputs.len(), 4);
        for name in ["story.en.txt", "dialogue.ja.json", "notes.en"] {
            let output = outputs.iter().find(|o| o.path.ends_with(name)).unwrap();
            assert_eq!(output.bytes, std::fs::read(fixture.0.join(name)).unwrap());
        }
        let report = coverage(&fixture.0).unwrap();
        assert_eq!(report.supported_files, 1);
        assert_eq!(report.copied_files, 3);
        assert_eq!(report.excluded_entries, 0);
    }

    #[test]
    fn source_code_and_sniffed_assignments_are_copied_unchanged() {
        let (fixture, _file) = Fixture::new(br#"{"message":"Hello traveler"}"#);
        for (name, body) in [
            ("config.py", "state = \"こんにちは\"\n"),
            ("config.js", "state = \"こんにちは\"\n"),
            ("config.ts", "message: こんにちは\n"),
            ("fragment.dat", "variable=\"こんにちは\"\n"),
            ("spaced.dat", "variable = \"こんにちは\"\n"),
            ("fragment.unknown", "state = 'こんにちは'\n"),
            ("script.unknown", "#!/usr/bin/env python\nstate = \"こんにちは\"\n"),
        ] {
            let file = fixture.0.join(name);
            std::fs::write(&file, body).unwrap();
            assert!(AutoAdapter.detect(&file).is_none(), "{name}");
        }
        let units = AutoAdapter.extract(&fixture.0, "en").unwrap();
        assert_eq!(units.len(), 1);
        let outputs = AutoAdapter.writeback(&fixture.0, "zh", &units, &translate(&units, "欢迎朋友")).unwrap();
        assert_eq!(outputs.len(), 8);
        for output in outputs.iter().filter(|o| !o.path.ends_with("source.mystery")) {
            assert_eq!(output.bytes, std::fs::read(fixture.0.join(output.path.file_name().unwrap())).unwrap());
        }
    }

    #[test]
    fn unknown_scalars_need_data_evidence_known_suffixes_authorize_subset() {
        let (fixture, _file) = Fixture::new(b"\x00");
        for (name, body) in [
            ("config.dat", "[messages]\nmessage = \"こんにちは\"\n"),
            ("config.ini", "message=\"こんにちは\"\n"),
            ("config.toml", "message=\"こんにちは\"\n"),
            ("config.yaml", "message: こんにちは\n"),
            ("config.odd", "message: こんにちは\n"),
        ] {
            let file = fixture.0.join(name);
            std::fs::write(&file, body).unwrap();
            let units = AutoAdapter.extract(&file, "ja").unwrap();
            assert_eq!(units.len(), 1, "{name}");
            let outputs = AutoAdapter.writeback(&file, "zh", &units, &translate(&units, "欢迎朋友")).unwrap();
            assert!(String::from_utf8(outputs[0].bytes.clone()).unwrap().contains("欢迎朋友"));
        }
    }

    #[test]
    fn csv_tsv_and_content_sniffed_rows_are_copied_unchanged() {
        let (fixture, _file) = Fixture::new(br#"{"message":"Hello traveler"}"#);
        for (name, body) in [
            ("dialogue.csv", "こんにちは、今日はいい天気ですね。,さようなら、また明日会いましょう。\n"),
            ("dialogue.tsv", "こんにちは、今日はいい天気ですね。\tさようなら、また明日会いましょう。\n"),
            ("rows.dat", "こんにちは、今日はいい天気ですね。,さようなら、また明日会いましょう。\n"),
            ("rows.odd", "The traveler crossed the quiet valley.,A new adventure was about to begin.\nThe traveler returned to the village.,The sun set over the distant mountains.\n"),
            ("quoted.dat", "\"Hello, traveler\",\"Good morning\"\n\"Hello again\",\"Good evening\"\n"),
        ] {
            let file = fixture.0.join(name);
            std::fs::write(&file, body).unwrap();
            assert!(AutoAdapter.detect(&file).is_none(), "{name}");
        }
        let units = AutoAdapter.extract(&fixture.0, "en").unwrap();
        assert_eq!(units.len(), 1);
        let outputs = AutoAdapter.writeback(&fixture.0, "zh", &units, &translate(&units, "欢迎朋友")).unwrap();
        assert_eq!(outputs.len(), 6);
        for output in outputs.iter().filter(|o| !o.path.ends_with("source.mystery")) {
            assert_eq!(output.bytes, std::fs::read(fixture.0.join(output.path.file_name().unwrap())).unwrap());
        }
        assert_eq!(coverage(&fixture.0).unwrap().copied_files, 5);
    }

    #[cfg(unix)]
    #[test]
    fn translated_and_copied_outputs_carry_source_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let (fixture, file) = Fixture::new(br#"{"message":"Hello traveler"}"#);
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
        for (name, body, mode) in [
            ("private.bin", b"\x00\x01".as_slice(), 0o600),
            ("run.sh", b"message=\"Hello traveler\"\n".as_slice(), 0o750),
        ] {
            let path = fixture.0.join(name);
            std::fs::write(&path, body).unwrap();
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
        }
        let units = AutoAdapter.extract(&fixture.0, "en").unwrap();
        let outputs = AutoAdapter.writeback(&fixture.0, "zh", &units, &translate(&units, "欢迎朋友")).unwrap();
        assert_eq!(outputs.len(), 3);
        for output in &outputs {
            let source = fixture.0.join(output.path.file_name().unwrap());
            assert_eq!(output.permissions.as_ref().unwrap().mode() & 0o7777,
                std::fs::metadata(source).unwrap().permissions().mode() & 0o7777);
        }
        let single = AutoAdapter.writeback(&file, "zh", &AutoAdapter.extract(&file, "en").unwrap(), &BTreeMap::new()).unwrap();
        assert_eq!(single[0].permissions.as_ref().unwrap().mode() & 0o7777, 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_not_read_or_used_for_output() {
        use std::os::unix::fs::symlink;
        let (fixture, _file) = Fixture::new(br#"{"message":"Hello traveler"}"#);
        let (outside, outside_file) = Fixture::new(br#"{"message":"Outside secret text"}"#);
        symlink(&outside_file, fixture.0.join("linked.json")).unwrap();
        symlink(&outside.0, fixture.0.join("external")).unwrap();
        let units = AutoAdapter.extract(&fixture.0, "en").unwrap();
        assert_eq!(units.len(), 1);
        let report = coverage(&fixture.0).unwrap();
        assert_eq!(report.supported_files, 1);
        assert_eq!(report.copied_files, 0);
        assert_eq!(report.excluded_entries, 2);
        assert_eq!(report.excluded_paths, ["external", "linked.json"]);
        symlink(&outside.0, fixture.0.join("translated-zh")).unwrap();
        assert!(AutoAdapter.writeback(&fixture.0, "zh", &units, &BTreeMap::new()).is_err());
        assert!(AutoAdapter.extract(&fixture.0.join("linked.json"), "en").is_err());
    }

    #[test]
    fn coverage_caps_samples_and_empty_text_does_not_detect() {
        let (fixture, file) = Fixture::new(br#"{"id":"friendly_identifier"}"#);
        assert!(AutoAdapter.detect(&file).is_none());
        for i in 0..60 {
            std::fs::write(fixture.0.join(format!("binary-{i:02}.dat")), b"\x00\x01").unwrap();
        }
        let report = coverage(&fixture.0).unwrap();
        assert_eq!(report.supported_files, 0);
        assert_eq!(report.copied_files, 61);
        assert_eq!(report.unsupported_total, 61);
        assert_eq!(report.unsupported_paths.len(), 50);
        assert!(AutoAdapter.detect(&fixture.0).is_none());
    }

    #[test]
    fn utf16_bom_is_preserved_without_lossy_binary_guessing() {
        let mut raw = vec![0xff, 0xfe];
        for c in "{\"message\":\"Hello traveler\"}".encode_utf16() { raw.extend_from_slice(&c.to_le_bytes()); }
        let (_fixture, file) = Fixture::new(&raw);
        let units = AutoAdapter.extract(&file, "en").unwrap();
        assert_eq!(units.len(), 1);
        assert_eq!(AutoAdapter.writeback(&file, "zh", &units, &BTreeMap::new()).unwrap()[0].bytes, raw);
        let out = AutoAdapter.writeback(&file, "zh", &units, &translate(&units, "欢迎朋友")).unwrap();
        assert!(out[0].bytes.starts_with(&[0xff, 0xfe]));
        assert_eq!(read_document_bytes(out[0].bytes.clone()).unwrap().unwrap().spans[0].text, "欢迎朋友");
    }
}
