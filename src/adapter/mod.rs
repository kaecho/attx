//! Format adapter registry.
//!
//! Every supported input — game engine directory, ebook, document, subtitle,
//! or localization file — is handled by one [`FormatAdapter`]. Adapters are
//! pure I/O + structure logic: they never talk to the network. The pipeline
//! owns batching, LLM calls, caching, and disk writes.
//!
//! To add a new format: create `src/adapter/<name>.rs`, implement
//! [`FormatAdapter`], and register it in [`all_adapters`]. See README
//! "Contributing" for the full checklist.

pub mod docx;
pub mod epub;
pub mod jsonkv;
pub mod jsonl;
pub mod plaintext;
pub mod po;
pub mod renpy;
pub mod rmmz;
pub mod rmmz_plugins;
pub mod subtitle;
pub mod xmllite;

use crate::model::{TextUnit, Translation};
use anyhow::{Result, bail};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct DetectHit {
    pub engine_id: &'static str,
    pub label: &'static str,
    /// Canonical input path: a directory (game engines) or a single file
    /// (document / subtitle / localization formats).
    pub content_root: PathBuf,
}

/// One artifact produced by writeback. `path` is absolute; existing files are
/// backed up once as `*.attxbak` by the pipeline before being overwritten.
pub struct OutputFile {
    pub path: PathBuf,
    pub bytes: Vec<u8>,
}

impl OutputFile {
    pub fn text(path: PathBuf, body: String) -> Self {
        Self {
            path,
            bytes: body.into_bytes(),
        }
    }
}

/// A translatable file format or game engine.
pub trait FormatAdapter: Send + Sync {
    /// Stable id used in CLI `--engine`, workspace meta, and unit records.
    fn id(&self) -> &'static str;
    fn label(&self) -> &'static str;
    /// Lowercase file extensions claimed by this adapter (empty → directory input).
    fn extensions(&self) -> &'static [&'static str] {
        &[]
    }
    /// "file", "directory" or "file|directory" — surfaced by `attx formats`.
    fn input_kind(&self) -> &'static str {
        if self.extensions().is_empty() {
            "directory"
        } else {
            "file"
        }
    }
    /// Probe the input; return a hit to claim it. Default: extension match.
    fn detect(&self, input: &Path) -> Option<DetectHit> {
        detect_by_extension(self.id(), self.label(), self.extensions(), input)
    }
    /// Parse the input into engine-agnostic text units. Only units whose text
    /// matches the source language should be emitted.
    fn extract(&self, input: &Path, source_lang: &str) -> Result<Vec<TextUnit>>;
    /// Render output files with translations applied. Untranslated units must
    /// keep their original text. Document formats write a translated sibling
    /// (`<stem>.<target_lang>.<ext>`); game engines overwrite in place.
    fn writeback(
        &self,
        input: &Path,
        target_lang: &str,
        units: &[TextUnit],
        translations: &BTreeMap<String, Translation>,
    ) -> Result<Vec<OutputFile>>;
}

/// Registry, in detect priority order. Directory engines first, then
/// unambiguous file extensions, then `.json` content-sniffers (most specific
/// shape first).
pub fn all_adapters() -> Vec<Box<dyn FormatAdapter>> {
    vec![
        Box::new(rmmz::RmmzAdapter),
        Box::new(epub::EpubAdapter),
        Box::new(docx::DocxAdapter),
        Box::new(subtitle::SrtAdapter),
        Box::new(subtitle::VttAdapter),
        Box::new(subtitle::LrcAdapter),
        Box::new(po::PoAdapter),
        Box::new(renpy::RenpyAdapter),
        Box::new(plaintext::MdAdapter),
        Box::new(plaintext::TxtAdapter),
        Box::new(jsonkv::ParatranzAdapter),
        Box::new(jsonkv::VntAdapter),
        Box::new(jsonkv::MtoolAdapter),
        Box::new(jsonkv::I18nextAdapter),
        Box::new(jsonl::JsonlAdapter),
    ]
}

pub fn detect(input: &Path) -> Result<DetectHit> {
    let input = input.canonicalize().unwrap_or_else(|_| input.to_path_buf());
    for a in all_adapters() {
        if let Some(hit) = a.detect(&input) {
            return Ok(hit);
        }
    }
    let ids: Vec<&str> = all_adapters().iter().map(|a| a.id()).collect();
    bail!(
        "no format adapter matched {}. Supported: {}. Pass --engine to force.",
        input.display(),
        ids.join(", ")
    )
}

pub fn get(engine_id: &str) -> Result<Box<dyn FormatAdapter>> {
    for a in all_adapters() {
        if a.id() == engine_id {
            return Ok(a);
        }
    }
    bail!("unknown format adapter: {engine_id}")
}

pub fn detect_or_force(input: &Path, engine: Option<&str>) -> Result<DetectHit> {
    if let Some(id) = engine {
        let a = get(id)?;
        if let Some(hit) = a.detect(input) {
            return Ok(hit);
        }
        // force even if detect soft-fails (e.g. partial tree / ambiguous json)
        return Ok(DetectHit {
            engine_id: a.id(),
            label: a.label(),
            content_root: input.canonicalize().unwrap_or_else(|_| input.to_path_buf()),
        });
    }
    detect(input)
}

/// Extension-based detect used by most single-file adapters.
pub fn detect_by_extension(
    id: &'static str,
    label: &'static str,
    extensions: &[&str],
    input: &Path,
) -> Option<DetectHit> {
    if !input.is_file() {
        return None;
    }
    let ext = input.extension()?.to_str()?.to_ascii_lowercase();
    if extensions.iter().any(|e| *e == ext) {
        return Some(DetectHit {
            engine_id: id,
            label,
            content_root: input.canonicalize().unwrap_or_else(|_| input.to_path_buf()),
        });
    }
    None
}

/// `book.epub` + target `zh` → `book.zh.epub` beside the input.
pub fn output_sibling(input: &Path, target_lang: &str, ext: &str) -> PathBuf {
    let stem = input
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("output");
    input.with_file_name(format!("{stem}.{target_lang}.{ext}"))
}

/// Shared helper: set JSON value at slash path with numeric segments for arrays.
pub fn set_json_path(root: &mut Value, path: &str, value: Value) -> Result<()> {
    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    if parts.is_empty() {
        bail!("empty json path");
    }
    let mut cur = root;
    for (i, part) in parts.iter().enumerate() {
        let last = i + 1 == parts.len();
        if last {
            if let Ok(idx) = part.parse::<usize>() {
                let arr = cur
                    .as_array_mut()
                    .ok_or_else(|| anyhow::anyhow!("expected array at {path}"))?;
                if idx >= arr.len() {
                    bail!("index {idx} out of range at {path}");
                }
                arr[idx] = value;
                return Ok(());
            } else {
                let obj = cur
                    .as_object_mut()
                    .ok_or_else(|| anyhow::anyhow!("expected object at {path}"))?;
                obj.insert((*part).to_string(), value);
                return Ok(());
            }
        }
        if let Ok(idx) = part.parse::<usize>() {
            let arr = cur
                .as_array_mut()
                .ok_or_else(|| anyhow::anyhow!("expected array navigating {path}"))?;
            cur = arr
                .get_mut(idx)
                .ok_or_else(|| anyhow::anyhow!("missing index {idx} in {path}"))?;
        } else {
            let obj = cur
                .as_object_mut()
                .ok_or_else(|| anyhow::anyhow!("expected object navigating {path}"))?;
            cur = obj
                .get_mut(*part)
                .ok_or_else(|| anyhow::anyhow!("missing key {part} in {path}"))?;
        }
    }
    unreachable!()
}

#[allow(dead_code)] // util pair with set_json_path; kept for adapter authors
pub fn get_json_path<'a>(root: &'a Value, path: &str) -> Option<&'a Value> {
    let mut cur = root;
    for part in path.split('/').filter(|p| !p.is_empty()) {
        if let Ok(idx) = part.parse::<usize>() {
            cur = cur.as_array()?.get(idx)?;
        } else {
            cur = cur.get(part)?;
        }
    }
    Some(cur)
}

#[cfg(test)]
pub(crate) fn test_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("attx-test-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&d).expect("create test dir");
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_sibling_naming() {
        let p = output_sibling(Path::new("/tmp/book v1.epub"), "zh", "epub");
        assert_eq!(p, Path::new("/tmp/book v1.zh.epub"));
    }

    #[test]
    fn registry_ids_unique() {
        let mut ids: Vec<&str> = all_adapters().iter().map(|a| a.id()).collect();
        let n = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(n, ids.len(), "duplicate adapter id registered");
    }
}
