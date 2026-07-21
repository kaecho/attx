//! RPG Maker MV/MZ plugin parameter text extraction.
//!
//! Flow:
//! 1. Parse `js/plugins.js` (`$plugins = [...]`)
//! 2. For enabled plugins, optionally read only the plugin *header* in
//!    `js/plugins/{name}.js` (`@param` / `@type` / `@text`) — never modify plugin source.
//! 3. Keep leaf strings that are user-visible (UI / help / labels).
//! 4. Writeback only rewrites `js/plugins.js`.

use crate::model::{ItemType, TextUnit, Translation, needs_translation};
use anyhow::{Context, Result, bail};
use regex::Regex;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::sync::LazyLock;

const DOMAIN: &str = "plugins";
/// Param types that hold player-visible text at the leaf (or wrap structs of such).
fn type_is_texty(ty: &str) -> bool {
    let t = ty.trim().to_ascii_lowercase();
    t == "string"
        || t == "multiline_string"
        || t == "note"
        || t.starts_with("struct<")
        || t.ends_with("[]") && {
            let inner = t.trim_end_matches("[]");
            type_is_texty(inner) || inner.starts_with("struct<")
        }
}

fn type_is_pure_text(ty: &str) -> bool {
    matches!(
        ty.trim().to_ascii_lowercase().as_str(),
        "string" | "multiline_string" | "note"
    )
}

fn visible_key(key: &str) -> bool {
    static KEYS: LazyLock<BTreeSet<&'static str>> = LazyLock::new(|| {
        [
            "name",
            "text",
            "label",
            "title",
            "description",
            "message",
            "help",
            "helptext",
            "commandname",
            "commandtext",
            "displayname",
            "windowtitle",
            "buttontext",
            "caption",
            "header",
            "footer",
            "originalname",
            "anydefaultname",
            "menuname",
            "menulabel",
            "glossary",
            "term",
            "terms",
            "content",
            "body",
            "subtitle",
            "hint",
            "tooltip",
            "prefix",
            "suffix",
            "format",
            "template",
            "leftblocklabel",
            "rightblocklabel",
            "emptytext",
            "nonetext",
            "unknowntext",
            "defaultname",
            "pagename",
            "category",
            "categoryname",
        ]
        .into_iter()
        .collect()
    });
    let k = key.to_ascii_lowercase();
    KEYS.contains(k.as_str())
        || k.ends_with("name")
        || k.ends_with("text")
        || k.ends_with("label")
        || k.ends_with("title")
        || k.ends_with("message")
        || k.ends_with("help")
        || k.ends_with("description")
}

fn skip_key(key: &str) -> bool {
    static KEYS: LazyLock<BTreeSet<&'static str>> = LazyLock::new(|| {
        [
            "id",
            "symbol",
            "switch",
            "switchid",
            "variable",
            "variableid",
            "commonevent",
            "script",
            "formula",
            "file",
            "filename",
            "picture",
            "image",
            "icon",
            "color",
            "actor",
            "enemy",
            "state",
            "skill",
            "item",
            "weapon",
            "armor",
            "class",
            "troop",
            "map",
            "event",
            "bgm",
            "bgs",
            "me",
            "se",
            "type",
            "status",
            "code",
            "x",
            "y",
            "width",
            "height",
            "opacity",
            "volume",
            "pitch",
            "pan",
            "scenename",
            "classname",
            "tagname",
            "validactors",
            "invalidactors",
            "validenemies",
            "invalidenemies",
            "note",
        ]
        .into_iter()
        .collect()
    });
    KEYS.contains(key.to_ascii_lowercase().as_str())
}

#[derive(Debug, Default)]
struct ParamMeta {
    /// param name -> @type
    types: BTreeMap<String, String>,
    /// param name -> @text (JP label in editor; optional signal)
    texts: BTreeMap<String, String>,
}

fn parse_plugin_header(src: &str) -> ParamMeta {
    let mut meta = ParamMeta::default();
    let mut current: Option<String> = None;
    for line in src.lines() {
        let line = line.trim().trim_start_matches('*').trim();
        if let Some(rest) = line.strip_prefix("@param ") {
            let name = rest.trim().to_string();
            if !name.is_empty() {
                current = Some(name);
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("@type ") {
            if let Some(name) = &current {
                meta.types.insert(name.clone(), rest.trim().to_string());
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("@text ") {
            if let Some(name) = &current {
                meta.texts.insert(name.clone(), rest.trim().to_string());
            }
            continue;
        }
        // end of plugin doc block → stop early enough
        if line.starts_with("@help") {
            // keep scanning: more @param can appear after @help in some plugins
            continue;
        }
    }
    meta
}

/// Load `$plugins` array from plugins.js text.
pub fn parse_plugins_js(raw: &str) -> Result<Vec<Value>> {
    let re = Regex::new(r"(?s)\$plugins\s*=\s*(\[[\s\S]*\])\s*;").expect("plugins re");
    let caps = re
        .captures(raw)
        .ok_or_else(|| anyhow::anyhow!("$plugins array not found in plugins.js"))?;
    let arr_txt = caps.get(1).unwrap().as_str();
    let v: Value = serde_json::from_str(arr_txt).context("parse $plugins JSON")?;
    let arr = v
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("$plugins is not an array"))?
        .clone();
    Ok(arr)
}

fn render_plugins_js(plugins: &[Value]) -> Result<String> {
    // One plugin per line for smaller diffs, like RPG Maker exports.
    let mut lines = vec![
        "// Generated by RPG Maker.".to_string(),
        "// Do not edit this file directly.".to_string(),
        "// Modified by attx (plugin parameter translations only).".to_string(),
        "var $plugins =".to_string(),
        "[".to_string(),
    ];
    for (i, p) in plugins.iter().enumerate() {
        let mut s = serde_json::to_string(p)?;
        if i + 1 < plugins.len() {
            s.push(',');
        }
        lines.push(s);
    }
    lines.push("];".to_string());
    lines.push(String::new());
    Ok(lines.join("\n"))
}

pub fn extract_plugins(content_root: &Path, source_lang: &str) -> Result<Vec<TextUnit>> {
    let path = content_root.join("js/plugins.js");
    if !path.is_file() {
        return Ok(vec![]);
    }
    let raw = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let plugins = parse_plugins_js(&raw)?;
    let plugins_dir = content_root.join("js/plugins");
    let mut units = Vec::new();

    for (idx, plug) in plugins.iter().enumerate() {
        let status = plug
            .get("status")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if !status {
            continue;
        }
        let name = plug
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if name.is_empty() {
            continue;
        }
        let params = match plug.get("parameters").and_then(|v| v.as_object()) {
            Some(m) if !m.is_empty() => m,
            _ => continue,
        };

        // Read header only when parameters exist (agent-style: only open source when needed).
        let meta = {
            let js = plugins_dir.join(format!("{name}.js"));
            if js.is_file() {
                let src = fs::read_to_string(&js).unwrap_or_default();
                // only first ~200 lines matter for @param
                let head: String = src.lines().take(400).collect::<Vec<_>>().join("\n");
                parse_plugin_header(&head)
            } else {
                ParamMeta::default()
            }
        };

        for (key, val) in params {
            let Some(s) = val.as_str() else { continue };
            if s.is_empty() {
                continue;
            }
            let ty = meta.types.get(key).map(|s| s.as_str()).unwrap_or("");
            // Skip pure machine params when typed
            if !ty.is_empty() && !type_is_texty(ty) && !visible_key(key) {
                continue;
            }
            // Numeric / bool string
            if is_machine_literal(s) {
                continue;
            }

            let force = type_is_pure_text(ty) || visible_key(key);
            extract_value_strings(
                s,
                &format!("js/plugins.js/{idx}/{key}"),
                &name,
                key,
                "",
                force,
                source_lang,
                &mut units,
            );
        }
    }
    Ok(units)
}

fn is_machine_literal(s: &str) -> bool {
    let t = s.trim();
    if t.is_empty() {
        return true;
    }
    if matches!(
        t.to_ascii_lowercase().as_str(),
        "true" | "false" | "null" | "undefined"
    ) {
        return true;
    }
    if Regex::new(r"^-?\d+(\.\d+)?$").unwrap().is_match(t) {
        return true;
    }
    // pure path / filename-ish without CJK
    if !needs_translation(t, "ja")
        && (t.contains('/') || t.ends_with(".png") || t.ends_with(".ogg"))
    {
        return true;
    }
    false
}

#[allow(clippy::too_many_arguments)] // internal recursion carries full addressing context
fn extract_value_strings(
    raw: &str,
    base_loc: &str,
    plugin: &str,
    param: &str,
    json_path: &str,
    force_parent: bool,
    source_lang: &str,
    units: &mut Vec<TextUnit>,
) {
    let trimmed = raw.trim();
    // Nested JSON string (struct / array)
    if (trimmed.starts_with('[') || trimmed.starts_with('{'))
        && let Ok(v) = serde_json::from_str::<Value>(trimmed)
    {
        walk_json(
            &v,
            base_loc,
            plugin,
            param,
            json_path,
            force_parent,
            source_lang,
            units,
        );
        return;
    }
    // Leaf string
    if !force_parent && !needs_translation(raw, source_lang) {
        return;
    }
    if force_parent && !needs_translation(raw, source_lang) {
        return;
    }
    if is_machine_literal(raw) {
        return;
    }
    // Skip pure script-like
    if raw.contains("$game") || raw.contains("return ") || raw.starts_with("function") {
        return;
    }
    push_unit(units, base_loc, plugin, param, json_path, raw, source_lang);
}

#[allow(clippy::too_many_arguments)] // internal recursion carries full addressing context
fn walk_json(
    v: &Value,
    base_loc: &str,
    plugin: &str,
    param: &str,
    json_path: &str,
    force_parent: bool,
    source_lang: &str,
    units: &mut Vec<TextUnit>,
) {
    match v {
        Value::String(s) => {
            extract_value_strings(
                s,
                base_loc,
                plugin,
                param,
                json_path,
                force_parent,
                source_lang,
                units,
            );
        }
        Value::Array(arr) => {
            for (i, el) in arr.iter().enumerate() {
                let path = if json_path.is_empty() {
                    format!("{i}")
                } else {
                    format!("{json_path}/{i}")
                };
                walk_json(
                    el,
                    base_loc,
                    plugin,
                    param,
                    &path,
                    force_parent,
                    source_lang,
                    units,
                );
            }
        }
        Value::Object(map) => {
            for (k, el) in map {
                if skip_key(k) {
                    continue;
                }
                let path = if json_path.is_empty() {
                    k.clone()
                } else {
                    format!("{json_path}/{k}")
                };
                let force = force_parent || visible_key(k);
                // Only descend into non-visible keys if they hold nested objects/arrays
                // or the value is a CJK-looking string
                match el {
                    Value::String(s) if (force || needs_translation(s, source_lang)) => {
                        extract_value_strings(
                            s,
                            base_loc,
                            plugin,
                            param,
                            &path,
                            force,
                            source_lang,
                            units,
                        );
                    }
                    Value::Array(_) | Value::Object(_) => {
                        walk_json(
                            el,
                            base_loc,
                            plugin,
                            param,
                            &path,
                            force,
                            source_lang,
                            units,
                        );
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
}

fn push_unit(
    units: &mut Vec<TextUnit>,
    base_loc: &str,
    plugin: &str,
    param: &str,
    json_path: &str,
    text: &str,
    source_lang: &str,
) {
    let t = text.trim();
    if t.is_empty() || !needs_translation(t, source_lang) {
        return;
    }
    let location = if json_path.is_empty() {
        base_loc.to_string()
    } else {
        format!("{base_loc}#{json_path}")
    };
    let lines = vec![t.to_string()];
    let id = TextUnit::compute_id("rmmz", &location, &lines);
    let payload = serde_json::json!({
        "kind": "plugins",
        "plugin": plugin,
        "param": param,
        "json_path": json_path,
        "plugin_index": base_loc
            .trim_start_matches("js/plugins.js/")
            .split('/')
            .next()
            .unwrap_or(""),
    })
    .to_string();
    units.push(TextUnit {
        id,
        engine: "rmmz".into(),
        domain: DOMAIN.into(),
        location,
        item_type: if t.chars().count() > 40 {
            ItemType::LongText
        } else {
            ItemType::ShortText
        },
        role: "系统".into(),
        original_lines: lines,
        source_line_paths: vec![],
        context: format!("plugin:{plugin}/{param}"),
        payload,
    });
}

/// Apply plugin translations; returns full new `js/plugins.js` content when any unit hits.
pub fn writeback_plugins(
    content_root: &Path,
    units: &[TextUnit],
    translations: &BTreeMap<String, Translation>,
) -> Result<Option<String>> {
    let mut plugin_units: Vec<(&TextUnit, &Translation)> = Vec::new();
    for u in units {
        if u.domain != DOMAIN {
            continue;
        }
        if let Some(tr) = translations.get(&u.id)
            && !tr.translation_lines.is_empty()
        {
            plugin_units.push((u, tr));
        }
    }
    if plugin_units.is_empty() {
        return Ok(None);
    }

    let path = content_root.join("js/plugins.js");
    let raw = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let mut plugins = parse_plugins_js(&raw)?;

    for (u, tr) in plugin_units {
        let text = tr.translation_lines.join("\n");
        apply_one(&mut plugins, u, &text)?;
    }

    Ok(Some(render_plugins_js(&plugins)?))
}

fn apply_one(plugins: &mut [Value], unit: &TextUnit, text: &str) -> Result<()> {
    // location: js/plugins.js/{idx}/{param} or .../{param}#{json_path}
    let rest = unit
        .location
        .strip_prefix("js/plugins.js/")
        .unwrap_or(unit.location.as_str());
    let (path_part, json_path) = match rest.split_once('#') {
        Some((p, j)) => (p, j),
        None => (rest, ""),
    };
    let mut segs = path_part.split('/');
    let idx: usize = segs
        .next()
        .unwrap_or("")
        .parse()
        .with_context(|| format!("plugin index in {}", unit.location))?;
    let param = segs
        .next()
        .ok_or_else(|| anyhow::anyhow!("param missing in {}", unit.location))?;
    if idx >= plugins.len() {
        bail!("plugin index {idx} out of range");
    }
    let plug = &mut plugins[idx];
    let params = plug
        .get_mut("parameters")
        .and_then(|v| v.as_object_mut())
        .ok_or_else(|| anyhow::anyhow!("plugin {idx} has no parameters object"))?;
    let cur = params
        .get(param)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let new_val = if json_path.is_empty() {
        text.to_string()
    } else {
        set_in_json_string(&cur, json_path, text)?
    };
    params.insert(param.to_string(), Value::String(new_val));
    Ok(())
}

fn set_in_json_string(raw: &str, json_path: &str, text: &str) -> Result<String> {
    let mut root: Value = serde_json::from_str(raw)
        .with_context(|| format!("nested param is not JSON: {}", truncate(raw, 80)))?;
    set_path(&mut root, json_path, Value::String(text.to_string()))?;
    Ok(serde_json::to_string(&root)?)
}

fn set_path(root: &mut Value, path: &str, value: Value) -> Result<()> {
    if path.is_empty() {
        *root = value;
        return Ok(());
    }
    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    let mut cur = root;
    for (i, part) in parts.iter().enumerate() {
        let last = i + 1 == parts.len();
        if let Ok(idx) = part.parse::<usize>() {
            let arr = cur
                .as_array_mut()
                .ok_or_else(|| anyhow::anyhow!("expected array at {part}"))?;
            if idx >= arr.len() {
                bail!("index {idx} out of range");
            }
            if last {
                arr[idx] = value;
                return Ok(());
            }
            cur = &mut arr[idx];
        } else {
            if !cur.is_object() {
                *cur = Value::Object(Map::new());
            }
            let obj = cur.as_object_mut().unwrap();
            if last {
                obj.insert((*part).to_string(), value);
                return Ok(());
            }
            if !obj.contains_key(*part) {
                obj.insert((*part).to_string(), Value::Object(Map::new()));
            }
            cur = obj.get_mut(*part).unwrap();
        }
    }
    Ok(())
}

fn truncate(s: &str, n: usize) -> String {
    let mut t: String = s.chars().take(n).collect();
    if s.chars().count() > n {
        t.push('…');
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_extract_string_param() {
        let raw = r#"// Generated
var $plugins =
[
{"name":"Demo","status":true,"description":"x","parameters":{"leftBlockLabel":"現在地：","count":"3"}}
];
"#;
        let plugins = parse_plugins_js(raw).unwrap();
        assert_eq!(plugins.len(), 1);
        // extract without plugin file: key heuristic
        let dir = tempfile_dir(raw);
        let units = extract_plugins(&dir, "ja").unwrap();
        assert!(
            units.iter().any(|u| u.original_lines[0] == "現在地："),
            "units={units:?}"
        );
        assert!(
            !units.iter().any(|u| u.original_lines[0] == "3"),
            "must skip number"
        );
    }

    fn tempfile_dir(plugins_js: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("attx-pl-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("js/plugins")).unwrap();
        fs::write(dir.join("js/plugins.js"), plugins_js).unwrap();
        dir
    }
}
