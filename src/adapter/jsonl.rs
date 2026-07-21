//! Generic JSONL text-pack adapter — the universal escape hatch for engines
//! without a native adapter: extract to JSONL with any external tool, translate
//! here, write back with your own script.
//!
//! Input: a `.jsonl` file, or a directory containing `source.jsonl`.
//! Line format: `{"id","text","context"?,"role"?,"item_type"?}`.

use super::{DetectHit, FormatAdapter, OutputFile, output_sibling};
use crate::model::{ItemType, TextUnit, Translation};
use anyhow::{Context, Result, bail};
use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

pub struct JsonlAdapter;

impl JsonlAdapter {
    /// The actual jsonl file for a file-or-directory input.
    fn source_file(input: &Path) -> Result<PathBuf> {
        if input.is_file() {
            return Ok(input.to_path_buf());
        }
        let candidate = input.join("source.jsonl");
        if candidate.is_file() {
            return Ok(candidate);
        }
        bail!(
            "jsonl adapter expects a .jsonl file or source.jsonl under {}",
            input.display()
        );
    }
}

impl FormatAdapter for JsonlAdapter {
    fn id(&self) -> &'static str {
        "jsonl"
    }
    fn label(&self) -> &'static str {
        "Generic JSONL text pack"
    }
    fn extensions(&self) -> &'static [&'static str] {
        &["jsonl"]
    }
    fn input_kind(&self) -> &'static str {
        "file|directory"
    }

    fn detect(&self, input: &Path) -> Option<DetectHit> {
        let is_hit = (input.is_file()
            && input.extension().and_then(|e| e.to_str()) == Some("jsonl"))
            || (input.is_dir() && input.join("source.jsonl").is_file());
        is_hit.then(|| DetectHit {
            engine_id: self.id(),
            label: self.label(),
            content_root: input.canonicalize().unwrap_or_else(|_| input.to_path_buf()),
        })
    }

    fn extract(&self, input: &Path, _source_lang: &str) -> Result<Vec<TextUnit>> {
        read_jsonl_units(&Self::source_file(input)?)
    }

    fn writeback(
        &self,
        input: &Path,
        target_lang: &str,
        units: &[TextUnit],
        translations: &BTreeMap<String, Translation>,
    ) -> Result<Vec<OutputFile>> {
        let mut out_lines = Vec::new();
        for u in units {
            out_lines.push(serde_json::to_string(&record_for(
                u,
                translations.get(&u.id),
            ))?);
        }
        let body = out_lines.join("\n") + "\n";
        let dest = if input.is_dir() {
            input.join("translated.jsonl")
        } else {
            output_sibling(input, target_lang, "jsonl")
        };
        Ok(vec![OutputFile::text(dest, body)])
    }
}

fn record_for(u: &TextUnit, tr: Option<&Translation>) -> serde_json::Value {
    let translation_lines = tr.map(|t| t.translation_lines.clone());
    let translation = translation_lines
        .as_ref()
        .map(|l| l.join("\n"))
        .unwrap_or_default();
    serde_json::json!({
        "id": u.location,
        "text": u.joined_text(),
        "context": u.context,
        "role": u.role,
        "item_type": u.item_type.as_str(),
        "translation": translation,
        "translation_lines": translation_lines,
    })
}

/// Standalone helper used by translate-jsonl CLI (no workspace).
pub fn read_jsonl_units(path: &Path) -> Result<Vec<TextUnit>> {
    let file = fs::File::open(path).with_context(|| format!("{}", path.display()))?;
    let reader = BufReader::new(file);
    let mut units = Vec::new();
    for (lineno, line) in reader.lines().enumerate() {
        let line = line?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let rec: crate::model::JsonlRecord =
            serde_json::from_str(line).with_context(|| format!("line {}", lineno + 1))?;
        let item_type = ItemType::parse(rec.item_type.as_deref().unwrap_or("long_text"));
        let lines: Vec<String> = if rec.text.contains('\n') {
            rec.text.lines().map(|s| s.to_string()).collect()
        } else {
            vec![rec.text.clone()]
        };
        let location = if rec.id.is_empty() {
            format!("line:{}", lineno + 1)
        } else {
            rec.id.clone()
        };
        let id = TextUnit::compute_id("jsonl", &location, &lines);
        units.push(TextUnit {
            id,
            engine: "jsonl".into(),
            domain: "jsonl".into(),
            location,
            item_type,
            role: if rec.role.is_empty() {
                "旁白".into()
            } else {
                rec.role
            },
            original_lines: lines,
            source_line_paths: vec![],
            context: rec.context,
            payload: String::new(),
        });
    }
    Ok(units)
}

pub fn write_jsonl_translations(
    path: &Path,
    units: &[TextUnit],
    translations: &BTreeMap<String, Translation>,
) -> Result<usize> {
    let mut f = fs::File::create(path).with_context(|| format!("{}", path.display()))?;
    let mut n = 0;
    for u in units {
        writeln!(
            f,
            "{}",
            serde_json::to_string(&record_for(u, translations.get(&u.id)))?
        )?;
        n += 1;
    }
    Ok(n)
}
