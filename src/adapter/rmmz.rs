use super::{DetectHit, EngineAdapter, set_json_path};
use crate::model::{ItemType, TextUnit, Translation, needs_translation};
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
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

impl EngineAdapter for RmmzAdapter {
    fn id(&self) -> &'static str {
        "rmmz"
    }
    fn label(&self) -> &'static str {
        "RPG Maker MV/MZ"
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
            let path = entry.path();
            let raw = fs::read_to_string(&path)
                .with_context(|| format!("read {}", path.display()))?;
            let value: Value = serde_json::from_str(&raw)
                .with_context(|| format!("json {}", path.display()))?;
            extract_commands(&name, &value, source_lang, &mut units)?;
        }

        // System.json
        let system_path = data_dir.join("System.json");
        if system_path.is_file() {
            let raw = fs::read_to_string(&system_path)?;
            let system: Value = serde_json::from_str(&raw)?;
            extract_system(&system, source_lang, &mut units);
        }

        // Base DB
        for file in BASE_FILES {
            let p = data_dir.join(file);
            if !p.is_file() {
                continue;
            }
            let raw = fs::read_to_string(&p)?;
            let arr: Value = serde_json::from_str(&raw)?;
            extract_base(file, &arr, source_lang, &mut units);
        }

        // Plugin parameters (js/plugins.js + header @param types; never rewrite plugin source)
        match super::rmmz_plugins::extract_plugins(content_root, source_lang) {
            Ok(mut pu) => {
                if !pu.is_empty() {
                    eprintln!("rmmz: extracted {} plugin parameter unit(s)", pu.len());
                }
                units.append(&mut pu);
            }
            Err(e) => eprintln!("rmmz: plugin extract skipped: {e:#}"),
        }

        Ok(units)
    }

    fn writeback(
        &self,
        content_root: &Path,
        units: &[TextUnit],
        translations: &BTreeMap<String, Translation>,
    ) -> Result<BTreeMap<String, String>> {
        let data_dir = resolve_data_dir(content_root)?;
        let mut files: BTreeMap<String, Value> = BTreeMap::new();

        // Load only data/* files we need (skip js/plugins.js locations)
        let mut needed = BTreeMap::<String, ()>::new();
        for u in units {
            if !translations.contains_key(&u.id) {
                continue;
            }
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
                continue;
            }
            let raw = fs::read_to_string(&p)
                .with_context(|| format!("read origin {}", p.display()))?;
            let v: Value = serde_json::from_str(&raw)?;
            files.insert(file.clone(), v);
        }

        for u in units {
            if u.domain == "plugins" || u.location.starts_with("js/") {
                continue;
            }
            let Some(tr) = translations.get(&u.id) else {
                continue;
            };
            if tr.translation_lines.is_empty() {
                continue;
            }
            apply_unit(&mut files, u, tr)?;
        }

        let mut out = BTreeMap::new();
        for (file, value) in files {
            let text = serde_json::to_string(&value)?;
            let rel = format!("data/{file}");
            out.insert(rel, text);
        }

        if let Some(plugins_js) =
            super::rmmz_plugins::writeback_plugins(content_root, units, translations)?
        {
            out.insert("js/plugins.js".into(), plugins_js);
        }

        Ok(out)
    }
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

    let flush_long = |units: &mut Vec<TextUnit>,
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

    let flush_scroll = |units: &mut Vec<TextUnit>,
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
                if let Some(arr) = params.as_array() {
                    if arr.len() >= 5 {
                        if let Some(s) = arr[4].as_str() {
                            let t = s.trim();
                            if !t.is_empty() {
                                role = t.to_string();
                            }
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

fn push_short(units: &mut Vec<TextUnit>, location: &str, text: &str, source_lang: &str, domain: &str) {
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
    tr: &Translation,
) -> Result<()> {
    let file = unit
        .location
        .split('/')
        .next()
        .ok_or_else(|| anyhow::anyhow!("bad location {}", unit.location))?;
    let root = files
        .get_mut(file)
        .ok_or_else(|| anyhow::anyhow!("file not loaded: {file}"))?;

    match unit.item_type {
        ItemType::Array => {
            // choices: location points at command; parameters[0] is array
            write_choices(root, &unit.location, &tr.translation_lines)?;
        }
        ItemType::ShortText => {
            write_short(root, &unit.location, &tr.translation_lines)?;
        }
        ItemType::LongText => {
            write_long_text(root, unit, &tr.translation_lines)?;
        }
    }
    Ok(())
}

fn write_short(root: &mut Value, location: &str, lines: &[String]) -> Result<()> {
    let text = lines.first().cloned().unwrap_or_default();
    // location like System.json/gameTitle or Actors.json/1/name
    let rest = location
        .split_once('/')
        .map(|(_, r)| r)
        .unwrap_or("");
    if rest.is_empty() {
        bail!("short path missing fields: {location}");
    }
    set_json_path(root, rest, Value::String(text))?;
    Ok(())
}

fn write_choices(root: &mut Value, location: &str, lines: &[String]) -> Result<()> {
    let rest = location
        .split_once('/')
        .map(|(_, r)| r)
        .unwrap_or("");
    // navigate to command object
    let cmd = navigate_mut(root, rest)?;
    let params = cmd
        .get_mut("parameters")
        .and_then(|v| v.as_array_mut())
        .ok_or_else(|| anyhow::anyhow!("choices missing parameters at {location}"))?;
    if params.is_empty() {
        bail!("choices empty parameters at {location}");
    }
    params[0] = Value::Array(lines.iter().cloned().map(Value::String).collect());
    Ok(())
}

fn write_long_text(root: &mut Value, unit: &TextUnit, lines: &[String]) -> Result<()> {
    if unit.source_line_paths.is_empty() {
        return Ok(());
    }
    // ponytail: never insert/delete event commands (shifts later indices).
    // Fit translation into the original number of 401/405 slots.
    let n_src = unit.source_line_paths.len();
    let fitted = fit_lines(lines, n_src);

    for (i, path) in unit.source_line_paths.iter().enumerate() {
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
        let text = fitted.get(i).cloned().unwrap_or_default();
        if params.is_empty() {
            params.push(Value::String(text));
        } else {
            params[0] = Value::String(text);
        }
    }
    Ok(())
}

/// Resize translation lines to exactly `n` slots (join extras into last / pad empty).
fn fit_lines(lines: &[String], n: usize) -> Vec<String> {
    if n == 0 {
        return vec![];
    }
    if lines.len() == n {
        return lines.to_vec();
    }
    if lines.len() > n {
        let mut out: Vec<String> = lines[..n - 1].to_vec();
        out.push(lines[n - 1..].join(""));
        return out;
    }
    let mut out = lines.to_vec();
    out.resize(n, String::new());
    out
}

fn split_list_index(location: &str) -> Result<(String, usize)> {
    // "Map003.json/2/0/5" → list path "Map003.json/2/0" is not list; need events/N/pages/M/list
    // Our location uses compact form Map/event/page/cmdIndex for maps.
    // Navigate rebuilds actual JSON path.
    let parts: Vec<&str> = location.split('/').collect();
    if parts.len() < 2 {
        bail!("bad line path {location}");
    }
    let idx: usize = parts[parts.len() - 1]
        .parse()
        .with_context(|| format!("cmd index in {location}"))?;
    let list_loc = parts[..parts.len() - 1].join("/");
    Ok((list_loc, idx))
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
        return list
            .get_mut(cmd)
            .ok_or_else(|| anyhow::anyhow!("cmd OOB"));
    }

    if arr[idx].get("list").is_some() {
        // CommonEvents: id/cmd
        if parts.len() == 2 {
            if let Ok(cmd) = parts[1].parse::<usize>() {
                let list = arr[idx]
                    .get_mut("list")
                    .and_then(|v| v.as_array_mut())
                    .ok_or_else(|| anyhow::anyhow!("no list"))?;
                return list
                    .get_mut(cmd)
                    .ok_or_else(|| anyhow::anyhow!("cmd OOB"));
            }
        }
    }

    // base field id/name etc.
    let field = parts[1];
    arr[idx]
        .as_object_mut()
        .and_then(|o| o.get_mut(field))
        .ok_or_else(|| anyhow::anyhow!("missing field {field}"))
}
