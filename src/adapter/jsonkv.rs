//! JSON-family adapters (AiNiee MToolReader / ParatranzReader / I18nextReader
//! / VntReader equivalents). All four claim `.json`, so `detect` sniffs the
//! content — registry order goes most-specific-shape first:
//!
//! * paratranz — `[{"key":…,"original":…,"translation":…}]`
//! * vnt (VNTextPatch) — `[{"name":…,"message":…}]`
//! * mtool — flat `{"原文":"原文"}` export (`ManualTransFile.json`)
//! * i18next — nested object whose leaves are all strings
//!
//! Ambiguous files can be forced with `--engine <id>`.

use super::{DetectHit, FormatAdapter, OutputFile, output_sibling, set_json_path};
use crate::model::{ItemType, TextUnit, Translation, needs_translation};
use anyhow::{Context, Result};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;

pub struct MtoolAdapter;
pub struct ParatranzAdapter;
pub struct I18nextAdapter;
pub struct VntAdapter;

fn load_json(input: &Path) -> Result<Value> {
    let raw = std::fs::read_to_string(input).with_context(|| format!("{}", input.display()))?;
    serde_json::from_str(&raw).with_context(|| format!("parse json {}", input.display()))
}

fn sniff(input: &Path, pred: impl Fn(&Value) -> bool) -> bool {
    input.is_file()
        && input
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("json"))
        && load_json(input).map(|v| pred(&v)).unwrap_or(false)
}

fn make_unit(
    engine: &str,
    location: String,
    text: &str,
    role: &str,
    context: String,
    payload: String,
) -> TextUnit {
    let lines: Vec<String> = text.split('\n').map(str::to_string).collect();
    let item_type = if lines.len() > 1 {
        ItemType::LongText
    } else {
        ItemType::ShortText
    };
    TextUnit {
        id: TextUnit::compute_id(engine, &location, &lines),
        engine: engine.into(),
        domain: engine.into(),
        location,
        item_type,
        role: role.into(),
        original_lines: lines,
        source_line_paths: vec![],
        context,
        payload,
    }
}

fn joined(tr: &Translation) -> String {
    tr.translation_lines.join("\n")
}

fn pretty_output(input: &Path, target_lang: &str, value: &Value) -> Result<Vec<OutputFile>> {
    let body = serde_json::to_string_pretty(value)? + "\n";
    Ok(vec![OutputFile::text(
        output_sibling(input, target_lang, "json"),
        body,
    )])
}

// ---------------------------------------------------------------- mtool

impl FormatAdapter for MtoolAdapter {
    fn id(&self) -> &'static str {
        "mtool"
    }
    fn label(&self) -> &'static str {
        "MTool export (ManualTransFile.json)"
    }
    fn extensions(&self) -> &'static [&'static str] {
        &["json"]
    }

    fn detect(&self, input: &Path) -> Option<DetectHit> {
        let hit = sniff(input, |v| {
            let Some(obj) = v.as_object() else {
                return false;
            };
            if obj.is_empty() || !obj.values().all(|v| v.is_string()) {
                return false;
            }
            let cjk_keys = obj.keys().any(|k| crate::model::looks_like_source_ja(k));
            let eq = obj
                .iter()
                .filter(|(k, v)| v.as_str() == Some(k.as_str()))
                .count();
            cjk_keys || eq * 10 >= obj.len() * 3 // ≥30% untranslated key==value
        });
        hit.then(|| DetectHit {
            engine_id: self.id(),
            label: self.label(),
            content_root: input.canonicalize().unwrap_or_else(|_| input.to_path_buf()),
        })
    }

    fn extract(&self, input: &Path, source_lang: &str) -> Result<Vec<TextUnit>> {
        let v = load_json(input)?;
        let obj = v
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("mtool expects a flat JSON object"))?;
        let mut units = Vec::new();
        for (idx, (key, _)) in obj.iter().enumerate() {
            if !needs_translation(key, source_lang) {
                continue;
            }
            units.push(make_unit(
                "mtool",
                format!("k{idx:06}"),
                key,
                "",
                format!("s{:04}", idx / 50),
                key.clone(), // exact original key for writeback
            ));
        }
        Ok(units)
    }

    fn writeback(
        &self,
        input: &Path,
        target_lang: &str,
        units: &[TextUnit],
        translations: &BTreeMap<String, Translation>,
    ) -> Result<Vec<OutputFile>> {
        let mut v = load_json(input)?;
        let obj = v
            .as_object_mut()
            .ok_or_else(|| anyhow::anyhow!("mtool expects a flat JSON object"))?;
        for u in units {
            let Some(tr) = translations.get(&u.id) else {
                continue;
            };
            if let Some(slot) = obj.get_mut(&u.payload) {
                *slot = Value::String(joined(tr));
            }
        }
        pretty_output(input, target_lang, &v)
    }
}

// ------------------------------------------------------------ paratranz

impl FormatAdapter for ParatranzAdapter {
    fn id(&self) -> &'static str {
        "paratranz"
    }
    fn label(&self) -> &'static str {
        "Paratranz export"
    }
    fn extensions(&self) -> &'static [&'static str] {
        &["json"]
    }

    fn detect(&self, input: &Path) -> Option<DetectHit> {
        sniff(input, |v| {
            v.as_array()
                .and_then(|a| a.first())
                .is_some_and(|e| e.get("original").is_some() && e.get("key").is_some())
        })
        .then(|| DetectHit {
            engine_id: self.id(),
            label: self.label(),
            content_root: input.canonicalize().unwrap_or_else(|_| input.to_path_buf()),
        })
    }

    fn extract(&self, input: &Path, source_lang: &str) -> Result<Vec<TextUnit>> {
        let v = load_json(input)?;
        let arr = v
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("paratranz expects a JSON array"))?;
        let mut units = Vec::new();
        for (idx, e) in arr.iter().enumerate() {
            let Some(original) = e.get("original").and_then(|v| v.as_str()) else {
                continue;
            };
            let already = e
                .get("translation")
                .and_then(|v| v.as_str())
                .is_some_and(|s| !s.trim().is_empty());
            if already || !needs_translation(original, source_lang) {
                continue;
            }
            units.push(make_unit(
                "paratranz",
                format!("p{idx:06}"),
                original,
                "",
                format!("s{:04}", idx / 50),
                String::new(),
            ));
        }
        Ok(units)
    }

    fn writeback(
        &self,
        input: &Path,
        target_lang: &str,
        units: &[TextUnit],
        translations: &BTreeMap<String, Translation>,
    ) -> Result<Vec<OutputFile>> {
        let mut v = load_json(input)?;
        for u in units {
            let Some(tr) = translations.get(&u.id) else {
                continue;
            };
            let Ok(idx) = u.location.trim_start_matches('p').parse::<usize>() else {
                continue;
            };
            if let Some(obj) = v
                .as_array_mut()
                .and_then(|a| a.get_mut(idx))
                .and_then(|e| e.as_object_mut())
            {
                obj.insert("translation".into(), Value::String(joined(tr)));
            }
        }
        pretty_output(input, target_lang, &v)
    }
}

// ------------------------------------------------------------------ vnt

impl FormatAdapter for VntAdapter {
    fn id(&self) -> &'static str {
        "vnt"
    }
    fn label(&self) -> &'static str {
        "VNTextPatch export"
    }
    fn extensions(&self) -> &'static [&'static str] {
        &["json"]
    }

    fn detect(&self, input: &Path) -> Option<DetectHit> {
        sniff(input, |v| {
            v.as_array()
                .and_then(|a| a.first())
                .is_some_and(|e| e.get("message").is_some())
        })
        .then(|| DetectHit {
            engine_id: self.id(),
            label: self.label(),
            content_root: input.canonicalize().unwrap_or_else(|_| input.to_path_buf()),
        })
    }

    fn extract(&self, input: &Path, source_lang: &str) -> Result<Vec<TextUnit>> {
        let v = load_json(input)?;
        let arr = v
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("vnt expects a JSON array"))?;
        let mut units = Vec::new();
        for (idx, e) in arr.iter().enumerate() {
            let Some(message) = e.get("message").and_then(|v| v.as_str()) else {
                continue;
            };
            if !needs_translation(message, source_lang) {
                continue;
            }
            let role = e.get("name").and_then(|v| v.as_str()).unwrap_or("");
            units.push(make_unit(
                "vnt",
                format!("m{idx:06}"),
                message,
                role,
                format!("s{:04}", idx / 40),
                String::new(),
            ));
        }
        Ok(units)
    }

    fn writeback(
        &self,
        input: &Path,
        target_lang: &str,
        units: &[TextUnit],
        translations: &BTreeMap<String, Translation>,
    ) -> Result<Vec<OutputFile>> {
        let mut v = load_json(input)?;
        for u in units {
            let Some(tr) = translations.get(&u.id) else {
                continue;
            };
            let Ok(idx) = u.location.trim_start_matches('m').parse::<usize>() else {
                continue;
            };
            if let Some(obj) = v
                .as_array_mut()
                .and_then(|a| a.get_mut(idx))
                .and_then(|e| e.as_object_mut())
            {
                obj.insert("message".into(), Value::String(joined(tr)));
            }
        }
        pretty_output(input, target_lang, &v)
    }
}

// -------------------------------------------------------------- i18next

impl FormatAdapter for I18nextAdapter {
    fn id(&self) -> &'static str {
        "i18next"
    }
    fn label(&self) -> &'static str {
        "i18next / nested JSON strings"
    }
    fn extensions(&self) -> &'static [&'static str] {
        &["json"]
    }

    fn detect(&self, input: &Path) -> Option<DetectHit> {
        // Real i18n files occasionally hold a stray number/bool leaf — accept
        // when ≥80% of leaves are strings. Structured game data (rmmz maps…)
        // stays excluded by its low string ratio.
        sniff(input, |v| {
            if !v.is_object() {
                return false;
            }
            let (strings, others) = leaf_counts(v);
            strings > 0 && strings * 5 >= (strings + others) * 4
        })
        .then(|| DetectHit {
            engine_id: self.id(),
            label: self.label(),
            content_root: input.canonicalize().unwrap_or_else(|_| input.to_path_buf()),
        })
    }

    fn extract(&self, input: &Path, source_lang: &str) -> Result<Vec<TextUnit>> {
        let v = load_json(input)?;
        let mut units = Vec::new();
        let mut idx = 0usize;
        walk_strings(&v, &mut String::new(), &mut |path, text| {
            if needs_translation(text, source_lang) {
                units.push(make_unit(
                    "i18next",
                    path.to_string(),
                    text,
                    "",
                    format!("s{:04}", idx / 50),
                    String::new(),
                ));
            }
            idx += 1;
        });
        Ok(units)
    }

    fn writeback(
        &self,
        input: &Path,
        target_lang: &str,
        units: &[TextUnit],
        translations: &BTreeMap<String, Translation>,
    ) -> Result<Vec<OutputFile>> {
        let mut v = load_json(input)?;
        for u in units {
            let Some(tr) = translations.get(&u.id) else {
                continue;
            };
            if let Err(e) = set_json_path(&mut v, &u.location, Value::String(joined(tr))) {
                eprintln!("i18next: skip {}: {e}", u.location);
            }
        }
        pretty_output(input, target_lang, &v)
    }
}

/// `(string leaves, non-string leaves)` over the whole tree.
fn leaf_counts(v: &Value) -> (usize, usize) {
    match v {
        Value::String(_) => (1, 0),
        Value::Object(o) => o.values().fold((0, 0), |(s, n), c| {
            let (cs, cn) = leaf_counts(c);
            (s + cs, n + cn)
        }),
        Value::Array(a) => a.iter().fold((0, 0), |(s, n), c| {
            let (cs, cn) = leaf_counts(c);
            (s + cs, n + cn)
        }),
        _ => (0, 1),
    }
}

/// DFS over string leaves with slash paths (keys containing '/' are skipped —
/// they would collide with the path syntax).
fn walk_strings(v: &Value, path: &mut String, f: &mut impl FnMut(&str, &str)) {
    match v {
        Value::String(s) => f(path, s),
        Value::Object(o) => {
            for (k, child) in o {
                if k.contains('/') {
                    eprintln!("i18next: skip key with '/': {k}");
                    continue;
                }
                let len = path.len();
                if !path.is_empty() {
                    path.push('/');
                }
                path.push_str(k);
                walk_strings(child, path, f);
                path.truncate(len);
            }
        }
        Value::Array(a) => {
            for (i, child) in a.iter().enumerate() {
                let len = path.len();
                if !path.is_empty() {
                    path.push('/');
                }
                path.push_str(&i.to_string());
                walk_strings(child, path, f);
                path.truncate(len);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tr_for(units: &[TextUnit], text: &str) -> BTreeMap<String, Translation> {
        units
            .iter()
            .map(|u| {
                (
                    u.id.clone(),
                    Translation {
                        unit_id: u.id.clone(),
                        translation_lines: vec![text.to_string()],
                        source_hash: TextUnit::source_hash(&u.original_lines),
                        passthrough: false,
                    },
                )
            })
            .collect()
    }

    #[test]
    fn mtool_sniff_and_writeback() {
        let dir = super::super::test_dir("mtool");
        let input = dir.join("ManualTransFile.json");
        std::fs::write(&input, r#"{"こんにちは":"こんにちは","OK":"OK"}"#).unwrap();
        assert!(MtoolAdapter.detect(&input).is_some());
        assert!(ParatranzAdapter.detect(&input).is_none());
        let units = MtoolAdapter.extract(&input, "ja").unwrap();
        assert_eq!(units.len(), 1);
        let outs = MtoolAdapter
            .writeback(&input, "zh", &units, &tr_for(&units, "你好"))
            .unwrap();
        let v: Value = serde_json::from_slice(&outs[0].bytes).unwrap();
        assert_eq!(v["こんにちは"], "你好");
        assert_eq!(v["OK"], "OK");
    }

    #[test]
    fn paratranz_skips_translated() {
        let dir = super::super::test_dir("paratranz");
        let input = dir.join("p.json");
        std::fs::write(
            &input,
            r#"[{"key":"a","original":"未訳です","translation":""},{"key":"b","original":"済み","translation":"已译"}]"#,
        )
        .unwrap();
        assert!(ParatranzAdapter.detect(&input).is_some());
        let units = ParatranzAdapter.extract(&input, "ja").unwrap();
        assert_eq!(units.len(), 1);
        let outs = ParatranzAdapter
            .writeback(&input, "zh", &units, &tr_for(&units, "新译"))
            .unwrap();
        let v: Value = serde_json::from_slice(&outs[0].bytes).unwrap();
        assert_eq!(v[0]["translation"], "新译");
        assert_eq!(v[1]["translation"], "已译");
    }

    #[test]
    fn vnt_roundtrip() {
        let dir = super::super::test_dir("vnt");
        let input = dir.join("s.json");
        std::fs::write(
            &input,
            r#"[{"name":"ヒロイン","message":"負けました"},{"message":"..."}]"#,
        )
        .unwrap();
        assert!(VntAdapter.detect(&input).is_some());
        let units = VntAdapter.extract(&input, "ja").unwrap();
        assert_eq!(units.len(), 1);
        assert_eq!(units[0].role, "ヒロイン");
        let outs = VntAdapter
            .writeback(&input, "zh", &units, &tr_for(&units, "我输了"))
            .unwrap();
        let v: Value = serde_json::from_slice(&outs[0].bytes).unwrap();
        assert_eq!(v[0]["message"], "我输了");
    }

    #[test]
    fn i18next_nested_paths() {
        let dir = super::super::test_dir("i18next");
        let input = dir.join("ja.json");
        std::fs::write(
            &input,
            r#"{"menu":{"save":"保存する","items":["剣","盾"]}}"#,
        )
        .unwrap();
        assert!(I18nextAdapter.detect(&input).is_some());
        // rmmz-style data file must NOT match (has numbers)
        let notl10n = dir.join("Map001.json");
        std::fs::write(&notl10n, r#"{"events":[{"id":1,"name":"EV001"}]}"#).unwrap();
        assert!(I18nextAdapter.detect(&notl10n).is_none());

        let units = I18nextAdapter.extract(&input, "ja").unwrap();
        assert_eq!(units.len(), 3);
        let outs = I18nextAdapter
            .writeback(&input, "zh", &units, &tr_for(&units, "译"))
            .unwrap();
        let v: Value = serde_json::from_slice(&outs[0].bytes).unwrap();
        assert_eq!(v["menu"]["save"], "译");
        assert_eq!(v["menu"]["items"][0], "译");
    }
}
