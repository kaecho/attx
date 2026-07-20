pub mod jsonl;
pub mod rmmz;

use crate::model::{TextUnit, Translation};
use anyhow::{Result, bail};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct DetectHit {
    pub engine_id: &'static str,
    pub label: &'static str,
    /// Absolute path to content root (may equal game root)
    pub content_root: PathBuf,
}

pub trait EngineAdapter: Send + Sync {
    fn id(&self) -> &'static str;
    fn label(&self) -> &'static str;
    fn detect(&self, game_path: &Path) -> Option<DetectHit>;
    fn extract(&self, content_root: &Path, source_lang: &str) -> Result<Vec<TextUnit>>;
    /// Apply translations; returns map of relative path → new file content (UTF-8 text / JSON pretty).
    fn writeback(
        &self,
        content_root: &Path,
        units: &[TextUnit],
        translations: &BTreeMap<String, Translation>,
    ) -> Result<BTreeMap<String, String>>;
}

pub fn all_adapters() -> Vec<Box<dyn EngineAdapter>> {
    vec![Box::new(rmmz::RmmzAdapter), Box::new(jsonl::JsonlAdapter)]
}

pub fn detect(game_path: &Path) -> Result<DetectHit> {
    let game_path = game_path
        .canonicalize()
        .unwrap_or_else(|_| game_path.to_path_buf());
    for a in all_adapters() {
        if let Some(hit) = a.detect(&game_path) {
            return Ok(hit);
        }
    }
    bail!(
        "no engine adapter matched {}. Supported: rmmz (MV/MZ), jsonl workspace. Pass --engine to force.",
        game_path.display()
    )
}

pub fn get(engine_id: &str) -> Result<Box<dyn EngineAdapter>> {
    for a in all_adapters() {
        if a.id() == engine_id {
            return Ok(a);
        }
    }
    bail!("unknown engine adapter: {engine_id}")
}

pub fn detect_or_force(game_path: &Path, engine: Option<&str>) -> Result<DetectHit> {
    if let Some(id) = engine {
        let a = get(id)?;
        if let Some(hit) = a.detect(game_path) {
            return Ok(hit);
        }
        // force even if detect soft-fails (e.g. partial tree)
        return Ok(DetectHit {
            engine_id: a.id(),
            label: a.label(),
            content_root: game_path
                .canonicalize()
                .unwrap_or_else(|_| game_path.to_path_buf()),
        });
    }
    detect(game_path)
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
