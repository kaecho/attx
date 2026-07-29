use crate::adapter::{self, FormatAdapter};
use crate::config::{self, Settings};
use crate::knowledge;
use crate::llm::{Profile, Translator, profile_for_format};
use crate::model::{TextUnit, Translation, WorkspaceMeta, needs_translation};
use crate::profile::{self, CustomAdapter};
use crate::store::{self, Store};
use crate::textio;
use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde_json::json;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Serialize)]
pub struct TranslateReport {
    pub pending_before: usize,
    pub translated: usize,
    pub pending_after: usize,
    pub passthrough: usize,
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
    /// Units whose "translation" is the untouched original (model refused or
    /// kept failing). Re-queue with `translate --retry-passthrough`.
    pub passthrough: usize,
    /// domain -> {total, translated}
    pub domains: BTreeMap<String, serde_json::Value>,
}

/// Detect result surfaced to the CLI: engine may be a built-in adapter or a
/// saved custom profile (`custom:<name>`, with the profile path attached).
pub struct DetectAnyHit {
    pub engine: String,
    pub label: String,
    pub content_root: PathBuf,
    pub profile_path: Option<PathBuf>,
}

pub fn doctor(settings: &Settings, ping: bool, as_json: bool) -> Result<()> {
    config::ensure_example_written(Path::new("."))?;
    let adapters: Vec<String> = adapter::all_adapters()
        .iter()
        .map(|a| a.id().to_string())
        .collect();
    let profiles: Vec<serde_json::Value> = profile::saved_profiles()
        .iter()
        .map(|(path, a)| json!({"name": a.profile().name, "path": path.display().to_string()}))
        .collect();

    let mut llm = json!({"configured": false});
    let mut ping_result = "skipped".to_string();
    match settings.client(None) {
        Ok(c) => {
            llm = json!({
                "configured": true,
                "name": c.name,
                "model": c.model,
                "base_url": c.base_url,
            });
            if ping {
                let t = Translator::new(c, &settings.translation, "ja", "zh", Profile::Game)?;
                ping_result = match t.ping() {
                    Ok(r) => format!("ok: {}", r.chars().take(60).collect::<String>()),
                    Err(e) => format!("error: {e:#}"),
                };
            }
        }
        Err(e) => {
            llm["error"] = json!(format!("{e:#}"));
        }
    }

    if as_json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "llm": llm,
                "ping": ping_result,
                "adapters": adapters,
                "saved_profiles": profiles,
                "status": "ok",
            }))?
        );
        return Ok(());
    }

    println!("attx doctor");
    if llm["configured"].as_bool() == Some(true) {
        println!(
            "llm client: {} ({})",
            llm["name"].as_str().unwrap_or("?"),
            llm["model"].as_str().unwrap_or("?")
        );
        println!("base_url: {}", llm["base_url"].as_str().unwrap_or("?"));
        println!("ping: {ping_result}");
    } else {
        println!(
            "llm: not configured ({})",
            llm["error"].as_str().unwrap_or("no clients")
        );
        println!("write setting.toml from setting.example.toml");
    }
    println!("adapters: {}", adapters.join(", "));
    if !profiles.is_empty() {
        let names: Vec<&str> = profiles.iter().filter_map(|p| p["name"].as_str()).collect();
        println!("saved profiles: {}", names.join(", "));
    }
    Ok(())
}

/// Built-in adapters first, then saved custom profiles.
pub fn detect_any(input: &Path) -> Result<DetectAnyHit> {
    if let Ok(hit) = adapter::detect(input) {
        return Ok(DetectAnyHit {
            engine: hit.engine_id.to_string(),
            label: hit.label.to_string(),
            content_root: hit.content_root,
            profile_path: None,
        });
    }
    for (path, a) in profile::saved_profiles() {
        if let Some(hit) = a.detect(input) {
            return Ok(DetectAnyHit {
                engine: hit.engine_id.to_string(),
                label: hit.label.to_string(),
                content_root: hit.content_root,
                profile_path: Some(path),
            });
        }
    }
    let ids: Vec<String> = adapter::all_adapters()
        .iter()
        .map(|a| a.id().to_string())
        .collect();
    bail!(
        "no format adapter or saved profile matched {}. Supported: {}. \
         Run `attx analyze --input …` then write a custom profile \
         (`attx profile new`), or force with --engine.",
        input.display(),
        ids.join(", ")
    )
}

pub fn init_workspace(
    input: &Path,
    engine: Option<&str>,
    profile_arg: Option<&str>,
    src: &str,
    dst: &str,
    workspace: Option<PathBuf>,
) -> Result<PathBuf> {
    // Resolve engine + optional custom profile source file.
    let (engine_id, content_root, profile_src): (String, PathBuf, Option<PathBuf>) =
        if let Some(p) = profile_arg {
            let (path, a) = resolve_profile_arg(p)?;
            let root = input.canonicalize().unwrap_or_else(|_| input.to_path_buf());
            (a.id().to_string(), root, Some(path))
        } else if let Some(id) = engine {
            if id.starts_with(profile::ENGINE_PREFIX) {
                let (path, a) = profile::find_saved(id)?;
                let root = input.canonicalize().unwrap_or_else(|_| input.to_path_buf());
                (a.id().to_string(), root, Some(path))
            } else {
                let hit = adapter::detect_or_force(input, Some(id))?;
                (hit.engine_id.to_string(), hit.content_root, None)
            }
        } else {
            let hit = detect_any(input)?;
            (hit.engine, hit.content_root, hit.profile_path)
        };

    let ws = workspace.unwrap_or_else(|| default_workspace(&content_root));
    std::fs::create_dir_all(&ws)?;
    if let Some(src_path) = &profile_src {
        // Workspace keeps its own copy → runs stay reproducible even if the
        // saved profile changes later.
        std::fs::copy(src_path, ws.join(profile::WORKSPACE_PROFILE))
            .with_context(|| format!("copy profile {}", src_path.display()))?;
    }
    let store = Store::open(&ws)?;
    let meta = WorkspaceMeta {
        engine: engine_id,
        game_path: input
            .canonicalize()
            .unwrap_or_else(|_| input.to_path_buf())
            .display()
            .to_string(),
        content_root: content_root.display().to_string(),
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

/// `--profile` accepts a file path or a saved profile name.
fn resolve_profile_arg(arg: &str) -> Result<(PathBuf, CustomAdapter)> {
    let p = Path::new(arg);
    if p.is_file() {
        let a = CustomAdapter::load(p)?;
        return Ok((p.to_path_buf(), a));
    }
    profile::find_saved(arg)
}

/// Adapter for a workspace engine id; `custom:*` engines load the profile
/// copied into the workspace at init (fallback: saved profiles by name).
fn resolve_adapter(engine: &str, workspace: &Path) -> Result<Box<dyn FormatAdapter>> {
    if engine.starts_with(profile::ENGINE_PREFIX) {
        let ws_profile = workspace.join(profile::WORKSPACE_PROFILE);
        if ws_profile.is_file() {
            return Ok(Box::new(CustomAdapter::load(&ws_profile)?));
        }
        let (_, a) = profile::find_saved(engine)?;
        return Ok(Box::new(a));
    }
    adapter::get(engine)
}

/// Extraction report. `skipped_by_knowledge` is surfaced so a learned rule that
/// silently swallows units is visible without digging through the DB.
#[derive(Debug, Serialize)]
pub struct ExtractReport {
    pub extracted: usize,
    pub skipped_by_knowledge: usize,
    pub rules_applied: usize,
    pub status: &'static str,
}

pub fn extract(workspace: &Path, _settings: &Settings, use_knowledge: bool) -> Result<ExtractReport> {
    let store = store::workspace_db(workspace)?;
    let meta = store.meta()?;
    let adapter = resolve_adapter(&meta.engine, workspace)?;
    let content_root = PathBuf::from(&meta.content_root);
    let units = adapter.extract(&content_root, &meta.source_lang)?;

    // Learned rules are a pure filter *outside* the adapter: every format gets
    // them for free, and --no-knowledge restores the pre-learning behaviour
    // exactly, so a bad rule can always be bisected away.
    let (units, applied, skipped) = if use_knowledge {
        let rules = knowledge::load_rules(&meta.engine);
        let n = rules.rules.len();
        let (units, report) = knowledge::apply(units, &rules);
        if report.skipped > 0 {
            eprintln!(
                "knowledge: {} unit(s) skipped by {} learned rule(s) for {}",
                report.skipped, n, meta.engine
            );
        }
        if report.extract_vetoed > 0 {
            eprintln!(
                "knowledge: {} extract rule hit(s) vetoed (value is machine data)",
                report.extract_vetoed
            );
        }
        (units, n, report.skipped)
    } else {
        (units, 0, 0)
    };

    let n = units.len();
    store.replace_units(&units)?;
    Ok(ExtractReport {
        extracted: n,
        skipped_by_knowledge: skipped,
        rules_applied: applied,
        status: "ok",
    })
}

pub fn translate(
    workspace: &Path,
    settings: &Settings,
    limit: Option<usize>,
    dry_run: bool,
    retry_passthrough: bool,
) -> Result<TranslateReport> {
    let store = store::workspace_db(workspace)?;
    let meta = store.meta()?;
    if retry_passthrough {
        let n = store.clear_passthrough()?;
        if n > 0 {
            eprintln!("re-queued {n} passthrough unit(s)");
        }
    }
    let pending = store.pending_units()?;
    let pending_before = pending.len();
    if dry_run || pending.is_empty() {
        let counts = store.counts()?;
        return Ok(TranslateReport {
            pending_before,
            translated: 0,
            pending_after: pending_before,
            passthrough: counts.passthrough,
            dry_run,
            skipped_note: String::new(),
        });
    }
    let client = config::require_llm(settings)?;
    let translator = Translator::new(
        client,
        &settings.translation,
        &meta.source_lang,
        &meta.target_lang,
        profile_for_format(&meta.engine),
    )?;
    // Incremental save: each batch hits SQLite immediately so crashes keep progress.
    let results = translator.translate_units_with_sink(&pending, limit, &mut |batch| {
        for tr in batch {
            store.save_translation(tr)?;
        }
        Ok(())
    })?;
    let counts = store.counts()?;
    Ok(TranslateReport {
        pending_before,
        translated: results.len(),
        pending_after: counts.pending,
        passthrough: counts.passthrough,
        dry_run: false,
        skipped_note: if counts.pending > 0 {
            "re-run translate to fill remaining pending".into()
        } else if counts.passthrough > 0 {
            "some units kept original text (model refused); retry with --retry-passthrough".into()
        } else {
            String::new()
        },
    })
}

pub fn writeback(workspace: &Path, _settings: &Settings, dry_run: bool) -> Result<WritebackReport> {
    let store = store::workspace_db(workspace)?;
    let meta = store.meta()?;
    let adapter = resolve_adapter(&meta.engine, workspace)?;
    let units = store.all_units()?;
    let translations = store.all_translations()?;
    // only units with translation
    let applied = units
        .iter()
        .filter(|u| translations.contains_key(&u.id))
        .count();
    let input = PathBuf::from(&meta.content_root);
    let outputs = adapter.writeback(&input, &meta.target_lang, &units, &translations)?;
    let paths: Vec<String> = outputs
        .iter()
        .map(|o| o.path.display().to_string())
        .collect();
    if dry_run {
        return Ok(WritebackReport {
            files: paths.len(),
            units_applied: applied,
            dry_run: true,
            paths,
        });
    }
    for out in &outputs {
        if let Some(parent) = out.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if out.path.is_file() {
            let bak = PathBuf::from(format!("{}.attxbak", out.path.display()));
            if !bak.exists() {
                let _ = std::fs::copy(&out.path, &bak);
            }
        }
        std::fs::write(&out.path, &out.bytes)
            .with_context(|| format!("write {}", out.path.display()))?;
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
    let counts = store.counts()?;
    let domains = store
        .domain_counts()?
        .into_iter()
        .map(|(d, (total, translated))| (d, json!({"total": total, "translated": translated})))
        .collect();
    Ok(StatusReport {
        engine: meta.engine,
        game_path: meta.game_path,
        source_lang: meta.source_lang,
        target_lang: meta.target_lang,
        total: counts.total,
        translated: counts.translated,
        pending: counts.pending,
        passthrough: counts.passthrough,
        domains,
    })
}

pub fn translate_jsonl(
    input: &Path,
    output: &Path,
    settings: &Settings,
    src: &str,
    dst: &str,
    limit: Option<usize>,
) -> Result<TranslateReport> {
    let units = adapter::jsonl::read_jsonl_units(input)?;
    let pending_before = units.len();
    let client = config::require_llm(settings)?;
    let translator = Translator::new(client, &settings.translation, src, dst, Profile::Game)?;
    let results = translator.translate_units(&units, limit)?;
    let mut map = BTreeMap::new();
    let mut passthrough = 0usize;
    for tr in &results {
        if tr.passthrough {
            passthrough += 1;
        }
        map.insert(tr.unit_id.clone(), tr.clone());
    }
    adapter::jsonl::write_jsonl_translations(output, &units, &map)?;
    Ok(TranslateReport {
        pending_before,
        translated: results.len(),
        pending_after: pending_before.saturating_sub(results.len()),
        passthrough,
        dry_run: false,
        skipped_note: String::new(),
    })
}

pub fn export_jsonl(workspace: &Path, output: &Path, filter: &str) -> Result<usize> {
    let store = store::workspace_db(workspace)?;
    let tr = store.all_translations()?;
    let units = match filter {
        "pending" => store.pending_units()?,
        "translated" => {
            let all = store.all_units()?;
            all.into_iter().filter(|u| tr.contains_key(&u.id)).collect()
        }
        "passthrough" => {
            let all = store.all_units()?;
            all.into_iter()
                .filter(|u| tr.get(&u.id).is_some_and(|t| t.passthrough))
                .collect()
        }
        "all" => store.all_units()?,
        other => bail!("unknown filter {other}, use pending|all|translated|passthrough"),
    };
    adapter::jsonl::write_jsonl_translations(output, &units, &tr)
}

pub fn import_jsonl(workspace: &Path, input: &Path) -> Result<usize> {
    let store = store::workspace_db(workspace)?;
    // Index once — imports may carry tens of thousands of lines.
    let units = store.all_units()?;
    let by_location: BTreeMap<&str, &TextUnit> =
        units.iter().map(|u| (u.location.as_str(), u)).collect();
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
        let Some(unit) = by_location.get(rec.id.as_str()) else {
            continue;
        };
        store.save_translation(&Translation {
            unit_id: unit.id.clone(),
            translation_lines: lines,
            source_hash: TextUnit::source_hash(&unit.original_lines),
            passthrough: false,
        })?;
        n += 1;
    }
    Ok(n)
}

// ---------------------------------------------------------------- analyze

/// Recon report for unknown inputs — everything an agent needs to decide
/// between a built-in adapter, a custom profile, or the JSONL escape hatch.
pub fn analyze(input: &Path, src: &str) -> Result<serde_json::Value> {
    if !input.exists() {
        bail!("input not found: {}", input.display());
    }
    let builtin = adapter::detect(input)
        .map(|h| json!({"engine": h.engine_id, "label": h.label}))
        .unwrap_or(serde_json::Value::Null);
    let saved = profile::saved_profiles()
        .iter()
        .find_map(|(path, a)| {
            a.detect(input)
                .map(|h| json!({"engine": h.engine_id, "profile": path.display().to_string()}))
        })
        .unwrap_or(serde_json::Value::Null);

    let details = if input.is_file() {
        analyze_file(input, src)?
    } else {
        analyze_dir(input, src)?
    };
    Ok(json!({
        "input": input.canonicalize().unwrap_or_else(|_| input.to_path_buf()).display().to_string(),
        "kind": if input.is_dir() { "directory" } else { "file" },
        "builtin_detect": builtin,
        "saved_profile_detect": saved,
        "details": details,
        "next_steps": [
            "builtin_detect/saved_profile_detect non-null → attx init --input …",
            "otherwise: write a profile (attx profile new), iterate with attx profile test",
            "binary or too complex → external extractor + attx translate-jsonl",
        ],
    }))
}

fn analyze_file(input: &Path, src: &str) -> Result<serde_json::Value> {
    let size = std::fs::metadata(input)?.len();
    let bytes = std::fs::read(input)?;
    let has_utf16_bom = bytes.starts_with(b"\xFF\xFE") || bytes.starts_with(b"\xFE\xFF");
    let looks_binary = !has_utf16_bom && bytes.iter().take(65536).any(|b| *b == 0);
    if looks_binary {
        let container = if bytes.starts_with(b"PK\x03\x04") {
            "zip (try epub/docx/xlsx, or unpack and analyze entries)"
        } else {
            "unknown binary"
        };
        return Ok(json!({
            "size": size,
            "binary": true,
            "container": container,
        }));
    }
    let decoded = textio::decode_bytes(&bytes);
    let text = &decoded.text;
    let total_lines = text.lines().count();
    let source_lines = text.lines().filter(|l| needs_translation(l, src)).count();
    let sample: Vec<String> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .take(40)
        .map(|l| l.chars().take(160).collect())
        .collect();
    let json_shape = serde_json::from_str::<serde_json::Value>(text)
        .map(|v| match &v {
            serde_json::Value::Object(o) => json!({
                "type": "object",
                "top_keys": o.keys().take(20).cloned().collect::<Vec<_>>(),
            }),
            serde_json::Value::Array(a) => json!({
                "type": "array",
                "len": a.len(),
                "first": a.first().cloned().unwrap_or(serde_json::Value::Null),
            }),
            _ => json!({"type": "scalar"}),
        })
        .unwrap_or(serde_json::Value::Null);
    Ok(json!({
        "size": size,
        "binary": false,
        "encoding": decoded.encoding,
        "encoding_lossy": decoded.lossy,
        "total_lines": total_lines,
        "source_language_lines": source_lines,
        "json": json_shape,
        "sample_head": sample,
    }))
}

fn analyze_dir(input: &Path, src: &str) -> Result<serde_json::Value> {
    let mut ext_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut files = 0usize;
    let mut sample_files: Vec<String> = Vec::new();
    for entry in walkdir::WalkDir::new(input)
        .max_depth(6)
        .sort_by_file_name()
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            !(name.starts_with(".attx") || name == ".git" || name == "node_modules")
        })
        .flatten()
    {
        if !entry.file_type().is_file() {
            continue;
        }
        files += 1;
        let ext = entry
            .path()
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("<none>")
            .to_ascii_lowercase();
        *ext_counts.entry(ext).or_insert(0) += 1;
        if sample_files.len() < 15 {
            sample_files.push(
                entry
                    .path()
                    .strip_prefix(input)
                    .unwrap_or(entry.path())
                    .display()
                    .to_string(),
            );
        }
    }
    // Peek into the most common text-looking extension for a content sample.
    const MEDIA: &[&str] = &[
        "png", "jpg", "jpeg", "webp", "gif", "ogg", "wav", "mp3", "m4a", "avif", "mp4", "webm",
        "ttf", "otf", "woff", "woff2", "dll", "exe", "so", "dylib", "<none>",
    ];
    let peek = ext_counts
        .iter()
        .filter(|(e, _)| !MEDIA.contains(&e.as_str()))
        .max_by_key(|(_, n)| **n)
        .map(|(e, _)| e.clone());
    let peek_sample = peek.as_ref().and_then(|ext| {
        walkdir::WalkDir::new(input)
            .max_depth(6)
            .sort_by_file_name()
            .into_iter()
            .flatten()
            .find(|en| {
                en.file_type().is_file()
                    && en
                        .path()
                        .extension()
                        .and_then(|x| x.to_str())
                        .map(|x| x.eq_ignore_ascii_case(ext))
                        .unwrap_or(false)
            })
            .and_then(|en| {
                analyze_file(en.path(), src)
                    .ok()
                    .map(|v| json!({"file": en.path().display().to_string(), "analysis": v}))
            })
    });
    Ok(json!({
        "files": files,
        "extensions": ext_counts,
        "sample_files": sample_files,
        "peek": peek_sample.unwrap_or(serde_json::Value::Null),
    }))
}

// ---------------------------------------------------------------- profiles

pub fn profile_test(
    profile_path: &Path,
    input: &Path,
    src: &str,
    limit: usize,
    roundtrip: bool,
) -> Result<serde_json::Value> {
    let a = CustomAdapter::load(profile_path)?;
    let units = a.extract(input, src)?;
    let sample: Vec<serde_json::Value> = units
        .iter()
        .take(limit.max(1))
        .map(|u| {
            json!({
                "location": u.location,
                "role": u.role,
                "text": u.joined_text(),
            })
        })
        .collect();
    let mut out = json!({
        "profile": a.profile().name,
        "engine": a.id(),
        "units": units.len(),
        "sample": sample,
        "detects": a.detect(input).is_some(),
    });
    if roundtrip && !units.is_empty() {
        // In-memory writeback with marker translations — nothing touches disk.
        let tr: BTreeMap<String, Translation> = units
            .iter()
            .map(|u| {
                (
                    u.id.clone(),
                    Translation {
                        unit_id: u.id.clone(),
                        translation_lines: u
                            .original_lines
                            .iter()
                            .map(|l| format!("【译】{l}"))
                            .collect(),
                        source_hash: TextUnit::source_hash(&u.original_lines),
                        passthrough: false,
                    },
                )
            })
            .collect();
        match a.writeback(input, "zh", &units, &tr) {
            Ok(files) => {
                out["roundtrip"] = json!({
                    "ok": true,
                    "output_files": files.len(),
                    "outputs": files
                        .iter()
                        .map(|f| f.path.display().to_string())
                        .collect::<Vec<_>>(),
                });
            }
            Err(e) => out["roundtrip"] = json!({"ok": false, "error": format!("{e:#}")}),
        }
    }
    Ok(out)
}

pub fn profile_list() -> serde_json::Value {
    let list: Vec<serde_json::Value> = profile::saved_profiles()
        .iter()
        .map(|(path, a)| {
            json!({
                "name": a.profile().name,
                "engine": a.id(),
                "label": a.label(),
                "extensions": a.profile().extensions,
                "path": path.display().to_string(),
            })
        })
        .collect();
    json!({
        "profiles": list,
        "dirs": profile::profile_dirs()
            .iter()
            .map(|d| d.display().to_string())
            .collect::<Vec<_>>(),
    })
}

// ---------------------------------------------------------------- misc

fn now_secs() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "0".into())
}

/// Directory inputs nest `.attx/` inside; file inputs get a sibling
/// `.attx-<stem>/` so several files in one directory don't collide.
fn default_workspace(input: &Path) -> PathBuf {
    if input.is_dir() {
        return input.join(".attx");
    }
    let stem = input
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("input");
    input
        .parent()
        .unwrap_or(Path::new("."))
        .join(format!(".attx-{stem}"))
}

/// Machine-readable adapter capability list for `attx formats` — built-ins
/// plus saved custom profiles.
pub fn formats() -> serde_json::Value {
    let mut list: Vec<serde_json::Value> = adapter::all_adapters()
        .iter()
        .map(|a| {
            json!({
                "id": a.id(),
                "label": a.label(),
                "extensions": a.extensions(),
                "input": a.input_kind(),
            })
        })
        .collect();
    for (path, a) in profile::saved_profiles() {
        list.push(json!({
            "id": a.id(),
            "label": a.label(),
            "extensions": a.profile().extensions,
            "input": "file|directory",
            "profile": path.display().to_string(),
        }));
    }
    json!({ "formats": list })
}
