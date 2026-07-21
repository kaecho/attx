use crate::adapter::{self, detect_or_force};
use crate::config::{self, Settings};
use crate::llm::Translator;
use crate::model::{TextUnit, Translation, WorkspaceMeta};
use crate::store::{self, Store};
use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Serialize)]
pub struct TranslateReport {
    pub pending_before: usize,
    pub translated: usize,
    pub pending_after: usize,
    pub dry_run: bool,
    #[serde(default)]
    pub skipped_note: String,
}

#[derive(Debug, Serialize)]
pub struct WritebackReport {
    pub files: usize,
    pub units_applied: usize,
    pub dry_run: bool,
    pub paths: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct StatusReport {
    pub engine: String,
    pub game_path: String,
    pub source_lang: String,
    pub target_lang: String,
    pub total: usize,
    pub translated: usize,
    pub pending: usize,
}

pub fn doctor(settings: &Settings, ping: bool) -> Result<()> {
    println!("attx doctor");
    config::ensure_example_written(Path::new("."))?;
    match settings.client(None) {
        Ok(c) => {
            println!("llm client: {} ({})", c.name, c.model);
            println!("base_url: {}", c.base_url);
            if ping {
                let t = Translator::new(c, &settings.translation, "ja")?;
                let r = t.ping()?;
                println!("ping: {}", r.chars().take(80).collect::<String>());
            } else {
                println!("ping: skipped (pass --ping)");
            }
        }
        Err(e) => {
            println!("llm: not configured ({e})");
            println!("write setting.toml from setting.example.toml");
        }
    }
    println!("adapters: rmmz, jsonl");
    Ok(())
}

pub fn init_workspace(
    game: &Path,
    engine: Option<&str>,
    src: &str,
    dst: &str,
    workspace: Option<PathBuf>,
) -> Result<PathBuf> {
    let hit = detect_or_force(game, engine)?;
    let ws = workspace.unwrap_or_else(|| hit.content_root.join(".attx"));
    std::fs::create_dir_all(&ws)?;
    let store = Store::open(&ws)?;
    let meta = WorkspaceMeta {
        engine: hit.engine_id.to_string(),
        game_path: game
            .canonicalize()
            .unwrap_or_else(|_| game.to_path_buf())
            .display()
            .to_string(),
        content_root: hit.content_root.display().to_string(),
        source_lang: src.to_string(),
        target_lang: dst.to_string(),
        created_at: now_secs(),
    };
    store.set_meta(&meta)?;
    // snapshot pointer
    std::fs::write(
        ws.join("workspace.json"),
        serde_json::to_string_pretty(&meta)?,
    )?;
    Ok(ws.canonicalize().unwrap_or(ws))
}

pub fn extract(workspace: &Path, _settings: &Settings) -> Result<usize> {
    let store = store::workspace_db(workspace)?;
    let meta = store.meta()?;
    let adapter = adapter::get(&meta.engine)?;
    let content_root = PathBuf::from(&meta.content_root);
    let units = adapter.extract(&content_root, &meta.source_lang)?;
    let n = units.len();
    store.replace_units(&units)?;
    Ok(n)
}

pub fn translate(
    workspace: &Path,
    settings: &Settings,
    limit: Option<usize>,
    dry_run: bool,
) -> Result<TranslateReport> {
    let store = store::workspace_db(workspace)?;
    let meta = store.meta()?;
    let pending = store.pending_units()?;
    let pending_before = pending.len();
    if dry_run {
        return Ok(TranslateReport {
            pending_before,
            translated: 0,
            pending_after: pending_before,
            dry_run: true,
            skipped_note: String::new(),
        });
    }
    if pending.is_empty() {
        return Ok(TranslateReport {
            pending_before: 0,
            translated: 0,
            pending_after: 0,
            dry_run: false,
            skipped_note: String::new(),
        });
    }
    let client = config::require_llm(settings)?;
    let translator = Translator::new(client, &settings.translation, &meta.source_lang)?;
    // Incremental save: each batch hits SQLite immediately so crashes keep progress.
    let results = translator.translate_units_with_sink(&pending, limit, &mut |batch| {
        for tr in batch {
            store.save_translation(tr)?;
        }
        Ok(())
    })?;
    let (_, _, pending_after) = store.counts()?;
    Ok(TranslateReport {
        pending_before,
        translated: results.len(),
        pending_after,
        dry_run: false,
        skipped_note: if pending_after > 0 {
            "re-run translate to fill remaining pending".into()
        } else {
            String::new()
        },
    })
}

pub fn writeback(
    workspace: &Path,
    _settings: &Settings,
    dry_run: bool,
) -> Result<WritebackReport> {
    let store = store::workspace_db(workspace)?;
    let meta = store.meta()?;
    let adapter = adapter::get(&meta.engine)?;
    let units = store.all_units()?;
    let translations = store.all_translations()?;
    // only units with translation
    let applied = units
        .iter()
        .filter(|u| translations.contains_key(&u.id))
        .count();
    let content_root = PathBuf::from(&meta.content_root);
    let files = adapter.writeback(&content_root, &units, &translations)?;
    let paths: Vec<String> = files.keys().cloned().collect();
    if dry_run {
        return Ok(WritebackReport {
            files: paths.len(),
            units_applied: applied,
            dry_run: true,
            paths,
        });
    }
    for (rel, content) in &files {
        let dest = content_root.join(rel);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if dest.is_file() {
            let bak = PathBuf::from(format!("{}.attxbak", dest.display()));
            if !bak.exists() {
                let _ = std::fs::copy(&dest, &bak);
            }
        }
        std::fs::write(&dest, content)
            .with_context(|| format!("write {}", dest.display()))?;
    }
    Ok(WritebackReport {
        files: paths.len(),
        units_applied: applied,
        dry_run: false,
        paths,
    })
}

pub fn status(workspace: &Path) -> Result<StatusReport> {
    let store = store::workspace_db(workspace)?;
    let meta = store.meta()?;
    let (total, translated, pending) = store.counts()?;
    Ok(StatusReport {
        engine: meta.engine,
        game_path: meta.game_path,
        source_lang: meta.source_lang,
        target_lang: meta.target_lang,
        total,
        translated,
        pending,
    })
}

pub fn translate_jsonl(
    input: &Path,
    output: &Path,
    settings: &Settings,
    src: &str,
    _dst: &str,
    limit: Option<usize>,
) -> Result<TranslateReport> {
    let units = adapter::jsonl::read_jsonl_units(input)?;
    let pending_before = units.len();
    let client = config::require_llm(settings)?;
    let translator = Translator::new(client, &settings.translation, src)?;
    let results = translator.translate_units(&units, limit)?;
    let mut map = BTreeMap::new();
    for tr in &results {
        map.insert(tr.unit_id.clone(), tr.clone());
    }
    // fill missing with empty to keep alignment optional — only write translated
    let n = adapter::jsonl::write_jsonl_translations(output, &units, &map)?;
    Ok(TranslateReport {
        pending_before,
        translated: n.min(results.len()),
        pending_after: pending_before.saturating_sub(results.len()),
        dry_run: false,
        skipped_note: String::new(),
    })
}

pub fn export_jsonl(workspace: &Path, output: &Path, filter: &str) -> Result<usize> {
    let store = store::workspace_db(workspace)?;
    let units = match filter {
        "pending" => store.pending_units()?,
        "translated" => {
            let all = store.all_units()?;
            let tr = store.all_translations()?;
            all.into_iter()
                .filter(|u| tr.contains_key(&u.id))
                .collect()
        }
        "all" => store.all_units()?,
        other => bail!("unknown filter {other}, use pending|all|translated"),
    };
    let tr = store.all_translations()?;
    adapter::jsonl::write_jsonl_translations(output, &units, &tr)
}

pub fn import_jsonl(workspace: &Path, input: &Path) -> Result<usize> {
    let store = store::workspace_db(workspace)?;
    let file = std::fs::File::open(input)?;
    let reader = std::io::BufReader::new(file);
    use std::io::BufRead;
    let mut n = 0;
    for line in reader.lines() {
        let line = line?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let rec: crate::model::JsonlRecord = serde_json::from_str(line)?;
        let lines = rec
            .translation_lines
            .clone()
            .or_else(|| rec.translation.map(|t| vec![t]))
            .unwrap_or_default();
        if lines.is_empty() {
            continue;
        }
        // find unit by location == id
        let units = store.all_units()?;
        let unit = units.iter().find(|u| u.location == rec.id);
        let Some(unit) = unit else {
            continue;
        };
        store.save_translation(&Translation {
            unit_id: unit.id.clone(),
            translation_lines: lines,
            source_hash: TextUnit::source_hash(&unit.original_lines),
        })?;
        n += 1;
    }
    Ok(n)
}

fn now_secs() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "0".into())
}
