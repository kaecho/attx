//! Mechanical post-translate review. No LLM.
//!
//! Surfaces leftover source script, identical copies, dropped preserve tokens,
//! namebox/body drift, and glossary misses so an agent can fix via JSONL
//! instead of guessing.

use crate::glossary::{self, Glossary};
use crate::model::{TextUnit, Translation};
use crate::preserve::{self, PreserveSet};
use crate::quality::{self, KanaKind};
use crate::store;
use anyhow::Result;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// ponytail: full dumps go through export-jsonl; this cap keeps the JSON report scannable.
const SAMPLE_CAP: usize = 40;

#[derive(Debug, Clone, Serialize)]
pub struct Hit {
    pub location: String,
    pub unit_id: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Bucket {
    pub count: usize,
    pub sample: Vec<Hit>,
}

impl Bucket {
    fn from_hits(mut hits: Vec<Hit>) -> Self {
        let count = hits.len();
        hits.truncate(SAMPLE_CAP);
        Self {
            count,
            sample: hits,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub total: usize,
    pub translated: usize,
    pub pending: usize,
    pub passthrough: usize,
    pub glossary: glossary::CheckReport,
    pub residual_source: Bucket,
    pub kana_edge: Bucket,
    pub kana_mixed: Bucket,
    pub kana_untranslated: Bucket,
    pub identical: Bucket,
    pub control_loss: Bucket,
    pub namebox_mismatch: Bucket,
}

pub fn review(workspace: &Path) -> Result<Report> {
    let store = store::workspace_db(workspace)?;
    let meta = store.meta()?;
    let units = store.all_units()?;
    let translations = store.all_translations()?;
    let glossary = glossary::load(workspace);
    let preserve = preserve::load(workspace, &meta.engine);
    Ok(inspect(
        &units,
        &translations,
        &glossary,
        &meta.source_lang,
        &meta.target_lang,
        &preserve,
    ))
}

pub fn inspect(
    units: &[TextUnit],
    translations: &BTreeMap<String, Translation>,
    glossary: &Glossary,
    source_lang: &str,
    target_lang: &str,
    preserve: &PreserveSet,
) -> Report {
    let mut residual = Vec::new();
    let mut kana_edge = Vec::new();
    let mut kana_mixed = Vec::new();
    let mut kana_untranslated = Vec::new();
    let mut identical = Vec::new();
    let mut control_loss = Vec::new();
    let mut passthrough = 0usize;
    let mut translated = 0usize;
    let source = quality::language(source_lang);
    let target = quality::language(target_lang);

    for u in units {
        let Some(tr) = translations.get(&u.id) else {
            continue;
        };
        if tr.passthrough {
            passthrough += 1;
        } else {
            translated += 1;
        }
        let lines = if tr.passthrough { &u.original_lines } else { &tr.translation_lines };
        let src = quality::visible_lines(&u.original_lines, preserve).join("\n");
        let visible_dst = quality::visible_lines(lines, preserve);
        let dst = visible_dst.join("\n");
        let copied = src.trim() == dst.trim() && quality::identical_requires_translation(&src, source_lang, target_lang);
        if copied {
            identical.push(hit(u, "translation identical to source"));
        }
        let mut kind = quality::kana_kind_visible(&visible_dst, &target);
        if copied && source == "ja" {
            kind = Some(KanaKind::Untranslated);
        }
        let detail = match kind {
            Some(KanaKind::Edge) => {
                let detail = "Chinese translation contains mechanical kana edge residue";
                kana_edge.push(hit(u, detail));
                Some(detail)
            }
            Some(KanaKind::Mixed) => {
                let detail = "translation mixes target text with Japanese kana";
                kana_mixed.push(hit(u, detail));
                Some(detail)
            }
            Some(KanaKind::Untranslated) => {
                let detail = "Japanese text remains untranslated";
                kana_untranslated.push(hit(u, detail));
                Some(detail)
            }
            None => quality::residual_script(&dst, source_lang, target_lang).map(|script| match script {
                "hangul" => "translation still contains hangul",
                "Cyrillic" => "translation still contains Cyrillic",
                "Arabic" => "translation still contains Arabic",
                "Devanagari" => "translation still contains Devanagari",
                "Thai" => "translation still contains Thai",
                _ => "translation still contains source CJK",
            }),
        };
        if let Some(detail) = detail {
            residual.push(hit(u, detail));
        }
        let map = quality::protected_tokens(&u.original_lines, preserve);
        let lost = preserve::lost_token_count(lines, &map);
        if lost > 0 {
            control_loss.push(Hit {
                location: u.location.clone(),
                unit_id: u.id.clone(),
                detail: format!("preserved tokens lost: {lost}/{}", map.len()),
            });
        }
    }

    let namebox_mismatch = namebox_hits(units, translations);

    Report {
        total: units.len(),
        translated,
        pending: units
            .iter()
            .filter(|u| translations.get(&u.id).is_none())
            .count(),
        passthrough,
        glossary: glossary::check_units(units, translations, glossary),
        residual_source: Bucket::from_hits(residual),
        kana_edge: Bucket::from_hits(kana_edge),
        kana_mixed: Bucket::from_hits(kana_mixed),
        kana_untranslated: Bucket::from_hits(kana_untranslated),
        identical: Bucket::from_hits(identical),
        control_loss: Bucket::from_hits(control_loss),
        namebox_mismatch: Bucket::from_hits(namebox_mismatch),
    }
}


fn namebox_hits(
    units: &[TextUnit],
    translations: &BTreeMap<String, Translation>,
) -> Vec<Hit> {
    let mut hits = Vec::new();
    for nb in units.iter().filter(|u| is_namebox(u)) {
        let Some(nb_tr) = translations.get(&nb.id) else {
            continue;
        };
        if nb_tr.passthrough {
            continue;
        }
        let src_name = nb.joined_text();
        let dst_name = nb_tr.translation_lines.join("\n");
        if src_name.trim().is_empty() || dst_name.trim().is_empty() {
            continue;
        }
        for u in units {
            if u.id == nb.id || u.context != nb.context {
                continue;
            }
            if !u.original_lines.iter().any(|l| l.contains(&src_name)) {
                continue;
            }
            let Some(tr) = translations.get(&u.id) else {
                continue;
            };
            if tr.passthrough {
                continue;
            }
            if !tr.translation_lines.iter().any(|l| l.contains(&dst_name)) {
                hits.push(Hit {
                    location: u.location.clone(),
                    unit_id: u.id.clone(),
                    detail: format!(
                        "namebox `{src_name}` → `{dst_name}` not used in this unit"
                    ),
                });
            }
        }
    }
    hits
}

pub fn is_namebox(u: &TextUnit) -> bool {
    u.domain == "namebox" || u.role == "namebox"
}

fn hit(unit: &TextUnit, detail: &str) -> Hit {
    Hit {
        location: unit.location.clone(),
        unit_id: unit.id.clone(),
        detail: detail.into(),
    }
}

/// Uncapped repair candidates. Glossary substring misses remain advisory because
/// inflected target forms can be correct without containing the exact glossary text.
pub fn repair_unit_ids(
    units: &[TextUnit],
    translations: &BTreeMap<String, Translation>,
    _glossary: &Glossary,
    source_lang: &str,
    target_lang: &str,
    preserve: &PreserveSet,
) -> BTreeSet<String> {
    let mut ids: BTreeSet<String> = units.iter().filter_map(|unit| {
        let tr = translations.get(&unit.id)?;
        (tr.passthrough || quality::check_translation(
            unit, &tr.translation_lines, source_lang, target_lang, preserve,
        ).is_err() || quality::kana_kind(&tr.translation_lines, target_lang, preserve).is_some())
            .then(|| unit.id.clone())
    }).collect();
    ids.extend(namebox_hits(units, translations).into_iter().map(|hit| hit.unit_id));
    ids
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ItemType;

    fn unit(id: &str, domain: &str, context: &str, text: &str) -> TextUnit {
        TextUnit {
            id: id.into(),
            engine: "rmmz".into(),
            domain: domain.into(),
            location: id.into(),
            item_type: ItemType::ShortText,
            role: if domain == "namebox" {
                "namebox".into()
            } else {
                String::new()
            },
            original_lines: vec![text.into()],
            source_line_paths: vec![],
            context: context.into(),
            payload: String::new(),
        }
    }

    fn tr(id: &str, text: &str, passthrough: bool) -> Translation {
        Translation {
            unit_id: id.into(),
            translation_lines: vec![text.into()],
            source_hash: String::new(),
            passthrough,
        }
    }

    #[test]
    fn residual_kana_and_identical_and_namebox() {
        let units = vec![
            unit("nb", "namebox", "map1", "アレイ"),
            unit("d1", "dialogue", "map1", "アレイは村を出た"),
            unit("d2", "dialogue", "map1", "今日はいい天気"),
        ];
        let mut translations = BTreeMap::new();
        translations.insert("nb".into(), tr("nb", "艾蕾", false));
        translations.insert("d1".into(), tr("d1", "アレイ离开了村子", false));
        translations.insert("d2".into(), tr("d2", "今日はいい天気", false));
        let g = Glossary::default();
        let report = inspect(
            &units,
            &translations,
            &g,
            "ja",
            "zh",
            PreserveSet::core(),
        );
        assert_eq!(report.residual_source.count, 2, "mixed and copied Japanese");
        assert_eq!(report.kana_mixed.count, 1);
        assert_eq!(report.kana_untranslated.count, 1);
        assert_eq!(report.identical.count, 1, "d2 copied source");
        assert_eq!(report.namebox_mismatch.count, 1, "d1 did not use 艾蕾");
        assert_eq!(report.passthrough, 0);
        assert_eq!(report.translated, 3);
    }

    #[test]
    fn untranslated_passthrough_is_a_quality_hit() {
        let units = vec![unit("d", "dialogue", "m", "こんにちは")];
        let mut translations = BTreeMap::new();
        translations.insert("d".into(), tr("d", "こんにちは", true));
        let report = inspect(
            &units,
            &translations,
            &Glossary::default(),
            "ja",
            "zh",
            PreserveSet::core(),
        );
        assert_eq!(report.passthrough, 1);
        assert_eq!(report.residual_source.count, 1);
        assert_eq!(report.kana_untranslated.count, 1);
        assert_eq!(report.identical.count, 1);
    }

    #[test]
    fn control_loss_from_preserve_tokens() {
        let units = vec![unit("d", "dialogue", "m", "got {item}")];
        let mut translations = BTreeMap::new();
        translations.insert("d".into(), tr("d", "拿到了东西", false));
        let report = inspect(
            &units,
            &translations,
            &Glossary::default(),
            "en",
            "zh",
            PreserveSet::core(),
        );
        assert_eq!(report.control_loss.count, 1);
    }

    #[test]
    fn kana_categories_are_disjoint_and_protected_literals_are_exempt() {
        let units = vec![
            unit("edge", "dialogue", "m", "恥ずかしい"),
            unit("mixed", "dialogue", "m", "くの形"),
            unit("untranslated", "dialogue", "m", "おはよう"),
            unit("identical", "dialogue", "m", "こんにちは"),
            unit("passthrough", "dialogue", "m", "こんばんは"),
            unit("protected", "dialogue", "m", r"\SE[カナ]"),
            unit("separator", "dialogue", "m", "・箇条書き"),
        ];
        let translations = BTreeMap::from([
            ("edge".into(), tr("edge", "好羞耻……っ", false)),
            ("mixed".into(), tr("mixed", "反弓成「く」字形", false)),
            ("untranslated".into(), tr("untranslated", "今日はいい天気", false)),
            ("identical".into(), tr("identical", "こんにちは", false)),
            ("passthrough".into(), tr("passthrough", "こんばんは", true)),
            ("protected".into(), tr("protected", r"\SE[カナ]", false)),
            ("separator".into(), tr("separator", "・禁止挑食", false)),
        ]);
        let report = inspect(&units, &translations, &Glossary::default(), "ja-JP", "ZH_tw", PreserveSet::core());
        assert_eq!(report.kana_edge.count, 1);
        assert_eq!(report.kana_mixed.count, 1);
        assert_eq!(report.kana_untranslated.count, 3);
        assert_eq!(report.residual_source.count, 5);
        assert_eq!(report.identical.count, 2);
        assert_eq!(report.control_loss.count, 0);
        assert_eq!(report.kana_edge.sample[0].location, "edge");
        let ids: BTreeSet<_> = report.kana_edge.sample.iter()
            .chain(&report.kana_mixed.sample).chain(&report.kana_untranslated.sample)
            .map(|hit| hit.unit_id.as_str()).collect();
        assert_eq!(ids.len(), 5);
    }

    #[test]
    fn any_preserve_token_loss_is_reported() {
        let units = vec![unit("d", "dialogue", "m", "got {a} {b} {c}")];
        let translations = BTreeMap::from([("d".into(), tr("d", "拿到了 {a} {b}", false))]);
        let report = inspect(&units, &translations, &Glossary::default(), "en", "zh", PreserveSet::core());
        assert_eq!(report.control_loss.count, 1);
        assert!(report.control_loss.sample[0].detail.contains("1/3"));
    }

    #[test]
    fn repair_candidates_are_uncapped_and_exclude_pending() {
        let mut units = Vec::new();
        let mut translations = BTreeMap::new();
        for i in 0..45 {
            let id = format!("d{i}");
            units.push(unit(&id, "dialogue", "m", "こんにちは"));
            translations.insert(id.clone(), tr(&id, "こんにちは", false));
        }
        units.push(unit("pending", "dialogue", "m", "こんにちは"));
        units.push(unit("protected", "dialogue", "m", r"\SE[カナ]"));
        translations.insert("protected".into(), tr("protected", r"\SE[カナ]", false));
        let ids = repair_unit_ids(&units, &translations, &Glossary::default(), "ja", "zh", PreserveSet::core());
        assert_eq!(ids.len(), 45);
        assert!(!ids.contains("pending"));
        assert!(!ids.contains("protected"));
        let report = inspect(&units, &translations, &Glossary::default(), "ja", "zh", PreserveSet::core());
        assert_eq!(report.kana_untranslated.count, 45);
        assert_eq!(report.kana_untranslated.sample.len(), SAMPLE_CAP);
    }

    #[test]
    fn other_source_scripts_report_without_kana_categories() {
        let units = vec![unit("d", "dialogue", "m", "안녕")];
        let translations = BTreeMap::from([("d".into(), tr("d", "你好안녕", false))]);
        let report = inspect(&units, &translations, &Glossary::default(), "KO_kr", "zh", PreserveSet::core());
        assert_eq!(report.residual_source.count, 1);
        assert_eq!(report.kana_edge.count + report.kana_mixed.count + report.kana_untranslated.count, 0);
        let japanese = vec![unit("j", "dialogue", "m", "こんにちは")];
        let japanese_tr = BTreeMap::from([("j".into(), tr("j", "こんにちは", false))]);
        let report = inspect(&japanese, &japanese_tr, &Glossary::default(), "ja", "JA_jp", PreserveSet::core());
        assert_eq!(report.residual_source.count, 0);
        assert_eq!(report.identical.count, 0);
    }
}
