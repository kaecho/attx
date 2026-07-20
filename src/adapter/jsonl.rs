use super::{DetectHit, EngineAdapter};
use crate::model::{ItemType, TextUnit, Translation};
use anyhow::{Context, Result, bail};
use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

/// Adapter for a directory containing `source.jsonl` (or any *.jsonl marked as game).
/// Detects when path is a file ending in .jsonl or a dir with source.jsonl.
pub struct JsonlAdapter;

impl EngineAdapter for JsonlAdapter {
    fn id(&self) -> &'static str {
        "jsonl"
    }
    fn label(&self) -> &'static str {
        "Generic JSONL text pack"
    }

    fn detect(&self, game_path: &Path) -> Option<DetectHit> {
        if game_path.is_file()
            && game_path
                .extension()
                .and_then(|e| e.to_str())
                == Some("jsonl")
        {
            return Some(DetectHit {
                engine_id: self.id(),
                label: self.label(),
                content_root: game_path
                    .parent()
                    .unwrap_or(Path::new("."))
                    .canonicalize()
                    .unwrap_or_else(|_| game_path.parent().unwrap_or(Path::new(".")).to_path_buf()),
            });
        }
        if game_path.is_dir() && game_path.join("source.jsonl").is_file() {
            return Some(DetectHit {
                engine_id: self.id(),
                label: self.label(),
                content_root: game_path
                    .canonicalize()
                    .unwrap_or_else(|_| game_path.to_path_buf()),
            });
        }
        None
    }

    fn extract(&self, content_root: &Path, _source_lang: &str) -> Result<Vec<TextUnit>> {
        let path = if content_root.join("source.jsonl").is_file() {
            content_root.join("source.jsonl")
        } else {
            bail!("jsonl adapter expects source.jsonl under {}", content_root.display());
        };
        let file = fs::File::open(&path).with_context(|| format!("{}", path.display()))?;
        let reader = BufReader::new(file);
        let mut units = Vec::new();
        for (lineno, line) in reader.lines().enumerate() {
            let line = line?;
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let rec: crate::model::JsonlRecord = serde_json::from_str(line)
                .with_context(|| format!("jsonl line {}", lineno + 1))?;
            let item_type = ItemType::parse(rec.item_type.as_deref().unwrap_or("long_text"));
            let lines: Vec<String> = if item_type == ItemType::Array {
                rec.text.lines().map(|s| s.to_string()).collect()
            } else if rec.text.contains('\n') {
                rec.text.lines().map(|s| s.to_string()).collect()
            } else {
                vec![rec.text.clone()]
            };
            let location = rec.id.clone();
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

    fn writeback(
        &self,
        content_root: &Path,
        units: &[TextUnit],
        translations: &BTreeMap<String, Translation>,
    ) -> Result<BTreeMap<String, String>> {
        let mut out_lines = Vec::new();
        for u in units {
            let tr = translations.get(&u.id);
            let translation_lines = tr.map(|t| t.translation_lines.clone());
            let translation = translation_lines
                .as_ref()
                .map(|l| l.join("\n"))
                .unwrap_or_default();
            let rec = serde_json::json!({
                "id": u.location,
                "text": u.joined_text(),
                "context": u.context,
                "role": u.role,
                "item_type": u.item_type.as_str(),
                "translation": translation,
                "translation_lines": translation_lines,
            });
            out_lines.push(serde_json::to_string(&rec)?);
        }
        let body = out_lines.join("\n") + "\n";
        // also write to content_root/translated.jsonl for convenience when not using pipeline file writer
        let mut map = BTreeMap::new();
        map.insert("translated.jsonl".into(), body);
        let _ = content_root;
        Ok(map)
    }
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
        let lines = if item_type == ItemType::Array {
            rec.text.lines().map(|s| s.to_string()).collect()
        } else if rec.text.contains('\n') {
            rec.text.lines().map(|s| s.to_string()).collect()
        } else {
            vec![rec.text]
        };
        let location = rec.id.clone();
        let id = if rec.id.is_empty() {
            TextUnit::compute_id("jsonl", &format!("line:{}", lineno + 1), &lines)
        } else {
            TextUnit::compute_id("jsonl", &location, &lines)
        };
        units.push(TextUnit {
            id,
            engine: "jsonl".into(),
            domain: "jsonl".into(),
            location: if rec.id.is_empty() {
                format!("line:{}", lineno + 1)
            } else {
                rec.id
            },
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
        let tr = translations.get(&u.id);
        let translation_lines = tr.map(|t| t.translation_lines.clone());
        let translation = translation_lines
            .as_ref()
            .map(|l| l.join("\n"))
            .unwrap_or_default();
        let rec = serde_json::json!({
            "id": u.location,
            "text": u.joined_text(),
            "context": u.context,
            "role": u.role,
            "item_type": u.item_type.as_str(),
            "translation": translation,
            "translation_lines": translation_lines,
        });
        writeln!(f, "{}", serde_json::to_string(&rec)?)?;
        n += 1;
    }
    Ok(n)
}
