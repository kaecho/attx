use super::{DetectHit, FormatAdapter, OutputFile};
use crate::model::{ItemType, TextUnit, Translation, needs_translation};
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const CODE_NAME: i64 = 101;
const CODE_CHOICES: i64 = 102;
const CODE_TEXT: i64 = 401;
const CODE_SCROLL: i64 = 405;

const BASE_FILES: &[&str] = &[
    "Actors.json",
    "Armors.json",
    "Classes.json",
    "Enemies.json",
    "Items.json",
    "Skills.json",
    "States.json",
    "Weapons.json",
];

const BASE_FIELDS: &[&str] = &[
    "profile",
    "description",
    "message1",
    "message2",
    "message3",
    "message4",
];

pub struct RmmzAdapter;

impl FormatAdapter for RmmzAdapter {
    fn id(&self) -> &'static str {
        "rmmz"
    }
    fn label(&self) -> &'static str {
        "RPG Maker MV/MZ"
    }
    fn input_kind(&self) -> &'static str {
        "directory"
    }

    fn detect(&self, game_path: &Path) -> Option<DetectHit> {
        let root = find_content_root(game_path)?;
        let data = root.join("data");
        if !data.is_dir() {
            return None;
        }
        let has_system = data.join("System.json").is_file();
        let has_js = root.join("js").is_dir()
            || root.join("js/rmmz_core.js").is_file()
            || root.join("js/rpg_core.js").is_file();
        if has_system || has_js {
            return Some(DetectHit {
                engine_id: self.id(),
                label: self.label(),
                content_root: root,
            });
        }
        None
    }

    fn extract(&self, content_root: &Path, source_lang: &str) -> Result<Vec<TextUnit>> {
        let data_dir = resolve_data_dir(content_root)?;
        let mut units = Vec::new();

        // Event commands from Map*, CommonEvents, Troops
        for entry in fs::read_dir(&data_dir).with_context(|| format!("{}", data_dir.display()))? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if !(name.starts_with("Map") && name.ends_with(".json")
                || name == "CommonEvents.json"
                || name == "Troops.json")
            {
                continue;
            }
            let path = source_json_path(&entry.path())?;
            let raw =
                fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
            let value: Value =
                serde_json::from_str(&raw).with_context(|| format!("json {}", path.display()))?;
            extract_commands(&name, &value, source_lang, &mut units)?;
            attach_anchor_hash(&name, &value, &mut units);
        }

        // System.json
        let system_path = source_json_path(&data_dir.join("System.json"))?;
        if system_path.is_file() {
            let raw = fs::read_to_string(&system_path)?;
            let system: Value = serde_json::from_str(&raw)?;
            extract_system(&system, source_lang, &mut units);
            attach_anchor_hash("System.json", &system, &mut units);
        }

        // Base DB
        for file in BASE_FILES {
            let p = source_json_path(&data_dir.join(file))?;
            if !p.is_file() {
                continue;
            }
            let raw = fs::read_to_string(&p)?;
            let arr: Value = serde_json::from_str(&raw)?;
            extract_base(file, &arr, source_lang, &mut units);
            attach_anchor_hash(file, &arr, &mut units);
        }

        // Plugin parameters (js/plugins.js + header @param types; never rewrite plugin source)
        let mut plugin_units = super::rmmz_plugins::extract_plugins(content_root, source_lang)?;
        units.append(&mut plugin_units);

        Ok(units)
    }

    fn writeback(
        &self,
        content_root: &Path,
        _target_lang: &str,
        units: &[TextUnit],
        translations: &BTreeMap<String, Translation>,
    ) -> Result<Vec<OutputFile>> {
        let data_dir = data_dir_target(content_root);
        let mut files: BTreeMap<String, Value> = BTreeMap::new();

        // Load only data/* files we need (skip js/plugins.js locations)
        let mut needed = BTreeMap::<String, ()>::new();
        for u in units {
            if u.domain == "plugins" || u.location.starts_with("js/") {
                continue;
            }
            if let Some(file) = u.location.split('/').next() {
                needed.insert(file.to_string(), ());
            }
        }

        for file in needed.keys() {
            let p = data_dir.join(file);
            if !p.is_file() {
                bail!("missing live data {}", p.display());
            }
            let raw =
                fs::read_to_string(&p).with_context(|| format!("read live data {}", p.display()))?;
            let v: Value = serde_json::from_str(&raw)?;
            let live_hash = anchor_hash(file, &v);
            for unit in units.iter().filter(|unit| unit.location.split('/').next() == Some(file.as_str())) {
                let payload: Value = serde_json::from_str(&unit.payload)
                    .with_context(|| format!("missing extraction anchor snapshot for {}", unit.location))?;
                let expected = payload.get("anchor_hash").and_then(Value::as_str)
                    .ok_or_else(|| anyhow::anyhow!("missing extraction anchor hash for {}", unit.location))?;
                if expected != live_hash {
                    bail!("live anchors changed in {file}; refusing stale source slots");
                }
            }
            files.insert(file.clone(), v);
        }

        for u in units {
            if u.domain == "plugins" || u.location.starts_with("js/") {
                continue;
            }
            let translation = translations.get(&u.id).filter(|tr| !tr.translation_lines.is_empty());
            apply_unit(&mut files, u, translation)?;
        }

        let mut out = Vec::new();
        for (file, value) in files {
            let text = serde_json::to_string(&value)?;
            out.push(OutputFile::text(
                data_dir_target(content_root).join(&file),
                text,
            ));
        }

        if let Some(plugins_js) =
            super::rmmz_plugins::writeback_plugins(content_root, units, translations)?
        {
            out.push(OutputFile::text(
                content_root.join("js/plugins.js"),
                plugins_js,
            ));
        }

        Ok(out)
    }
}

/// Writeback always targets the live `data/` dir, even when extraction read
/// from a `data_origin` snapshot.
fn data_dir_target(content_root: &Path) -> PathBuf {
    content_root.join("data")
}

/// First-write backups retain source text when live data has been translated.
/// An explicit data_origin snapshot already contains the authoritative source.
pub(super) fn source_json_path(path: &Path) -> Result<PathBuf> {
    if path.parent().and_then(Path::file_name).is_some_and(|name| name == "data_origin") {
        return Ok(path.to_owned());
    }
    let mut backup = path.as_os_str().to_os_string();
    backup.push(".attxbak");
    let backup = PathBuf::from(backup);
    match fs::symlink_metadata(&backup) {
        Ok(metadata) if metadata.file_type().is_file() => Ok(backup),
        Ok(_) => bail!("authoritative source backup is not a regular file: {}", backup.display()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(path.to_owned()),
        Err(error) => Err(error).with_context(|| format!("inspect source backup {}", backup.display())),
    }
}

fn attach_anchor_hash(file: &str, source: &Value, units: &mut [TextUnit]) {
    let hash = anchor_hash(file, source);
    for unit in units.iter_mut().filter(|unit| unit.location.split('/').next() == Some(file)) {
        unit.payload = json!({"anchor_hash": hash}).to_string();
    }
}

struct HashWriter<'a>(&'a mut Sha256);

impl std::io::Write for HashWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
}

pub(super) fn hash_value(hash: &mut Sha256, value: &Value) {
    serde_json::to_writer(HashWriter(&mut *hash), value).expect("hash writer is infallible");
    hash.update(b"\0");
}

fn hash_text_shape(hash: &mut Sha256, value: &Value) {
    match value {
        Value::String(_) => hash.update(b"string\0"),
        Value::Array(values) => {
            hash.update(b"[\0");
            for value in values { hash_text_shape(hash, value); }
            hash.update(b"]\0");
        }
        Value::Object(values) => {
            hash.update(b"{\0");
            for (key, value) in values {
                hash.update(key.as_bytes());
                hash.update(b"\0");
                hash_text_shape(hash, value);
            }
            hash.update(b"}\0");
        }
        _ => hash_value(hash, value),
    }
}

// Event topology, command order, codes, indentation and non-text parameters
// are anchors. Coordinates, graphics and other database gameplay values are not.
fn anchor_hash(file: &str, root: &Value) -> String {
    let mut hash = Sha256::new();
    if file == "System.json" {
        hash_text_shape(&mut hash, &root["gameTitle"]);
        hash_text_shape(&mut hash, &root["terms"]);
    } else if BASE_FILES.contains(&file) {
        if let Some(items) = root.as_array() {
            for item in items {
                hash_value(&mut hash, &item["id"]);
                hash_text_shape(&mut hash, &item["name"]);
                for field in BASE_FIELDS { hash_text_shape(&mut hash, &item[*field]); }
            }
        } else { hash_value(&mut hash, root); }
    } else {
        let entities = if file.starts_with("Map") { &root["events"] } else { root };
        if let Some(entities) = entities.as_array() {
            for entity in entities {
                hash_value(&mut hash, &entity["id"]);
                hash_value(&mut hash, &entity["name"]);
                if file == "CommonEvents.json" {
                    hash_command_list(&mut hash, &entity["list"]);
                } else if let Some(pages) = entity["pages"].as_array() {
                    hash.update(b"pages[\0");
                    for page in pages {
                        // Conditions distinguish otherwise structurally equal pages.
                        hash_value(&mut hash, &page["conditions"]);
                        hash_command_list(&mut hash, &page["list"]);
                    }
                    hash.update(b"]\0");
                } else { hash_value(&mut hash, &entity["pages"]); }
            }
        } else { hash_value(&mut hash, entities); }
    }
    format!("{:x}", hash.finalize())
}

fn hash_command_list(hash: &mut Sha256, list: &Value) {
    let Some(commands) = list.as_array() else { hash_value(hash, list); return; };
    hash.update(b"list[\0");
    for command in commands {
        let Some(fields) = command.as_object() else { hash_value(hash, command); continue; };
        hash.update(b"command{\0");
        for (key, value) in fields {
            hash.update(key.as_bytes());
            hash.update(b"\0");
            let code = command["code"].as_i64();
            if key == "parameters" && let Some(params) = value.as_array() {
                hash.update(b"params[\0");
                for (index, param) in params.iter().enumerate() {
                    let text_slot = matches!(code, Some(CODE_TEXT | CODE_SCROLL | CODE_CHOICES)) && index == 0
                        || code == Some(CODE_NAME) && index == 4;
                    if text_slot { hash_text_shape(hash, param); }
                    else { hash_value(hash, param); }
                }
                hash.update(b"]\0");
            } else { hash_value(hash, value); }
        }
        hash.update(b"}\0");
    }
    hash.update(b"]\0");
}

fn find_content_root(game_path: &Path) -> Option<PathBuf> {
    let candidates = [
        game_path.to_path_buf(),
        game_path.join("www"),
        game_path.join("game"),
    ];
    for c in candidates {
        if c.join("data/System.json").is_file() || c.join("js").is_dir() {
            return Some(c.canonicalize().unwrap_or(c));
        }
    }
    // walk one level
    if let Ok(rd) = fs::read_dir(game_path) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() && p.join("data/System.json").is_file() {
                return Some(p.canonicalize().unwrap_or(p));
            }
        }
    }
    None
}

fn resolve_data_dir(content_root: &Path) -> Result<PathBuf> {
    // Prefer data_origin snapshot if present (att-mz style), else data/
    let origin = content_root.join("data_origin");
    if origin.is_dir() && origin.join("System.json").is_file() {
        return Ok(origin);
    }
    let data = content_root.join("data");
    if data.is_dir() {
        return Ok(data);
    }
    bail!("no data/ under {}", content_root.display())
}

fn extract_commands(
    file_name: &str,
    root: &Value,
    source_lang: &str,
    units: &mut Vec<TextUnit>,
) -> Result<()> {
    if file_name.starts_with("Map") {
        let events = root
            .get("events")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        for (ei, ev) in events.iter().enumerate() {
            if ev.is_null() {
                continue;
            }
            let event_id = ev.get("id").and_then(|v| v.as_i64()).unwrap_or(ei as i64);
            let pages = ev
                .get("pages")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            for (pi, page) in pages.iter().enumerate() {
                let list = page.get("list").and_then(|v| v.as_array());
                let Some(list) = list else { continue };
                let prefix = format!("{file_name}/{event_id}/{pi}");
                extract_command_list(&prefix, list, source_lang, units);
            }
        }
    } else if file_name == "CommonEvents.json" {
        let arr = root.as_array().cloned().unwrap_or_default();
        for (i, ev) in arr.iter().enumerate() {
            if ev.is_null() {
                continue;
            }
            let id = ev.get("id").and_then(|v| v.as_i64()).unwrap_or(i as i64);
            let list = ev.get("list").and_then(|v| v.as_array());
            let Some(list) = list else { continue };
            let prefix = format!("{file_name}/{id}");
            extract_command_list(&prefix, list, source_lang, units);
        }
    } else if file_name == "Troops.json" {
        let arr = root.as_array().cloned().unwrap_or_default();
        for (i, troop) in arr.iter().enumerate() {
            if troop.is_null() {
                continue;
            }
            let id = troop.get("id").and_then(|v| v.as_i64()).unwrap_or(i as i64);
            let pages = troop
                .get("pages")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            for (pi, page) in pages.iter().enumerate() {
                let list = page.get("list").and_then(|v| v.as_array());
                let Some(list) = list else { continue };
                let prefix = format!("{file_name}/{id}/{pi}");
                extract_command_list(&prefix, list, source_lang, units);
            }
        }
    }
    Ok(())
}

fn extract_command_list(
    prefix: &str,
    list: &[Value],
    source_lang: &str,
    units: &mut Vec<TextUnit>,
) {
    let mut pending_long: Option<(String, String, Vec<String>, Vec<String>)> = None;
    // (location, role, lines, line_paths)
    let mut pending_scroll: Option<(String, Vec<String>, Vec<String>, usize)> = None;

    let flush_long =
        |units: &mut Vec<TextUnit>,
         pending: &mut Option<(String, String, Vec<String>, Vec<String>)>| {
            if let Some((loc, role, lines, paths)) = pending.take() {
                if lines.is_empty() {
                    return;
                }
                if !lines.iter().any(|l| needs_translation(l, source_lang)) {
                    return;
                }
                let id = TextUnit::compute_id("rmmz", &loc, &lines);
                units.push(TextUnit {
                    id,
                    engine: "rmmz".into(),
                    domain: "dialogue".into(),
                    location: loc,
                    item_type: ItemType::LongText,
                    role,
                    original_lines: lines,
                    source_line_paths: paths,
                    context: prefix.to_string(),
                    payload: String::new(),
                });
            }
        };

    let flush_scroll =
        |units: &mut Vec<TextUnit>,
         pending: &mut Option<(String, Vec<String>, Vec<String>, usize)>| {
            if let Some((loc, lines, paths, _)) = pending.take() {
                if lines.is_empty() {
                    return;
                }
                if !lines.iter().any(|l| needs_translation(l, source_lang)) {
                    return;
                }
                let id = TextUnit::compute_id("rmmz", &loc, &lines);
                units.push(TextUnit {
                    id,
                    engine: "rmmz".into(),
                    domain: "scroll".into(),
                    location: loc,
                    item_type: ItemType::LongText,
                    role: "旁白".into(),
                    original_lines: lines,
                    source_line_paths: paths,
                    context: prefix.to_string(),
                    payload: String::new(),
                });
            }
        };

    for (idx, cmd) in list.iter().enumerate() {
        let code = cmd.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
        let params = cmd.get("parameters").cloned().unwrap_or(json!([]));
        let location = format!("{prefix}/{idx}");

        match code {
            CODE_NAME => {
                flush_scroll(units, &mut pending_scroll);
                flush_long(units, &mut pending_long);
                let mut role = "旁白".to_string();
                if let Some(s) = params
                    .as_array()
                    .filter(|a| a.len() >= 5)
                    .and_then(|a| a[4].as_str())
                {
                    let t = s.trim();
                    if !t.is_empty() {
                        role = t.to_string();
                        // MZ namebox (parameters[4]): extract as its own unit so
                        // writeback can translate the speaker plate. Skip \N[n]
                        // actor refs — those resolve at runtime from Actors.json.
                        if needs_translation(t, source_lang) && !is_actor_name_ref(t) {
                            let loc = format!("{location}/namebox");
                            let lines = vec![t.to_string()];
                            let id = TextUnit::compute_id("rmmz", &loc, &lines);
                            units.push(TextUnit {
                                id,
                                engine: "rmmz".into(),
                                domain: "namebox".into(),
                                location: loc.clone(),
                                item_type: ItemType::ShortText,
                                role: "namebox".into(),
                                original_lines: lines,
                                source_line_paths: vec![loc],
                                context: prefix.to_string(),
                                payload: String::new(),
                            });
                        }
                    }
                }
                pending_long = Some((location, role, Vec::new(), Vec::new()));
            }
            CODE_TEXT => {
                flush_scroll(units, &mut pending_scroll);
                let text = params
                    .as_array()
                    .and_then(|a| a.first())
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                if let Some((_, _, lines, paths)) = pending_long.as_mut() {
                    lines.push(text);
                    paths.push(location);
                }
            }
            CODE_CHOICES => {
                flush_scroll(units, &mut pending_scroll);
                flush_long(units, &mut pending_long);
                let choices: Vec<String> = params
                    .as_array()
                    .and_then(|a| a.first())
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_default();
                if choices.iter().any(|l| needs_translation(l, source_lang)) {
                    let id = TextUnit::compute_id("rmmz", &location, &choices);
                    units.push(TextUnit {
                        id,
                        engine: "rmmz".into(),
                        domain: "choices".into(),
                        location: location.clone(),
                        item_type: ItemType::Array,
                        role: "旁白".into(),
                        original_lines: choices,
                        source_line_paths: vec![location],
                        context: prefix.to_string(),
                        payload: String::new(),
                    });
                }
            }
            CODE_SCROLL => {
                flush_long(units, &mut pending_long);
                let text = params
                    .as_array()
                    .and_then(|a| a.first())
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                match pending_scroll.as_mut() {
                    Some((loc, lines, paths, last_idx)) if *last_idx + 1 == idx => {
                        lines.push(text);
                        paths.push(location);
                        *last_idx = idx;
                        let _ = loc;
                    }
                    _ => {
                        flush_scroll(units, &mut pending_scroll);
                        pending_scroll = Some((location.clone(), vec![text], vec![location], idx));
                    }
                }
            }
            _ => {
                flush_scroll(units, &mut pending_scroll);
                flush_long(units, &mut pending_long);
            }
        }
    }
    flush_scroll(units, &mut pending_scroll);
    flush_long(units, &mut pending_long);
}

fn extract_system(system: &Value, source_lang: &str, units: &mut Vec<TextUnit>) {
    if let Some(title) = system.get("gameTitle").and_then(|v| v.as_str()) {
        push_short(units, "System.json/gameTitle", title, source_lang, "system");
    }
    if let Some(terms) = system.get("terms") {
        for key in ["basic", "commands", "params"] {
            if let Some(arr) = terms.get(key).and_then(|v| v.as_array()) {
                for (i, v) in arr.iter().enumerate() {
                    if let Some(s) = v.as_str() {
                        push_short(
                            units,
                            &format!("System.json/terms/{key}/{i}"),
                            s,
                            source_lang,
                            "system",
                        );
                    }
                }
            }
        }
        if let Some(obj) = terms.get("messages").and_then(|v| v.as_object()) {
            for (k, v) in obj {
                if let Some(s) = v.as_str() {
                    push_short(
                        units,
                        &format!("System.json/terms/messages/{k}"),
                        s,
                        source_lang,
                        "system",
                    );
                }
            }
        }
    }
}

fn extract_base(file: &str, arr: &Value, source_lang: &str, units: &mut Vec<TextUnit>) {
    let Some(list) = arr.as_array() else {
        return;
    };
    for item in list {
        if item.is_null() {
            continue;
        }
        let id = item.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
        for field in BASE_FIELDS {
            if let Some(s) = item.get(*field).and_then(|v| v.as_str()) {
                push_short(
                    units,
                    &format!("{file}/{id}/{field}"),
                    s,
                    source_lang,
                    "base",
                );
            }
        }
        // name is often UI-visible; include when source-looking
        if let Some(s) = item.get("name").and_then(|v| v.as_str()) {
            push_short(units, &format!("{file}/{id}/name"), s, source_lang, "base");
        }
    }
}

fn push_short(
    units: &mut Vec<TextUnit>,
    location: &str,
    text: &str,
    source_lang: &str,
    domain: &str,
) {
    let t = text.trim();
    if t.is_empty() || !needs_translation(t, source_lang) {
        return;
    }
    let lines = vec![t.to_string()];
    let id = TextUnit::compute_id("rmmz", location, &lines);
    units.push(TextUnit {
        id,
        engine: "rmmz".into(),
        domain: domain.into(),
        location: location.into(),
        item_type: ItemType::ShortText,
        role: "旁白".into(),
        original_lines: lines,
        source_line_paths: vec![location.into()],
        context: domain.into(),
        payload: String::new(),
    });
}

fn apply_unit(
    files: &mut BTreeMap<String, Value>,
    unit: &TextUnit,
    translation: Option<&Translation>,
) -> Result<()> {
    let file = unit
        .location
        .split('/')
        .next()
        .ok_or_else(|| anyhow::anyhow!("bad location {}", unit.location))?;
    let root = files
        .get_mut(file)
        .ok_or_else(|| anyhow::anyhow!("file not loaded: {file}"))?;
    let lines = translation.map_or(unit.original_lines.as_slice(), |tr| tr.translation_lines.as_slice());

    match unit.item_type {
        ItemType::Array => {
            write_choices(root, &unit.location, lines)?;
        }
        ItemType::ShortText if unit.domain == "namebox" => {
            write_namebox(root, &unit.location, lines)?;
        }
        ItemType::ShortText => {
            write_short(root, &unit.location, lines)?;
        }
        ItemType::LongText => {
            write_long_text_with_reflow(root, unit, lines, translation.is_some())?;
        }
    }
    Ok(())
}

fn write_short(root: &mut Value, location: &str, lines: &[String]) -> Result<()> {
    let text = lines.first().cloned().unwrap_or_default();
    // location like System.json/gameTitle or Actors.json/1/name
    let rest = location.split_once('/').map(|(_, r)| r).unwrap_or("");
    if rest.is_empty() {
        bail!("short path missing fields: {location}");
    }
    let slot = navigate_mut(root, rest)?;
    if !slot.is_string() {
        bail!("short text anchor is not a string: {location}");
    }
    *slot = Value::String(text);
    Ok(())
}

fn write_namebox(root: &mut Value, location: &str, lines: &[String]) -> Result<()> {
    let text = lines.first().cloned().unwrap_or_default();
    let rest = location.split_once('/').map(|(_, r)| r).unwrap_or("");
    let rest = rest.strip_suffix("/namebox").unwrap_or(rest);
    if rest.is_empty() {
        bail!("namebox path missing command: {location}");
    }
    let cmd = navigate_mut(root, rest)?;
    let code = cmd.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
    if code != CODE_NAME {
        bail!("expected namebox code 101 at {location}, got {code}");
    }
    let params = cmd
        .get_mut("parameters")
        .and_then(|v| v.as_array_mut())
        .ok_or_else(|| anyhow::anyhow!("namebox missing parameters at {location}"))?;
    if params.get(4).and_then(Value::as_str).is_none() {
        bail!("namebox string anchor missing at {location}");
    }
    params[4] = Value::String(text);
    Ok(())
}

fn write_choices(root: &mut Value, location: &str, lines: &[String]) -> Result<()> {
    let rest = location.split_once('/').map(|(_, r)| r).unwrap_or("");
    // navigate to command object
    let cmd = navigate_mut(root, rest)?;
    if cmd.get("code").and_then(Value::as_i64) != Some(CODE_CHOICES) {
        bail!("expected choices code 102 at {location}");
    }
    let params = cmd
        .get_mut("parameters")
        .and_then(|v| v.as_array_mut())
        .ok_or_else(|| anyhow::anyhow!("choices missing parameters at {location}"))?;
    let choices = params.first().and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("choices array anchor missing at {location}"))?;
    if choices.len() != lines.len() || choices.iter().any(|choice| !choice.is_string()) {
        bail!("choices slot shape changed at {location}");
    }
    params[0] = Value::Array(lines.iter().cloned().map(Value::String).collect());
    Ok(())
}

#[cfg(test)]
fn write_long_text(root: &mut Value, unit: &TextUnit, lines: &[String]) -> Result<()> {
    write_long_text_with_reflow(root, unit, lines, true)
}

fn write_long_text_with_reflow(root: &mut Value, unit: &TextUnit, lines: &[String], reflow: bool) -> Result<()> {
    if unit.source_line_paths.is_empty() {
        bail!("missing source slots at {}", unit.location);
    }
    // ponytail: never insert/delete event commands (shifts later indices).
    // Fit translation into the original number of 401/405 slots.
    let n_src = unit.source_line_paths.len();
    let fitted = if reflow {
        fitted_unit_lines(unit, lines, n_src, DEFAULT_MSG_WIDTH)
    } else {
        if lines.len() != n_src {
            bail!("original line count does not match source slots at {}", unit.location);
        }
        Cow::Borrowed(lines)
    };

    for (path, text) in unit.source_line_paths.iter().zip(fitted.iter()) {
        let rest = path.split_once('/').map(|(_, r)| r).unwrap_or("");
        let cmd = navigate_mut(root, rest)?;
        let code = cmd.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
        if code != CODE_TEXT && code != CODE_SCROLL {
            bail!("expected text/scroll code at {path}, got {code}");
        }
        let params = cmd
            .get_mut("parameters")
            .and_then(|v| v.as_array_mut())
            .ok_or_else(|| anyhow::anyhow!("missing parameters at {path}"))?;
        if params.first().and_then(Value::as_str).is_none() {
            bail!("missing text string anchor at {path}");
        }
        params[0] = Value::String(text.clone());
    }
    Ok(())
}

/// Default message window display width (half-width cells). CJK ≈ 2.
const DEFAULT_MSG_WIDTH: usize = 44;

/// Changes to the physical message slots, shared by caching and rendering.
#[derive(Default, Debug)]
pub struct LayoutChange {
    pub reflowed: bool,
    /// Number of emitted slots wider than the default 44 half-width cells.
    /// Overflow is retained, not truncated, when the available slots cannot fit it.
    pub overflow_lines: usize,
}

/// Normalize an RPG Maker message before rendering or saving its translation.
/// Repeated calls leave fitted lines unchanged, including unavoidable overflow.
pub fn normalize_for_writeback(unit: &TextUnit, translation: &mut Translation) -> LayoutChange {
    if unit.engine != "rmmz"
        || unit.item_type != ItemType::LongText
        || unit.source_line_paths.is_empty()
        || translation.translation_lines.is_empty()
    {
        return LayoutChange::default();
    }
    let fitted = fitted_unit_lines(
        unit,
        &translation.translation_lines,
        unit.source_line_paths.len(),
        DEFAULT_MSG_WIDTH,
    );
    let overflow_lines = fitted.iter().filter(|line| display_width(line) > DEFAULT_MSG_WIDTH).count();
    let reflowed = fitted.as_ref() != translation.translation_lines.as_slice();
    if reflowed {
        translation.translation_lines = fitted.into_owned();
    }
    LayoutChange { reflowed, overflow_lines }
}

fn standalone_name(text: &str) -> Option<&str> {
    let text = text.trim();
    let body = text.strip_prefix('【')?.strip_suffix('】')?;
    (!body.is_empty() && !body.contains(['【', '】', '\n'])).then_some(text)
}

/// Accept a collapsed label only where the source reserved a name slot.
/// Leading controls belong to the body, never to the protected label.
fn collapsed_name(text: &str) -> Option<(&str, &str, &str)> {
    let mut start = 0;
    while start < text.len() {
        let ch = text[start..].chars().next()?;
        if ch.is_whitespace() {
            start += ch.len_utf8();
        } else if let Some(end) = control_token_end(text, start) {
            start = end;
        } else {
            break;
        }
    }
    let tail = text[start..].strip_prefix('【')?;
    let end = start + '【'.len_utf8() + tail.find('】')? + '】'.len_utf8();
    let name = standalone_name(&text[start..end])?;
    Some((name, &text[..start], &text[end..]))
}

fn fitted_unit_lines<'a>(
    unit: &TextUnit,
    lines: &'a [String],
    n: usize,
    max_w: usize,
) -> Cow<'a, [String]> {
    if n < 2 || lines.is_empty() {
        return fit_lines_borrowed(lines, n, max_w);
    }
    let source_name = unit.original_lines.first().and_then(|line| standalone_name(line));
    let translated_name = lines.first().and_then(|line| standalone_name(line));
    if let Some(name) = translated_name {
        let body = fit_lines_borrowed(&lines[1..], n - 1, max_w);
        if matches!(&body, Cow::Borrowed(_)) && lines[0] == name {
            return Cow::Borrowed(lines);
        }
        let mut out = Vec::with_capacity(n);
        out.push(name.to_owned());
        out.extend(body.into_owned());
        return Cow::Owned(out);
    }
    if let Some(source_name) = source_name {
        let mut body = Vec::with_capacity(lines.len());
        let name = if let Some((name, prefix, suffix)) = collapsed_name(&lines[0]) {
            let mut first_body = String::with_capacity(prefix.len() + suffix.len());
            first_body.push_str(prefix);
            first_body.push_str(suffix);
            if !first_body.is_empty() {
                body.push(first_body);
            }
            body.extend_from_slice(&lines[1..]);
            name
        } else {
            body.extend_from_slice(lines);
            source_name
        };
        let fitted_body = fit_lines_borrowed(&body, n - 1, max_w);
        let mut out = Vec::with_capacity(n);
        out.push(name.to_owned());
        out.extend(fitted_body.into_owned());
        return Cow::Owned(out);
    }
    fit_lines_borrowed(lines, n, max_w)
}

#[cfg(test)]
fn fit_lines_with_width(lines: &[String], n: usize, max_w: usize) -> Vec<String> {
    fit_lines_borrowed(lines, n, max_w).into_owned()
}

/// Keep safe boundaries and reflow only the suffix needing more room.
/// The final slot may overflow; event command indices never move.
fn fit_lines_borrowed(lines: &[String], n: usize, max_w: usize) -> Cow<'_, [String]> {
    if n == 0 {
        return Cow::Owned(Vec::new());
    }
    let mut reflow_from = None;
    for (i, line) in lines.iter().enumerate().take(n.saturating_sub(1)) {
        if display_width(line) > max_w
            || lines.get(i + 1).is_some_and(|next| starts_closing_punctuation(next))
        {
            reflow_from = Some(i);
            break;
        }
    }
    if let Some(start) = reflow_from {
        let mut merged = String::with_capacity(lines[start..].iter().map(String::len).sum());
        for line in &lines[start..] {
            merged.push_str(line);
        }
        let mut out = Vec::with_capacity(n);
        out.extend_from_slice(&lines[..start]);
        out.extend(reflow_to_n(&merged, n - start, max_w));
        return Cow::Owned(out);
    }
    if lines.len() == n {
        return Cow::Borrowed(lines);
    }
    let mut out = Vec::with_capacity(n);
    out.extend_from_slice(&lines[..lines.len().min(n - 1)]);
    if lines.len() >= n {
        let mut last = String::with_capacity(lines[n - 1..].iter().map(String::len).sum());
        for line in &lines[n - 1..] {
            last.push_str(line);
        }
        out.push(last);
    }
    out.resize(n, String::new());
    Cow::Owned(out)
}

fn is_actor_name_ref(s: &str) -> bool {
    let s = s.trim();
    // \N[1] / \n[1] — party member name escape in the namebox
    let bytes = s.as_bytes();
    if bytes.len() < 4 || bytes[0] != b'\\' {
        return false;
    }
    let rest = &s[1..];
    let rest = rest
        .strip_prefix('N')
        .or_else(|| rest.strip_prefix('n'))
        .unwrap_or("");
    rest.starts_with('[')
        && rest.ends_with(']')
        && rest.len() > 2
        && rest[1..rest.len() - 1].chars().all(|c| c.is_ascii_digit())
}

fn display_width(s: &str) -> usize {
    let mut w = 0usize;
    let mut i = 0;
    let b = s.as_bytes();
    while i < b.len() {
        if b[i] == b'\\' && let Some(end) = control_token_end(s, i) {
                i = end;
                continue;
        }
        let ch = s[i..].chars().next().unwrap();
        w += if spawns_half_width(ch) {
            1
        } else {
            2
        };
        i += ch.len_utf8();
    }
    w
}

fn spawns_half_width(ch: char) -> bool {
    (ch as u32) < 128
}

/// End offset of an RPG Maker control code starting at `i` (`s.as_bytes()[i]==b'\\'`),
/// or None if it is a lone backslash.
fn control_token_end(s: &str, i: usize) -> Option<usize> {
    let b = s.as_bytes();
    if i >= b.len() || b[i] != b'\\' {
        return None;
    }
    let rest = &s[i + 1..];
    if rest.is_empty() {
        return None;
    }
    // Multi-letter codes with bracket args: \C[n] \I[n] \N[n] \V[n] \S[n] ...
    // Also short codes: \. \| \! \> \< \^ \\ \{ \} \$
    let first = rest.chars().next()?;
    if matches!(
        first,
        '.' | '|' | '!' | '>' | '<' | '^' | '\\' | '{' | '}' | '$'
    ) {
        return Some(i + 1 + first.len_utf8());
    }
    // Letter + optional [args]
    let mut j = i + 1;
    while j < b.len() && b[j].is_ascii_alphabetic() {
        j += 1;
    }
    if j < b.len() && b[j] == b'[' && let Some(close) = s[j + 1..].find(']') {
            return Some(j + 1 + close + 1);
    }
    if j > i + 1 {
        return Some(j);
    }
    None
}

fn is_closing_punctuation(ch: char) -> bool {
    matches!(ch, '，' | '。' | '！' | '？' | '、' | '」' | '』' | '；' | '：' | '…'
        | ',' | '.' | '!' | '?' | ';' | ':' | ')' | '）' | '】' | ']' | '}')
}

fn starts_closing_punctuation(text: &str) -> bool {
    let mut i = 0;
    while i < text.len() {
        if let Some(end) = control_token_end(text, i) {
            i = end;
            continue;
        }
        let ch = text[i..].chars().next().unwrap();
        if !ch.is_whitespace() {
            return is_closing_punctuation(ch);
        }
        i += ch.len_utf8();
    }
    false
}

struct TokenSpan {
    start: usize,
    end: usize,
    width: usize,
    visible: Option<char>,
    next_is_closing: bool,
}

/// Each token borrows a byte span; no character-sized strings are allocated.
fn tokenize_controls(text: &str) -> Vec<TokenSpan> {
    let mut tokens = Vec::new();
    let mut start = 0;
    while start < text.len() {
        if let Some(end) = control_token_end(text, start) {
            tokens.push(TokenSpan { start, end, width: 0, visible: None, next_is_closing: false });
            start = end;
        } else {
            let ch = text[start..].chars().next().unwrap();
            let end = start + ch.len_utf8();
            tokens.push(TokenSpan {
                start,
                end,
                width: if spawns_half_width(ch) { 1 } else { 2 },
                visible: Some(ch),
                next_is_closing: false,
            });
            start = end;
        }
    }
    let mut next_is_closing = false;
    for token in tokens.iter_mut().rev() {
        token.next_is_closing = next_is_closing;
        if let Some(ch) = token.visible
            && !ch.is_whitespace()
        {
            next_is_closing = is_closing_punctuation(ch);
        }
    }
    tokens
}

fn reflow_to_n(text: &str, n: usize, max_w: usize) -> Vec<String> {
    if n == 0 {
        return Vec::new();
    }
    let text = if text.contains('\n') {
        Cow::Owned(text.replace('\n', ""))
    } else {
        Cow::Borrowed(text)
    };
    if n == 1 {
        return vec![text.into_owned()];
    }
    let tokens = tokenize_controls(&text);
    let mut remaining_width: usize = tokens.iter().map(|token| token.width).sum();
    let mut lines = Vec::with_capacity(n);
    let mut start = 0;
    while start < tokens.len() {
        if lines.len() + 1 == n {
            lines.push(text[tokens[start].start..].to_owned());
            break;
        }
        let mut width = 0;
        let mut latest_safe = None;
        let mut latest_punctuation = None;
        let mut end = start;
        while end < tokens.len() && width + tokens[end].width <= max_w {
            let token = &tokens[end];
            width += token.width;
            if let Some(ch) = token.visible
                && !token.next_is_closing
            {
                latest_safe = Some((end + 1, width));
                if is_closing_punctuation(ch) {
                    latest_punctuation = latest_safe;
                }
            }
            end += 1;
        }
        if end == tokens.len() {
            lines.push(text[tokens[start].start..].to_owned());
            break;
        }
        let remaining_capacity = (n - lines.len() - 1).saturating_mul(max_w);
        let preferred = latest_punctuation.filter(|(_, used_width)| {
            remaining_width.saturating_sub(*used_width) <= remaining_capacity
                || remaining_width > (n - lines.len()).saturating_mul(max_w)
        });
        let Some((split, used_width)) = preferred.or(latest_safe) else {
            // No width-bounded split can keep closing punctuation with its text.
            // Preserve the remainder instead of losing content or splitting controls.
            while lines.len() + 1 < n {
                lines.push(String::new());
            }
            lines.push(text[tokens[start].start..].to_owned());
            break;
        };
        lines.push(text[tokens[start].start..tokens[split - 1].end].to_owned());
        remaining_width = remaining_width.saturating_sub(used_width);
        start = split;
    }
    lines.resize(n, String::new());
    lines
}

fn navigate_mut<'a>(root: &'a mut Value, compact_rest: &str) -> Result<&'a mut Value> {
    // compact_rest forms:
    // Map: {eventId}/{pageIndex}/{cmdIndex}  → events[i] where id==eventId, pages[page], list[cmd]
    // CommonEvents: {id}/{cmdIndex} → array item id, list[cmd]
    // Troops: {id}/{page}/{cmd}
    // System/base: field path as-is
    let parts: Vec<&str> = compact_rest.split('/').filter(|p| !p.is_empty()).collect();
    if parts.is_empty() {
        bail!("empty path");
    }

    // Heuristic: if root is object with "events", treat as Map
    if root.get("events").is_some() {
        return navigate_map(root, &parts);
    }
    if root.is_array() {
        return navigate_array_db(root, &parts);
    }
    // System-like object path
    let mut cur = root;
    for p in parts {
        if let Ok(idx) = p.parse::<usize>() {
            cur = cur
                .as_array_mut()
                .and_then(|a| a.get_mut(idx))
                .ok_or_else(|| anyhow::anyhow!("nav array {p}"))?;
        } else {
            cur = cur
                .as_object_mut()
                .and_then(|o| o.get_mut(p))
                .ok_or_else(|| anyhow::anyhow!("nav key {p}"))?;
        }
    }
    Ok(cur)
}

fn navigate_map<'a>(root: &'a mut Value, parts: &[&str]) -> Result<&'a mut Value> {
    // parts: eventId, pageIndex, cmdIndex?  OR eventId, pageIndex for list
    if parts.len() < 2 {
        bail!("map path too short");
    }
    let event_id: i64 = parts[0].parse().context("event id")?;
    let page_index: usize = parts[1].parse().context("page index")?;
    let events = root
        .get_mut("events")
        .and_then(|v| v.as_array_mut())
        .ok_or_else(|| anyhow::anyhow!("map missing events"))?;
    let mut event_idx = None;
    for (i, ev) in events.iter().enumerate() {
        if ev.get("id").and_then(|v| v.as_i64()) == Some(event_id) {
            event_idx = Some(i);
            break;
        }
    }
    let event_idx = event_idx.ok_or_else(|| anyhow::anyhow!("event {event_id} not found"))?;
    let pages = events[event_idx]
        .get_mut("pages")
        .and_then(|v| v.as_array_mut())
        .ok_or_else(|| anyhow::anyhow!("missing pages"))?;
    if page_index >= pages.len() {
        bail!("page {page_index} OOB");
    }
    if parts.len() == 2 {
        // return list
        return pages[page_index]
            .get_mut("list")
            .ok_or_else(|| anyhow::anyhow!("missing list"));
    }
    let cmd_index: usize = parts[2].parse().context("cmd index")?;
    let list = pages[page_index]
        .get_mut("list")
        .and_then(|v| v.as_array_mut())
        .ok_or_else(|| anyhow::anyhow!("missing list"))?;
    list.get_mut(cmd_index)
        .ok_or_else(|| anyhow::anyhow!("cmd {cmd_index} OOB"))
}

fn navigate_array_db<'a>(root: &'a mut Value, parts: &[&str]) -> Result<&'a mut Value> {
    // CommonEvents: id  | id/cmdIndex  → event or command
    // Special: when writing inserts/deletes we pass only "id" and need the list array.
    // Troops: id/page | id/page/cmd
    // Base: id/field
    let id: i64 = parts[0].parse().context("db id")?;
    let arr = root
        .as_array_mut()
        .ok_or_else(|| anyhow::anyhow!("expected array db"))?;
    let mut idx = None;
    for (i, item) in arr.iter().enumerate() {
        if item.get("id").and_then(|v| v.as_i64()) == Some(id) {
            idx = Some(i);
            break;
        }
    }
    let idx = idx.ok_or_else(|| anyhow::anyhow!("id {id} not found"))?;

    // parts == [id] → prefer command list if present (writeback insert/delete)
    if parts.len() == 1 {
        if arr[idx].get("list").is_some() {
            return arr[idx]
                .get_mut("list")
                .ok_or_else(|| anyhow::anyhow!("no list"));
        }
        return Ok(&mut arr[idx]);
    }

    if arr[idx].get("pages").is_some() {
        let page: usize = parts[1].parse().context("troop page")?;
        let pages = arr[idx]
            .get_mut("pages")
            .and_then(|v| v.as_array_mut())
            .ok_or_else(|| anyhow::anyhow!("no pages"))?;
        if page >= pages.len() {
            bail!("page {page} OOB");
        }
        if parts.len() == 2 {
            return pages[page]
                .get_mut("list")
                .ok_or_else(|| anyhow::anyhow!("no list"));
        }
        let cmd: usize = parts[2].parse().context("cmd")?;
        let list = pages[page]
            .get_mut("list")
            .and_then(|v| v.as_array_mut())
            .ok_or_else(|| anyhow::anyhow!("no list"))?;
        return list.get_mut(cmd).ok_or_else(|| anyhow::anyhow!("cmd OOB"));
    }

    if arr[idx].get("list").is_some() {
        // CommonEvents: id/cmd
        if parts.len() == 2
            && let Ok(cmd) = parts[1].parse::<usize>()
        {
            let list = arr[idx]
                .get_mut("list")
                .and_then(|v| v.as_array_mut())
                .ok_or_else(|| anyhow::anyhow!("no list"))?;
            return list.get_mut(cmd).ok_or_else(|| anyhow::anyhow!("cmd OOB"));
        }
    }

    // base field id/name etc.
    let field = parts[1];
    arr[idx]
        .as_object_mut()
        .and_then(|o| o.get_mut(field))
        .ok_or_else(|| anyhow::anyhow!("missing field {field}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Translation;
    use serde_json::json;

    #[test]
    fn namebox_extracted_and_written_back() {
        let list = json!([
            {"code": 101, "parameters": ["Actor1", 0, 0, 2, "エレノア"]},
            {"code": 401, "parameters": ["こんにちは。"]},
            {"code": 101, "parameters": ["", 0, 0, 2, "\\N[1]"]},
            {"code": 401, "parameters": ["はい。"]},
            {"code": 0, "parameters": []}
        ]);
        let mut units = Vec::new();
        extract_command_list("Map001.json/1/0", list.as_array().unwrap(), "ja", &mut units);

        let nameboxes: Vec<_> = units.iter().filter(|u| u.domain == "namebox").collect();
        assert_eq!(nameboxes.len(), 1, "units={units:?}");
        assert_eq!(nameboxes[0].original_lines, vec!["エレノア".to_string()]);
        assert!(
            !units.iter().any(|u| u.original_lines.iter().any(|l| l.contains("\\N["))),
            "\\N[n] must not become a namebox unit"
        );
        assert!(
            units.iter().any(|u| u.domain == "dialogue" && u.role == "エレノア"),
            "dialogue still carries namebox as role"
        );

        let mut root = json!({"events": [null, {
            "id": 1,
            "pages": [{"list": list}]
        }]});
        let u = nameboxes[0];
        let tr = Translation {
            unit_id: u.id.clone(),
            translation_lines: vec!["埃莉诺".into()],
            source_hash: String::new(),
            passthrough: false,
        };
        write_namebox(&mut root, &u.location, &tr.translation_lines).unwrap();
        let cmd = &root["events"][1]["pages"][0]["list"][0];
        assert_eq!(cmd["parameters"][4], json!("埃莉诺"));
        assert_eq!(
            root["events"][1]["pages"][0]["list"][1]["parameters"][0],
            json!("こんにちは。"),
            "body untouched"
        );
    }

    #[test]
    fn fit_lines_reflows_long_cjk_into_slots() {
        let long = "上午的课就先到这里。下午会进行结合实践的课程，请大家到训练场集合。".to_string();
        let out = fit_lines_with_width(std::slice::from_ref(&long), 3, 44);
        assert_eq!(out.len(), 3);
        assert!(out.iter().filter(|l| !l.is_empty()).count() >= 2, "out={out:?}");
        assert!(
            out[..2].iter().all(|l| display_width(l) <= 44),
            "non-last lines within width: {out:?} widths={:?}",
            out.iter().map(|l| display_width(l)).collect::<Vec<_>>()
        );
        assert_eq!(out.join(""), long.replace('\n', ""));
    }

    #[test]
    fn fit_lines_keeps_equal_width_safe_lines() {
        let lines = vec!["短い一行。".into(), "もう一行。".into(), "三行目。".into()];
        let out = fit_lines_with_width(&lines, 3, 44);
        assert_eq!(out, lines);
    }

    #[test]
    fn fit_lines_does_not_split_control_codes() {
        let s = "\\C[27]你好世界，这是一段比较长的测试文本用来检查控制符。".to_string();
        let out = fit_lines_with_width(&[s], 2, 20);
        assert!(out.iter().any(|l| l.contains("\\C[27]")), "out={out:?}");
        assert!(!out.iter().any(|l| l.contains("\\C[2") && !l.contains("\\C[27]")));
    }

    fn layout_unit(source: &[&str], slots: usize) -> TextUnit {
        TextUnit {
            id: "report-layout".into(),
            engine: "rmmz".into(),
            domain: "dialogue".into(),
            location: "Map040.json/7/0/315".into(),
            item_type: ItemType::LongText,
            role: "旁白".into(),
            original_lines: source.iter().map(|line| (*line).to_owned()).collect(),
            source_line_paths: (315..315 + slots).map(|i| format!("Map040.json/7/0/{i}")).collect(),
            context: String::new(),
            payload: String::new(),
        }
    }

    fn layout_translation(lines: &[&str]) -> Translation {
        Translation {
            unit_id: "report-layout".into(),
            translation_lines: lines.iter().map(|line| (*line).to_owned()).collect(),
            source_hash: "unchanged-source-hash".into(),
            passthrough: false,
        }
    }

    #[test]
    fn report_name_slot_and_expression_survive_two_slot_overflow() {
        let unit = layout_unit(&[
            "【ラージ】",
            r"\SE[magao]「いい話なのか不憫な話なのかわからないな……」",
        ], 2);
        let mut tr = layout_translation(&[
            "【拉吉】",
            r"\SE[magao]「这算温馨往事还是可怜的故事我都分不清了……」",
        ]);
        let expected = tr.translation_lines.clone();
        let change = normalize_for_writeback(&unit, &mut tr);
        assert!(!change.reflowed);
        assert_eq!(change.overflow_lines, 1);
        assert_eq!(tr.translation_lines, expected);

        let mut list = vec![json!({"code": 0, "indent": 0, "parameters": []}); 318];
        list[314] = json!({"code": 101, "indent": 0, "parameters": ["", 0, 0, 2]});
        list[315] = json!({"code": 401, "indent": 0, "parameters": [unit.original_lines[0]]});
        list[316] = json!({"code": 401, "indent": 0, "parameters": [unit.original_lines[1]]});
        let original_commands = list.clone();
        let mut events = vec![Value::Null; 8];
        events[7] = json!({"id": 7, "pages": [{"list": list}]});
        let mut root = json!({"events": events});
        write_long_text(&mut root, &unit, &tr.translation_lines).unwrap();
        let written = root["events"][7]["pages"][0]["list"].as_array().unwrap();
        assert_eq!(written.len(), original_commands.len());
        assert_eq!(written[315]["parameters"][0], json!("【拉吉】"));
        assert_eq!(written[316]["parameters"][0], json!(expected[1]));
        assert!(written[316]["parameters"][0].as_str().unwrap().starts_with(r"\SE[magao]"));
        for (i, (before, after)) in original_commands.iter().zip(written).enumerate() {
            assert_eq!(before["code"], after["code"], "command index {i}");
            assert_eq!(before["indent"], after["indent"], "command index {i}");
            if i != 315 && i != 316 {
                assert_eq!(before, after, "unrelated command index {i}");
            }
        }
        let expected_root = root.clone();
        let second = normalize_for_writeback(&unit, &mut tr);
        assert!(!second.reflowed);
        assert_eq!(second.overflow_lines, 1);
        write_long_text(&mut root, &unit, &tr.translation_lines).unwrap();
        assert_eq!(root, expected_root);
    }

    #[test]
    fn source_name_slot_recovers_collapsed_or_missing_translated_label() {
        let unit = layout_unit(&["【ラージ】", "こんにちは。"], 2);
        for collapsed in [
            r"【拉吉】\SE[magao]「你好。」",
            r"\SE[magao]【拉吉】「你好。」",
        ] {
            let mut tr = layout_translation(&[collapsed]);
            assert!(normalize_for_writeback(&unit, &mut tr).reflowed);
            assert_eq!(tr.translation_lines, vec!["【拉吉】", r"\SE[magao]「你好。」"]);
            assert!(!normalize_for_writeback(&unit, &mut tr).reflowed);
        }
        let mut missing = layout_translation(&[r"\SE[magao]「你好。」"]);
        assert!(normalize_for_writeback(&unit, &mut missing).reflowed);
        assert_eq!(missing.translation_lines, vec!["【ラージ】", r"\SE[magao]「你好。」"]);
        assert!(!normalize_for_writeback(&unit, &mut missing).reflowed);

        let mismatch = layout_unit(&["名前のない台詞。", "次の行。"], 2);
        let mut named = layout_translation(&["【拉吉】", r"\SE[magao]「这是一句超过消息窗口宽度而且不能挤进名字行的非常长的台词。」"]);
        assert!(!normalize_for_writeback(&mismatch, &mut named).reflowed);
        assert_eq!(named.translation_lines[0], "【拉吉】");
    }

    #[test]
    fn latest_punctuation_break_wins_over_punctuation_type_order() {
        let text = "甲。乙，丙丁戊己";
        let out = reflow_to_n(text, 2, 12);
        assert_eq!(out, vec!["甲。乙，", "丙丁戊己"]);
        assert_eq!(out.join(""), text);
    }

    #[test]
    fn closing_punctuation_never_starts_a_reflowed_line() {
        for punctuation in ['，', '。', '！', '？', '、', '」', '』'] {
            let text = format!("甲乙{punctuation}丙丁");
            let out = reflow_to_n(&text, 2, 4);
            assert_eq!(out.join(""), text);
            assert_eq!(out[0], "甲");
            assert!(!starts_closing_punctuation(&out[1]));
        }
        let out = reflow_to_n("甲。！？乙", 2, 2);
        assert_eq!(out, vec!["", "甲。！？乙"]);
    }

    #[test]
    fn control_spans_are_atomic_and_remain_with_body() {
        let text = r"\SE[magao]甲乙\C[27]丙丁\I[12]戊己\V[999]庚辛壬癸";
        let out = reflow_to_n(text, 4, 6);
        assert_eq!(out.join(""), text);
        for control in [r"\SE[magao]", r"\C[27]", r"\I[12]", r"\V[999]"] {
            assert_eq!(out.iter().filter(|line| line.contains(control)).count(), 1);
        }
        let unit = layout_unit(&["【ラージ】", "一行。", "二行。", "三行。", "四行。"], 5);
        let mut tr = layout_translation(&["【拉吉】", text]);
        assert!(normalize_for_writeback(&unit, &mut tr).reflowed);
        assert_eq!(tr.translation_lines[0], "【拉吉】");
        assert_eq!(tr.translation_lines[1..].join(""), text);
        assert!(!normalize_for_writeback(&unit, &mut tr).reflowed);
    }

    #[test]
    fn early_punctuation_does_not_force_avoidable_last_slot_overflow() {
        let text = "甲。乙丙丁戊己庚辛壬";
        let out = reflow_to_n(text, 2, 12);
        assert_eq!(out.join(""), text);
        assert!(out.iter().all(|line| display_width(line) <= 12), "out={out:?}");
        assert_eq!(out[0], "甲。乙丙丁戊");
    }

    #[test]
    fn name_is_not_reused_when_body_exceeds_multiple_available_slots() {
        let unit = layout_unit(&["【ラージ】", "一行。", "二行。", "三行。"], 4);
        let text = r"\SE[magao]甲乙丙丁戊己庚辛壬癸".repeat(12);
        let mut tr = layout_translation(&["【拉吉】", &text]);
        let first = normalize_for_writeback(&unit, &mut tr);
        assert!(first.reflowed);
        assert_eq!(first.overflow_lines, 1);
        assert_eq!(tr.translation_lines[0], "【拉吉】");
        assert!(tr.translation_lines[1].starts_with(r"\SE[magao]"));
        assert_eq!(tr.translation_lines[1..].join(""), text);
        let fitted = tr.translation_lines.clone();
        let second = normalize_for_writeback(&unit, &mut tr);
        assert!(!second.reflowed);
        assert_eq!(second.overflow_lines, 1);
        assert_eq!(tr.translation_lines, fitted);
    }

    #[test]
    fn safe_prefix_boundaries_and_unavoidable_final_overflow_are_idempotent() {
        let unit = layout_unit(&["一行。", "二行。", "三行。"], 3);
        let long = "很长的台词需要重新分行但是最后一行超出窗口宽度时仍必须完整保存内容不能截断或者丢失任何字句。";
        let mut tr = layout_translation(&["保留这一行。", long]);
        assert!(normalize_for_writeback(&unit, &mut tr).reflowed);
        assert_eq!(tr.translation_lines[0], "保留这一行。");
        assert_eq!(tr.translation_lines[1..].join(""), long);
        let expected = tr.translation_lines.clone();
        let second = normalize_for_writeback(&unit, &mut tr);
        assert!(!second.reflowed);
        assert_eq!(tr.translation_lines, expected);
        assert_eq!(fitted_unit_lines(&unit, &tr.translation_lines, 3, DEFAULT_MSG_WIDTH).as_ref(), expected.as_slice());
        assert_eq!(tr.source_hash, "unchanged-source-hash");
    }

    #[test]
    fn reextract_uses_first_write_json_backups_and_explicit_origin() {
        let root = super::super::test_dir("rmmz-source-backups");
        let data = root.join("data");
        fs::create_dir_all(&data).unwrap();
        fs::write(data.join("System.json"), "{}").unwrap();
        let map = |text: &str| json!({"events": [null, {"id": 1, "pages": [{"list": [
            {"code": 101, "parameters": ["", 0, 0, 2]},
            {"code": 401, "parameters": [text]},
            {"code": 0, "parameters": []}
        ]}]}]});
        fs::write(data.join("Map001.json"), map("你好。").to_string()).unwrap();
        fs::write(data.join("Map001.json.attxbak"), map("こんにちは。").to_string()).unwrap();
        fs::write(data.join("Actors.json"), json!([null, {"id": 1, "name": "拉吉"}]).to_string()).unwrap();
        fs::write(data.join("Actors.json.attxbak"), json!([null, {"id": 1, "name": "ラージ"}]).to_string()).unwrap();
        let units = RmmzAdapter.extract(&root, "ja").unwrap();
        assert!(units.iter().any(|u| u.original_lines == vec!["こんにちは。"]));
        assert!(units.iter().any(|u| u.original_lines == vec!["ラージ"]));
        assert!(!units.iter().any(|u| u.original_lines == vec!["你好。"]));
        let origin = root.join("data_origin");
        fs::create_dir_all(&origin).unwrap();
        fs::write(origin.join("System.json"), "{}").unwrap();
        fs::write(origin.join("Map001.json"), map("元の台詞。").to_string()).unwrap();
        fs::write(origin.join("Map001.json.attxbak"), map("古い台詞。").to_string()).unwrap();
        let units = RmmzAdapter.extract(&root, "ja").unwrap();
        assert!(units.iter().any(|u| u.original_lines == vec!["元の台詞。"]));
        assert!(!units.iter().any(|u| u.original_lines == vec!["古い台詞。"]));
        fs::remove_dir_all(root).unwrap();
    }

    fn repeat_fixture(name: &str) -> (PathBuf, Value) {
        let root = super::super::test_dir(name);
        fs::create_dir_all(root.join("data")).unwrap();
        fs::write(root.join("data/System.json"), json!({"gameTitle": "物語", "currencyUnit": "G"}).to_string()).unwrap();
        fs::write(root.join("data/Actors.json"), json!([null, {"id": 7, "name": "ラージ", "level": 1}]).to_string()).unwrap();
        let source = json!({"displayName": "unchanged", "events": [null, {"id": 1, "name": "Scene", "x": 3,
            "pages": [{"conditions": {"switch1Valid": false}, "list": [
                {"code": 101, "indent": 0, "parameters": ["", 0, 0, 2, "ラージ"]},
                {"code": 401, "indent": 0, "parameters": ["【ラージ】"]},
                {"code": 401, "indent": 0, "parameters": [format!("\\SE[magao]{}", "原文の長い台詞。".repeat(12))]},
                {"code": 401, "indent": 0, "parameters": ["」次の行。"]},
                {"code": 102, "indent": 0, "parameters": [["はい", "いいえ"], 0, 0, 2, 0]},
                {"code": 405, "indent": 0, "parameters": ["巻物の一行。"]},
                {"code": 405, "indent": 0, "parameters": ["巻物の二行。"]},
                {"code": 0, "indent": 0, "parameters": []}
            ]}]}]});
        fs::write(root.join("data/Map001.json"), source.to_string()).unwrap();
        (root, source)
    }

    fn publish_test_outputs(outputs: Vec<OutputFile>) {
        for output in outputs {
            let mut backup = output.path.as_os_str().to_os_string();
            backup.push(".attxbak");
            if !Path::new(&backup).exists() { fs::copy(&output.path, &backup).unwrap(); }
            fs::write(output.path, output.bytes).unwrap();
        }
    }

    fn fixture_translations(units: &[TextUnit]) -> BTreeMap<String, Translation> {
        units.iter().map(|unit| (unit.id.clone(), Translation {
            unit_id: unit.id.clone(),
            translation_lines: unit.original_lines.iter().map(|line| format!("译文:{line}")).collect(),
            source_hash: TextUnit::source_hash(&unit.original_lines),
            passthrough: false,
        })).collect()
    }

    #[test]
    fn repeat_partial_write_restores_exact_original_slots_and_base_ids() {
        let (root, mut source) = repeat_fixture("rmmz-repeat-partial");
        let units = RmmzAdapter.extract(&root, "ja").unwrap();
        let full = fixture_translations(&units);
        publish_test_outputs(RmmzAdapter.writeback(&root, "zh", &units, &full).unwrap());
        let units = RmmzAdapter.extract(&root, "ja").unwrap();
        let mut live: Value = serde_json::from_str(&fs::read_to_string(root.join("data/Map001.json")).unwrap()).unwrap();
        live["displayName"] = json!("live map setting");
        live["events"][1]["x"] = json!(9);
        fs::write(root.join("data/Map001.json"), live.to_string()).unwrap();
        let mut partial = BTreeMap::new();
        for unit in &units {
            if unit.domain == "system" { partial.insert(unit.id.clone(), full[&unit.id].clone()); }
            if unit.domain == "namebox" {
                let mut empty = full[&unit.id].clone();
                empty.translation_lines.clear();
                partial.insert(unit.id.clone(), empty);
            }
        }
        let outputs = RmmzAdapter.writeback(&root, "zh", &units, &partial).unwrap();
        let map: Value = serde_json::from_slice(&outputs.iter().find(|output| output.path.ends_with("Map001.json")).unwrap().bytes).unwrap();
        source["displayName"] = json!("live map setting");
        source["events"][1]["x"] = json!(9);
        assert_eq!(map, source, "original overflow, name0, body controls and exact command slots must return");
        let actors: Value = serde_json::from_slice(&outputs.iter().find(|output| output.path.ends_with("Actors.json")).unwrap().bytes).unwrap();
        assert_eq!(actors[1]["name"], json!("ラージ"), "DB ID is not an array index");
        let system: Value = serde_json::from_slice(&outputs.iter().find(|output| output.path.ends_with("System.json")).unwrap().bytes).unwrap();
        assert_eq!(system["gameTitle"], json!("译文:物語"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn repeat_write_rejects_inserted_reordered_or_reshaped_live_commands() {
        let (root, source) = repeat_fixture("rmmz-repeat-anchor-conflicts");
        let units = RmmzAdapter.extract(&root, "ja").unwrap();
        publish_test_outputs(RmmzAdapter.writeback(&root, "zh", &units, &fixture_translations(&units)).unwrap());
        for change in 0..6 {
            let mut live = source.clone();
            let list = live["events"][1]["pages"][0]["list"].as_array_mut().unwrap();
            match change {
                0 => list.insert(1, json!({"code": 401, "indent": 0, "parameters": ["新しい台詞。"]})),
                1 => list.swap(1, 4),
                2 => list[1]["code"] = json!(405),
                3 => list[1]["indent"] = json!(1),
                4 => list[1]["parameters"] = json!([]),
                _ => live["events"][1]["pages"][0]["conditions"] = json!({"switch1Valid": true}),
            }
            fs::write(root.join("data/Map001.json"), live.to_string()).unwrap();
            let reextracted = RmmzAdapter.extract(&root, "ja").unwrap();
            assert_eq!(reextracted.iter().map(|unit| &unit.id).collect::<Vec<_>>(), units.iter().map(|unit| &unit.id).collect::<Vec<_>>());
            assert!(RmmzAdapter.writeback(&root, "zh", &reextracted, &BTreeMap::new()).is_err(), "conflict {change}");
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn repeat_write_rejects_missing_live_base_field_or_file() {
        let (root, _) = repeat_fixture("rmmz-missing-live-anchors");
        let units = RmmzAdapter.extract(&root, "ja").unwrap();
        fs::write(root.join("data/Actors.json"), json!([null, {"id": 7, "level": 1}]).to_string()).unwrap();
        assert!(RmmzAdapter.writeback(&root, "zh", &units, &BTreeMap::new()).is_err());
        fs::remove_file(root.join("data/Actors.json")).unwrap();
        assert!(RmmzAdapter.writeback(&root, "zh", &units, &BTreeMap::new()).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn extraction_rejects_symlinked_authoritative_json_backup() {
        let (root, source) = repeat_fixture("rmmz-symlink-backup");
        let external = root.join("outside.json");
        fs::write(&external, source.to_string()).unwrap();
        std::os::unix::fs::symlink(&external, root.join("data/Map001.json.attxbak")).unwrap();
        let error = RmmzAdapter.extract(&root, "ja").unwrap_err();
        assert!(error.to_string().contains("not a regular file"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn extraction_rejects_malformed_authoritative_json_backup() {
        let (root, _) = repeat_fixture("rmmz-malformed-backup");
        fs::write(root.join("data/Map001.json.attxbak"), "not JSON").unwrap();
        assert!(RmmzAdapter.extract(&root, "ja").is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn actor_name_ref_detection() {
        assert!(is_actor_name_ref(r"\N[1]"));
        assert!(is_actor_name_ref(r"\n[12]"));
        assert!(!is_actor_name_ref("エレノア"));
        assert!(!is_actor_name_ref(r"\N[1]さん"));
    }
}
