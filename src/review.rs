//! Mechanical post-translate review. No LLM.
//!
//! Surfaces leftover source script, identical copies, dropped preserve tokens,
//! namebox/body drift, and glossary misses so an agent can fix via JSONL
//! instead of guessing.

use crate::glossary::{self, Glossary};
use crate::model::{TextUnit, Translation, has_hangul, has_kana, needs_translation};
use crate::preserve::{self, PreserveSet};
use crate::store;
use anyhow::Result;
use serde::Serialize;
use std::collections::BTreeMap;
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
    let mut identical = Vec::new();
    let mut control_loss = Vec::new();
    let mut passthrough = 0usize;
    let mut translated = 0usize;

    let want_kana = source_lang_is_ja(source_lang) && !target_lang_is_ja(target_lang);
    let want_hangul = source_lang_is_ko(source_lang) && !target_lang_is_ko(target_lang);

    for u in units {
        let Some(tr) = translations.get(&u.id) else {
            continue;
        };
        if tr.passthrough {
            passthrough += 1;
            continue;
        }
        translated += 1;
        let dst = tr.translation_lines.join("\n");
        if tr.translation_lines == u.original_lines
            && needs_translation(&u.joined_text(), source_lang)
        {
            identical.push(Hit {
                location: u.location.clone(),
                unit_id: u.id.clone(),
                detail: "translation identical to source".into(),
            });
        } else {
            if want_kana && has_kana(&dst) {
                residual.push(Hit {
                    location: u.location.clone(),
                    unit_id: u.id.clone(),
                    detail: "translation still contains kana".into(),
                });
            }
            if want_hangul && has_hangul(&dst) {
                residual.push(Hit {
                    location: u.location.clone(),
                    unit_id: u.id.clone(),
                    detail: "translation still contains hangul".into(),
                });
            }
        }
        let (_, map) = preserve.mask_unit_lines(&u.original_lines);
        let lost = preserve::lost_token_count(&tr.translation_lines, &map);
        if !map.is_empty() && lost * 2 >= map.len() {
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

fn source_lang_is_ja(src: &str) -> bool {
    let s = src.to_ascii_lowercase();
    s == "ja" || s.starts_with("jp")
}

fn source_lang_is_ko(src: &str) -> bool {
    src.to_ascii_lowercase().starts_with("ko")
}

fn target_lang_is_ja(dst: &str) -> bool {
    let s = dst.to_ascii_lowercase();
    s == "ja" || s.starts_with("jp")
}

fn target_lang_is_ko(dst: &str) -> bool {
    dst.to_ascii_lowercase().starts_with("ko")
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
        assert_eq!(report.residual_source.count, 1, "kana left in d1");
        assert_eq!(report.identical.count, 1, "d2 copied source");
        assert_eq!(report.namebox_mismatch.count, 1, "d1 did not use 艾蕾");
        assert_eq!(report.passthrough, 0);
        assert_eq!(report.translated, 3);
    }

    #[test]
    fn passthrough_is_not_a_quality_hit() {
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
        assert_eq!(report.residual_source.count, 0);
        assert_eq!(report.identical.count, 0);
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
}
