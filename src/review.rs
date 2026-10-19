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
    /// Source Latin text left inside a CJK translation. Advisory only.
    pub residual_latin: Bucket,
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
    let mut residual_latin = Vec::new();
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
        if !tr.passthrough && matches!(target.as_str(), "zh" | "ja" | "ko") && source != target {
            let fragments = latin_residue_fragments(&dst, &src);
            if !fragments.is_empty() {
                residual_latin.push(Hit {
                    location: u.location.clone(),
                    unit_id: u.id.clone(),
                    detail: format!("translation retains source Latin text: {}", fragments.join(", ")),
                });
            }
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
        residual_latin: Bucket::from_hits(residual_latin),
    }
}

/// LinguaGacha-style foreign residue: aggregate Latin-word runs in a CJK
/// translation, exempt isolated graphemes and 2–4 uppercase acronyms, and keep
/// only fragments that also occur in the source (so translator-invented product
/// names are not reported). Advisory only — never blocks writeback.
fn latin_residue_fragments<'a>(dst: &'a str, src: &str) -> Vec<&'a str> {
    let src_lower = src.to_ascii_lowercase();
    let src_bytes = src_lower.as_bytes();
    let mut fragments: Vec<&str> = Vec::new();
    for run in dst.split(|c: char| !c.is_ascii_alphabetic()) {
        // ASCII-only run, so byte length is grapheme length.
        if run.len() < 2 || (run.len() <= 4 && run.bytes().all(|b| b.is_ascii_uppercase())) {
            continue;
        }
        let bytes = run.as_bytes();
        if !src_bytes.windows(bytes.len()).any(|window| window.eq_ignore_ascii_case(bytes)) {
            continue;
        }
        if !fragments.iter().any(|seen| seen.eq_ignore_ascii_case(run)) {
            fragments.push(run);
            if fragments.len() == 3 {
                break;
            }
        }
    }
    fragments
}

/// Japanese shinjitai/kyūjitai and Chinese simplified/traditional variants that
/// a translator may legitimately use in place of the namebox spelling. Folding
/// is only for comparison; writeback never rewrites the translation.
fn fold_variant(c: char) -> char {
    match c {
        '汚' => '污',
        '櫻' | '桜' => '樱',
        '種' => '种',
        '實' => '实',
        '寶' => '宝',
        '國' => '国',
        '廣' => '广',
        '舊' => '旧',
        '眞' => '真',
        '劍' => '剑',
        '龍' => '龙',
        '鳳' => '凤',
        '戰' => '战',
        '鬥' => '斗',
        '氣' => '气',
        '樂' => '乐',
        '藥' => '药',
        '髮' => '发',
        '齒' => '齿',
        '歲' => '岁',
        '萬' => '万',
        '與' => '与',
        '為' => '为',
        '會' => '会',
        '來' => '来',
        '個' => '个',
        '們' => '们',
        '時' => '时',
        '說' => '说',
        '對' => '对',
        '現' => '现',
        '懷' => '怀',
        '爺' => '爷',
        '譯' => '译',
        '該' => '该',
        '難' => '难',
        '覺' => '觉',
        '愛' => '爱',
        '開' => '开',
        '這' => '这',
        '麼' => '么',
        '嗎' => '吗',
        '妳' => '你',
        '裡' | '裏' => '里',
        '隣' => '邻',
        '靜' => '静',
        '淨' => '净',
        '燒' => '烧',
        other => other,
    }
}

/// Fold variants and drop whitespace so `そら ♥ 桜月` and `空♥樱月` compare.
fn fold_name(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_whitespace())
        .map(fold_variant)
        .collect()
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
        // RMMZ reflow can split a name across 401 slots, so compare against the
        // joined translation rather than individual physical lines.
        let dst_name = nb_tr.translation_lines.concat();
        if src_name.trim().is_empty() || dst_name.trim().is_empty() {
            continue;
        }
        let folded_src = fold_name(&src_name);
        let folded_dst = fold_name(&dst_name);
        if folded_src.is_empty() || folded_dst.is_empty() {
            continue;
        }
        for u in units {
            if u.id == nb.id || u.context != nb.context {
                continue;
            }
            if !u.original_lines.iter().any(|l| fold_name(l).contains(folded_src.as_str())) {
                continue;
            }
            let Some(tr) = translations.get(&u.id) else {
                continue;
            };
            if tr.passthrough {
                continue;
            }
            let joined = tr.translation_lines.concat();
            if !fold_name(&joined).contains(folded_dst.as_str()) {
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

/// Units that cannot be written: the cached text is a passthrough placeholder,
/// fails structural/quality checks, or still carries target-script residue.
/// Namebox drift is deliberately absent — it is advisory and must never block a
/// writeback (a name may legitimately be rendered as a pronoun or split by
/// RMMZ message reflow).
pub fn blocking_unit_ids(
    units: &[TextUnit],
    translations: &BTreeMap<String, Translation>,
    source_lang: &str,
    target_lang: &str,
    preserve: &PreserveSet,
) -> BTreeSet<String> {
    units.iter().filter_map(|unit| {
        let tr = translations.get(&unit.id)?;
        (tr.passthrough || quality::check_translation(
            unit, &tr.translation_lines, source_lang, target_lang, preserve,
        ).is_err() || quality::kana_kind(&tr.translation_lines, target_lang, preserve).is_some())
            .then(|| unit.id.clone())
    }).collect()
}

/// Uncapped repair candidates. Glossary substring misses remain advisory because
/// inflected target forms can be correct without containing the exact glossary text.
/// Namebox drift is included so `repair` can still target it.
pub fn repair_unit_ids(
    units: &[TextUnit],
    translations: &BTreeMap<String, Translation>,
    _glossary: &Glossary,
    source_lang: &str,
    target_lang: &str,
    preserve: &PreserveSet,
) -> BTreeSet<String> {
    let mut ids = blocking_unit_ids(units, translations, source_lang, target_lang, preserve);
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
    fn namebox_drift_is_repair_only_not_blocking() {
        let units = vec![
            unit("nb", "namebox", "map1", "アレイ"),
            unit("d1", "dialogue", "map1", "アレイは村を出た"),
        ];
        let translations = BTreeMap::from([
            ("nb".into(), tr("nb", "艾蕾", false)),
            ("d1".into(), tr("d1", "她离开了村子", false)),
        ]);
        let blocking = blocking_unit_ids(&units, &translations, "ja", "zh", PreserveSet::core());
        assert!(blocking.is_empty(), "advisory namebox drift must not block writeback");
        let repair = repair_unit_ids(&units, &translations, &Glossary::default(), "ja", "zh", PreserveSet::core());
        assert_eq!(repair, BTreeSet::from(["d1".to_string()]));
        let report = inspect(&units, &translations, &Glossary::default(), "ja", "zh", PreserveSet::core());
        assert_eq!(report.namebox_mismatch.count, 1, "drift is still reported");
    }

    #[test]
    fn namebox_matches_across_reflow_split_lines() {
        let units = vec![
            unit("nb", "namebox", "map1", "アレイ"),
            unit("d1", "dialogue", "map1", "アレイは村を出た"),
        ];
        let mut dialogue = tr("d1", "艾蕾", false);
        dialogue.translation_lines.push("离开了村子".into());
        let translations = BTreeMap::from([
            ("nb".into(), tr("nb", "艾蕾", false)),
            ("d1".into(), dialogue),
        ]);
        let report = inspect(&units, &translations, &Glossary::default(), "ja", "zh", PreserveSet::core());
        assert_eq!(report.namebox_mismatch.count, 0, "name split by reflow still matches");
    }

    #[test]
    fn namebox_variant_folding_matches_simplified_forms() {
        let units = vec![
            unit("nb", "namebox", "map1", "矢理汚"),
            unit("nb2", "namebox", "map2", "桜月"),
            unit("d1", "dialogue", "map1", "矢理汚は笑った"),
            unit("d2", "dialogue", "map2", "そら♥桜月の大好きな乳首虐められてイケ♥"),
        ];
        let translations = BTreeMap::from([
            ("nb".into(), tr("nb", "矢理污", false)),
            ("nb2".into(), tr("nb2", "樱月", false)),
            ("d1".into(), tr("d1", "矢理污笑了", false)),
            ("d2".into(), tr("d2", "来吧♥你最喜欢的乳头被玩弄着……", false)),
        ]);
        let report = inspect(&units, &translations, &Glossary::default(), "ja", "zh", PreserveSet::core());
        assert_eq!(report.namebox_mismatch.count, 1, "only the intentionally pronominal rendering drifts");
        assert_eq!(report.namebox_mismatch.sample[0].unit_id, "d2");
    }

    #[test]
    fn pending_and_passthrough_still_block() {
        let units = vec![
            unit("pending", "dialogue", "m", "こんにちは"),
            unit("pass", "dialogue", "m", "こんばんは"),
        ];
        let translations = BTreeMap::from([("pass".into(), tr("pass", "こんばんは", true))]);
        let blocking = blocking_unit_ids(&units, &translations, "ja", "zh", PreserveSet::core());
        assert_eq!(blocking, BTreeSet::from(["pass".to_string()]), "passthrough blocks; pending is counted separately");
        let units_with_bad = vec![unit("bad", "dialogue", "m", "こんにちは")];
        let bad = BTreeMap::from([("bad".into(), tr("bad", "こんにちは", false))]);
        assert_eq!(
            blocking_unit_ids(&units_with_bad, &bad, "ja", "zh", PreserveSet::core()),
            BTreeSet::from(["bad".to_string()])
        );
    }

    #[test]
    fn latin_residue_is_advisory_and_exempts_acronyms() {
        let units = vec![
            unit("d", "dialogue", "m", "The ancient sword"),
            unit("a", "dialogue", "m", "HP restore"),
        ];
        let translations = BTreeMap::from([
            ("d".into(), tr("d", "古老的剑 sword", false)),
            ("a".into(), tr("a", "HP 恢复", false)),
        ]);
        let report = inspect(&units, &translations, &Glossary::default(), "en", "zh", PreserveSet::core());
        assert_eq!(report.residual_latin.count, 1);
        assert_eq!(report.residual_latin.sample[0].unit_id, "d");
        assert!(blocking_unit_ids(&units, &translations, "en", "zh", PreserveSet::core()).is_empty());
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
