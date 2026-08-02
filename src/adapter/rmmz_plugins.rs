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
use serde_json::Value;
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
            // Identity handles: referenced verbatim by plugin commands and event
            // scripts (e.g. `gainAchievement("実績_x")`). They are often written
            // in Japanese, so the CJK heuristic alone would happily translate
            // them and silently break the lookup.
            "key",
            "keyname",
            "identifier",
            "ident",
            "uniquekey",
            "slug",
            "tag",
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
    let mut root = decode_json_container(raw)
        .with_context(|| format!("nested param is not JSON: {}", truncate(raw, 80)))?;
    set_path(&mut root, json_path, Value::String(text.to_string()))?;
    Ok(serde_json::to_string(&root)?)
}

/// Decode a value that is *itself* a JSON-encoded container (`"[…]"` / `"{…}"`).
///
/// RPG Maker stores struct/array plugin params as strings holding JSON, often
/// nested several levels deep. Extraction descends through those levels
/// transparently, so writeback **must** use this exact same predicate — any
/// asymmetry between the two sides re-introduces the nested-writeback bug
/// (`expected array at 0`, or a JSON string silently flattened into an object).
fn decode_json_container(s: &str) -> Option<Value> {
    let t = s.trim();
    if !(t.starts_with('[') || t.starts_with('{')) {
        return None;
    }
    serde_json::from_str::<Value>(t).ok()
}

fn set_path(root: &mut Value, path: &str, value: Value) -> Result<()> {
    if path.is_empty() {
        *root = value;
        return Ok(());
    }
    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    descend_set(root, &parts, value).with_context(|| format!("json path {path}"))
}

/// Walk `parts` into `cur`, re-encoding any layer that was a JSON string.
///
/// Never creates missing keys: a path that no longer matches the file is an
/// error, not a licence to overwrite. Because each layer is decoded into a
/// detached `Value` and only written back after the recursion succeeds, a
/// failure anywhere leaves the whole param byte-identical.
fn descend_set(cur: &mut Value, parts: &[&str], value: Value) -> Result<()> {
    // Path exhausted → this is the leaf being translated. Checked before the
    // decode branch so a leaf whose text merely *looks* like JSON is replaced
    // verbatim rather than descended into.
    let Some((part, rest)) = parts.split_first() else {
        *cur = value;
        return Ok(());
    };

    if let Value::String(s) = cur
        && let Some(mut inner) = decode_json_container(s)
    {
        descend_set(&mut inner, parts, value)?;
        *cur = Value::String(serde_json::to_string(&inner)?);
        return Ok(());
    }

    if let Ok(idx) = part.parse::<usize>() {
        let arr = cur
            .as_array_mut()
            .ok_or_else(|| anyhow::anyhow!("expected array at {part}"))?;
        let len = arr.len();
        let slot = arr
            .get_mut(idx)
            .ok_or_else(|| anyhow::anyhow!("index {idx} out of range (len {len})"))?;
        descend_set(slot, rest, value)
    } else {
        let obj = cur
            .as_object_mut()
            .ok_or_else(|| anyhow::anyhow!("expected object at {part}"))?;
        let slot = obj
            .get_mut(*part)
            .ok_or_else(|| anyhow::anyhow!("missing key {part}"))?;
        descend_set(slot, rest, value)
    }
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

    // ---- Nested JSON-string writeback regressions ----
    // RPG Maker plugin params encode structs as *strings containing JSON*, often
    // several layers deep. Extraction descends through those layers transparently,
    // so writeback must decode/re-encode the same layers or it either errors
    // ("expected array at 0") or silently flattens a string into an object.

    use serde_json::json;

    fn enc(v: &Value) -> String {
        serde_json::to_string(v).unwrap()
    }

    /// Structural fingerprint. A JSON-encoded string renders as `s(<inner>)`, so
    /// losing a wrapper (string -> object) changes the signature.
    fn type_sig(v: &Value) -> String {
        match v {
            Value::Null => "n".into(),
            Value::Bool(_) => "b".into(),
            Value::Number(_) => "#".into(),
            Value::String(s) => {
                let t = s.trim();
                if (t.starts_with('[') || t.starts_with('{'))
                    && let Ok(inner) = serde_json::from_str::<Value>(t)
                {
                    return format!("s({})", type_sig(&inner));
                }
                "s".into()
            }
            Value::Array(a) => {
                format!("[{}]", a.iter().map(type_sig).collect::<Vec<_>>().join(","))
            }
            Value::Object(m) => format!(
                "{{{}}}",
                m.iter()
                    .map(|(k, x)| format!("{k}:{}", type_sig(x)))
                    .collect::<Vec<_>>()
                    .join(",")
            ),
        }
    }

    fn test_root(name: &str, plugins_js: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("attx-pl-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("js/plugins")).unwrap();
        fs::write(dir.join("js/plugins.js"), plugins_js).unwrap();
        dir
    }

    /// Case A: array whose elements are JSON strings (Achievement2 style).
    #[test]
    fn nested_array_of_json_strings() {
        let ach = json!({"key":"実績_a","title":"称号","description":"説明"});
        let param = enc(&json!([enc(&ach)]));

        let out = set_in_json_string(&param, "0/description", "描述译文").expect("writeback");

        let root: Value = serde_json::from_str(&out).unwrap();
        let el = root[0]
            .as_str()
            .expect("array element must stay a JSON string");
        let obj: Value = serde_json::from_str(el).unwrap();
        assert_eq!(obj["description"], json!("描述译文"));
        assert_eq!(obj["title"], json!("称号"), "siblings must survive");
        assert_eq!(obj["key"], json!("実績_a"), "siblings must survive");
    }

    /// Case B: object value is a JSON string (SaveFilePlus style).
    #[test]
    fn nested_object_value_is_json_string() {
        let terms = json!({"gold":"所持金","mapname":"現在地"});
        let param = enc(&json!({"terms": enc(&terms)}));

        let out = set_in_json_string(&param, "terms/gold", "金钱").expect("writeback");

        let root: Value = serde_json::from_str(&out).unwrap();
        let t = root["terms"]
            .as_str()
            .expect("terms must stay a JSON string");
        let obj: Value = serde_json::from_str(t).unwrap();
        assert_eq!(obj["gold"], json!("金钱"));
        assert_eq!(obj["mapname"], json!("現在地"));
    }

    /// Case C: three encode levels (QuestSystem style) — the reported
    /// `expected array at 0` failure.
    #[test]
    fn nested_three_levels_keeps_type_signature() {
        let reward = json!({"Name":"報酬A"});
        let quest = json!({
            "Title":"クエスト1",
            "Rewards": enc(&json!([enc(&reward)])),
        });
        let param = enc(&json!([enc(&quest)]));
        let before = type_sig(&Value::String(param.clone()));

        let out = set_in_json_string(&param, "0/Rewards/0/Name", "奖励A").expect("writeback");

        assert_eq!(
            type_sig(&Value::String(out.clone())),
            before,
            "type signature must survive writeback"
        );
        let q: Value = serde_json::from_str(
            serde_json::from_str::<Value>(&out).unwrap()[0]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(q["Title"], json!("クエスト1"));
        let rewards: Value = serde_json::from_str(q["Rewards"].as_str().unwrap()).unwrap();
        let r0: Value = serde_json::from_str(rewards[0].as_str().unwrap()).unwrap();
        assert_eq!(r0["Name"], json!("奖励A"));
    }

    /// Regression: ordinary (unwrapped) nested JSON must keep working.
    #[test]
    fn plain_nested_json_still_works() {
        let param = enc(&json!({"list":[{"name":"名前"}]}));
        let out = set_in_json_string(&param, "list/0/name", "名字").expect("writeback");
        let root: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(root["list"][0]["name"], json!("名字"));
    }

    /// A path that no longer matches the file must not abort the whole writeback.
    #[test]
    fn missing_path_is_reported_not_silently_created() {
        let param = enc(&json!({"terms": enc(&json!({"gold":"所持金"}))}));
        let err = set_in_json_string(&param, "terms/nosuch", "x")
            .expect_err("missing key must error rather than corrupt");
        assert!(
            err.to_string().contains("nosuch"),
            "error should name the segment: {err}"
        );
    }

    /// End-to-end: extract emits the paths, writeback must consume the same
    /// paths and hand back a structurally identical plugins.js.
    #[test]
    fn extract_writeback_roundtrip_preserves_nesting() {
        let ach = json!({"key":"実績_a","title":"称号","description":"説明"});
        let quest = json!({
            "Title":"クエスト1",
            "Rewards": enc(&json!([enc(&json!({"Name":"報酬A"}))])),
        });
        let plugins = json!([
            {"name":"TorigoyaMZ_Achievement2","status":true,"description":"",
             "parameters":{"baseAchievementData": enc(&json!([enc(&ach)]))}},
            {"name":"SaveFilePlus","status":true,"description":"",
             "parameters":{"info1": enc(&json!({"terms": enc(&json!({"gold":"所持金"}))}))}},
            {"name":"QuestSystem","status":true,"description":"",
             "parameters":{"QuestDatas": enc(&json!([enc(&quest)]))}}
        ]);
        let js = format!(
            "var $plugins =\n{};\n",
            serde_json::to_string(&plugins).unwrap()
        );
        let dir = test_root("roundtrip", &js);

        let units = extract_plugins(&dir, "ja").unwrap();
        assert!(!units.is_empty(), "expected plugin units");

        let mut trs = BTreeMap::new();
        for u in &units {
            trs.insert(
                u.id.clone(),
                Translation {
                    unit_id: u.id.clone(),
                    translation_lines: vec![format!("ZH:{}", u.original_lines.join(""))],
                    source_hash: String::new(),
                    passthrough: false,
                },
            );
        }

        let out = writeback_plugins(&dir, &units, &trs)
            .expect("writeback must succeed")
            .expect("plugins.js content");

        let before = parse_plugins_js(&js).unwrap();
        let after = parse_plugins_js(&out).unwrap();
        assert_eq!(before.len(), after.len());
        for (b, a) in before.iter().zip(after.iter()) {
            assert_eq!(
                type_sig(&b["parameters"]),
                type_sig(&a["parameters"]),
                "param type signature changed for {}",
                b["name"]
            );
        }
        assert!(out.contains("ZH:説明"), "translation must land in output");
        assert!(out.contains("ZH:報酬A"), "deep translation must land");
        assert!(
            !out.contains("ZH:実績_a"),
            "identity key must not be translated (events reference it verbatim)"
        );
        assert!(
            out.contains("実績_a"),
            "identity key must survive unchanged"
        );
    }

    /// Identity fields inside nested structs address game logic, not the player.
    /// Achievement `key`s are referenced verbatim by `gainAchievement(...)`, so
    /// translating them silently breaks unlocking.
    #[test]
    fn identity_keys_are_not_extracted() {
        let ach = json!({"key":"実績_xxx","title":"タイトル","description":"説明文"});
        let plugins = json!([
            {"name":"TorigoyaMZ_Achievement2","status":true,"description":"",
             "parameters":{"baseAchievementData": enc(&json!([enc(&ach)]))}}
        ]);
        let js = format!(
            "var $plugins =\n{};\n",
            serde_json::to_string(&plugins).unwrap()
        );
        let dir = test_root("identity", &js);

        let units = extract_plugins(&dir, "ja").unwrap();
        let paths: Vec<&str> = units.iter().map(|u| u.location.as_str()).collect();

        assert!(
            paths.iter().any(|p| p.ends_with("#0/title")),
            "player-visible title must still be extracted: {paths:?}"
        );
        assert!(
            paths.iter().any(|p| p.ends_with("#0/description")),
            "player-visible description must still be extracted: {paths:?}"
        );
        assert!(
            !paths.iter().any(|p| p.ends_with("#0/key")),
            "identity key must be skipped: {paths:?}"
        );
    }
}
