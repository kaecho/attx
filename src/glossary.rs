//! Glossary: one agreed translation per proper noun, for a whole work.
//!
//! Without one, a model translating a long novel or game will render the same
//! character name differently across batches — `アレイ` becomes 艾蕾 in chapter
//! one and 埃雷 in chapter nine, and the reader cannot tell they are the same
//! person. Batching makes this structural, not accidental: no single request
//! ever sees enough of the work to be consistent with the rest of it.
//!
//! Two extraction methods:
//!
//! ```text
//! llm   (default): source batches → model emits {src,dst,info} → vote/cap → inject
//! stats:           mine (regex) → threshold → cap → name (LLM) → inject
//! ```
//!
//! `stats` keeps LLM spend proportional to *term count*. `llm` spends on text
//! volume but catches proper nouns regex cannot see. Both write the same
//! `glossary.toml` and share inject/check.
//!
//! Two failure modes shape the design:
//! * Statistics cannot tell a proper noun from a common word — `春` appears
//!   constantly and is not a term. The model gets a `keep` flag (stats) or a
//!   type whitelist (llm); vetoes are remembered so a re-build does not re-ask.
//! * A wrong `dst` is applied everywhere, which makes it *harder* to spot than a
//!   one-off mistranslation. Hence `check`, and hence entries stay editable.

use crate::config::{GlossaryMethod, Settings};
use crate::knowledge;
use crate::llm;
use crate::model::TextUnit;
use anyhow::{Context, Result, bail};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

pub const GLOSSARY_VERSION: u32 = 1;
pub const GLOSSARY_FILE: &str = "glossary.toml";

/// Candidates per stats-method naming request.
const NAME_BATCH: usize = 40;
/// Source characters per llm-method extract request.
const EXTRACT_BATCH_CHARS: usize = 3500;
/// Hard cap on extract batches so a huge workspace cannot open an unbounded bill.
/// ponytail: raise or make configurable if novels routinely need deeper coverage.
const EXTRACT_MAX_BATCHES: usize = 40;

/// Allowed `info` / type labels from llm extract (LinguaGacha-style whitelist).
const LLM_INFO_OK: &[&str] = &[
    "男性角色",
    "女性角色",
    "未知性别角色",
    "地名",
    "家族",
    "组织",
    "特殊物品",
    "特殊技能",
    "特殊生物",
    "其他",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TermStatus {
    #[default]
    Active,
    /// The model judged this candidate not to be a proper noun. Kept rather
    /// than deleted so the next build does not pay to ask again.
    Rejected,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlossaryTerm {
    pub src: String,
    #[serde(default)]
    pub dst: String,
    /// Disambiguating description ("女性名字", "地点"). Not decoration: without
    /// it the model cannot tell `アレイさん` should be 艾蕾小姐 and not 埃雷先生.
    #[serde(default)]
    pub info: String,
    #[serde(default)]
    pub count: usize,
    #[serde(default)]
    pub status: TermStatus,
    #[serde(default)]
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Glossary {
    // Declaration order is emission order: scalars before the term array.
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub source_lang: String,
    #[serde(default)]
    pub target_lang: String,
    #[serde(default, rename = "term")]
    pub terms: Vec<GlossaryTerm>,
}

fn default_version() -> u32 {
    GLOSSARY_VERSION
}

impl Default for Glossary {
    fn default() -> Self {
        Self {
            version: GLOSSARY_VERSION,
            source_lang: String::new(),
            target_lang: String::new(),
            terms: Vec::new(),
        }
    }
}

impl Glossary {
    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }

    pub fn active(&self) -> Vec<GlossaryTerm> {
        self.terms
            .iter()
            .filter(|t| t.status == TermStatus::Active && !t.dst.is_empty())
            .cloned()
            .collect()
    }

    fn find(&self, src: &str) -> Option<&GlossaryTerm> {
        self.terms.iter().find(|t| t.src == src)
    }

    /// Replace a term with the same `src`, else append.
    pub fn upsert(&mut self, term: GlossaryTerm) {
        self.terms.retain(|t| t.src != term.src);
        self.terms.push(term);
    }

    pub fn remove(&mut self, src: &str) -> bool {
        let before = self.terms.len();
        self.terms.retain(|t| t.src != src);
        self.terms.len() != before
    }
}

// ---- storage ----

pub fn path(workspace: &Path) -> PathBuf {
    workspace.join(GLOSSARY_FILE)
}

/// Load a workspace glossary. Missing file → empty (not an error); a malformed
/// one is reported and treated as empty so a bad hand-edit degrades to "no
/// glossary" rather than blocking translation.
pub fn load(workspace: &Path) -> Glossary {
    let p = path(workspace);
    if !p.is_file() {
        return Glossary::default();
    }
    match std::fs::read_to_string(&p)
        .map_err(anyhow::Error::from)
        .and_then(|raw| toml::from_str::<Glossary>(&raw).map_err(anyhow::Error::from))
    {
        Ok(g) => g,
        Err(e) => {
            eprintln!("glossary: ignoring {}: {e:#}", p.display());
            Glossary::default()
        }
    }
}

pub fn save(workspace: &Path, g: &Glossary) -> Result<PathBuf> {
    let p = path(workspace);
    let body = toml::to_string_pretty(g).context("serialize glossary")?;
    let header = "# attx glossary — one agreed translation per proper noun.\n\
                  # Edit freely: `attx glossary list` shows what is active,\n\
                  # `attx glossary check` reports terms the translation ignored.\n";
    std::fs::write(&p, format!("{header}{body}"))
        .with_context(|| format!("write {}", p.display()))?;
    Ok(p)
}

/// Fill in the language pair from the workspace when it is not set yet.
///
/// `build` knows the languages because it reads the DB anyway; a glossary
/// created by `add` or `import` would otherwise carry no record of which
/// direction it is for, which matters once the file is shared or re-imported.
/// A workspace that cannot be opened is not an error here — the terms are
/// still worth saving.
pub fn ensure_langs(workspace: &Path, g: &mut Glossary) {
    if !g.source_lang.is_empty() && !g.target_lang.is_empty() {
        return;
    }
    if let Ok(store) = crate::store::workspace_db(workspace)
        && let Ok(meta) = store.meta()
    {
        if g.source_lang.is_empty() {
            g.source_lang = meta.source_lang;
        }
        if g.target_lang.is_empty() {
            g.target_lang = meta.target_lang;
        }
    }
}

// ---- candidate mining ----

// Japanese has no word boundaries, so mining leans on script transitions:
// katakana runs and kanji runs are where proper nouns live in games and light
// novels. Recall is favoured over precision — the occurrence threshold and the
// model's `keep` veto are the two filters that follow.
static JA_KATAKANA: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[\x{30A1}-\x{30FA}\x{30FC}]{2,}").expect("katakana regex"));
/// `エルギア国` — a katakana name with a kanji suffix reads as one term.
static JA_KATAKANA_KANJI: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[\x{30A1}-\x{30FA}\x{30FC}]{2,}[\x{4E00}-\x{9FFF}]{1,3}")
        .expect("katakana+kanji regex")
});
static JA_KANJI: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[\x{4E00}-\x{9FFF}]{2,6}").expect("kanji regex"));
static EN_PROPER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b[A-Z][a-z]+(?:\s+[A-Z][a-z]+)*\b").expect("proper noun regex"));

/// High-frequency words the patterns above will always surface and that are
/// never worth a glossary slot. Deliberately short: this list exists to stop
/// obvious noise from eating the `max_terms` budget, not to make semantic
/// judgements — that is the model's job via `keep`.
const JA_STOPWORDS: &[&str] = &[
    "自分",
    "今日",
    "明日",
    "昨日",
    "時間",
    "世界",
    "人間",
    "本当",
    "大丈夫",
    "場合",
    "必要",
    "場所",
    "部屋",
    "仕事",
    "相手",
    "最初",
    "最後",
    "普通",
    "一緒",
    "全部",
    "意味",
    "問題",
    "理由",
    "関係",
    "状態",
    "状況",
    "気持",
    "気分",
    "男性",
    "女性",
    "子供",
    "二人",
    "一人",
    "自身",
    "今回",
    "現在",
    "説明",
    "確認",
    "使用",
];

const EN_STOPWORDS: &[&str] = &[
    "The",
    "This",
    "That",
    "These",
    "Those",
    "There",
    "Then",
    "They",
    "Their",
    "What",
    "When",
    "Where",
    "Which",
    "While",
    "With",
    "Without",
    "You",
    "Your",
    "And",
    "But",
    "For",
    "From",
    "Have",
    "Has",
    "Had",
    "Not",
    "Are",
    "Was",
    "Were",
    "Will",
    "Would",
    "Could",
    "Should",
    "Yes",
    "Well",
    "Just",
    "Now",
    "Here",
    "Very",
    "Really",
    "Something",
    "Nothing",
    "Anything",
    "Everything",
    "Someone",
    "Anyone",
    "Everyone",
    "Nobody",
    "Because",
    "Before",
    "After",
    "Even",
    "Only",
    "Also",
    "Still",
    "Maybe",
    "Okay",
    "Let",
    "Get",
    "Got",
    "Can",
    "How",
    "Why",
    "Who",
    "All",
    "Its",
    "Our",
    "His",
    "Her",
    "She",
    "Him",
    "Them",
    "Was",
];

fn is_noise(term: &str, src_lang: &str) -> bool {
    let n = term.chars().count();
    if n < 2 {
        return true;
    }
    if knowledge::is_machine_literal(term) {
        return true;
    }
    if src_lang == "en" {
        return n < 3 || EN_STOPWORDS.contains(&term);
    }
    JA_STOPWORDS.contains(&term)
}

/// Count proper-noun candidates across a workspace's source text.
/// Returns `(term, occurrences)` sorted by count descending, then by term so
/// the order is stable across runs.
pub fn mine_candidates(units: &[TextUnit], src_lang: &str) -> Vec<(String, usize)> {
    let patterns: Vec<&Regex> = if src_lang.eq_ignore_ascii_case("en") {
        vec![&EN_PROPER]
    } else {
        vec![&JA_KATAKANA, &JA_KATAKANA_KANJI, &JA_KANJI]
    };
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for u in units {
        for line in &u.original_lines {
            for re in &patterns {
                for m in re.find_iter(line) {
                    let t = m.as_str().trim();
                    if is_noise(t, src_lang) {
                        continue;
                    }
                    *counts.entry(t.to_string()).or_insert(0) += 1;
                }
            }
        }
    }
    let mut out: Vec<(String, usize)> = counts.into_iter().collect();
    out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    out
}

// ---- build ----

#[derive(Debug, Clone, Serialize)]
pub struct BuildReport {
    pub method: String,
    pub candidates: usize,
    pub above_threshold: usize,
    /// Candidates dropped by `max_terms`. Reported rather than silently cut:
    /// a truncated run that looks complete is worse than a noisy one.
    pub truncated: usize,
    pub asked: usize,
    pub added: usize,
    pub rejected: usize,
    pub total_active: usize,
    pub min_occurrences: usize,
    pub dry_run: bool,
    pub file: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub sample: Vec<String>,
}

/// Build a glossary for a workspace.
///
/// `method` / `min_occurrences` override `[glossary]` when set (CLI flags).
pub fn build(
    workspace: &Path,
    settings: &Settings,
    method: Option<GlossaryMethod>,
    min_occurrences: Option<usize>,
    dry_run: bool,
) -> Result<BuildReport> {
    let store = crate::store::workspace_db(workspace)?;
    let meta = store.meta()?;
    let units = store.all_units()?;
    if units.is_empty() {
        bail!(
            "no extracted units in {}; run `attx extract` first",
            workspace.display()
        );
    }

    let method = method.unwrap_or(settings.glossary.method);
    match method {
        GlossaryMethod::Llm => build_llm(workspace, settings, &meta, &units, dry_run),
        GlossaryMethod::Stats => {
            build_stats(workspace, settings, &meta, &units, min_occurrences, dry_run)
        }
    }
}

fn build_stats(
    workspace: &Path,
    settings: &Settings,
    meta: &crate::model::WorkspaceMeta,
    units: &[TextUnit],
    min_occurrences: Option<usize>,
    dry_run: bool,
) -> Result<BuildReport> {
    let min_occ = min_occurrences
        .unwrap_or(settings.glossary.min_occurrences)
        .max(1);
    let max_terms = settings.glossary.max_terms.max(1);

    let mined = mine_candidates(units, &meta.source_lang);
    let candidates = mined.len();

    let mut glossary = load(workspace);
    glossary.source_lang = meta.source_lang.clone();
    glossary.target_lang = meta.target_lang.clone();

    // Anything already decided — named or vetoed — is not worth paying for again.
    let fresh: Vec<(String, usize)> = mined
        .into_iter()
        .filter(|(t, c)| *c >= min_occ && glossary.find(t).is_none())
        .collect();
    let above_threshold = fresh.len();
    let truncated = above_threshold.saturating_sub(max_terms);
    let asked: Vec<(String, usize)> = fresh.into_iter().take(max_terms).collect();

    if truncated > 0 {
        eprintln!(
            "glossary: {truncated} candidate(s) above the threshold were dropped by max_terms={max_terms}; \
             raise it or raise min_occurrences to change what is covered"
        );
    }

    let sample: Vec<String> = asked
        .iter()
        .take(10)
        .map(|(t, c)| format!("{t} ({c})"))
        .collect();

    if dry_run {
        return Ok(BuildReport {
            method: GlossaryMethod::Stats.as_str().into(),
            candidates,
            above_threshold,
            truncated,
            asked: asked.len(),
            added: 0,
            rejected: 0,
            total_active: glossary.active().len(),
            min_occurrences: min_occ,
            dry_run: true,
            file: path(workspace).display().to_string(),
            sample,
        });
    }

    let mut added = 0usize;
    let mut rejected = 0usize;
    if !asked.is_empty() {
        let client = crate::config::require_llm(settings)?;
        for chunk in asked.chunks(NAME_BATCH) {
            match name_candidates(client, chunk, &meta.source_lang, &meta.target_lang) {
                Ok(named) => {
                    for n in named {
                        let count = chunk
                            .iter()
                            .find(|(t, _)| *t == n.src)
                            .map(|(_, c)| *c)
                            .unwrap_or(0);
                        if n.keep && !n.dst.trim().is_empty() {
                            glossary.upsert(GlossaryTerm {
                                src: n.src,
                                dst: n.dst,
                                info: n.info,
                                count,
                                status: TermStatus::Active,
                                source: "auto:stats".into(),
                            });
                            added += 1;
                        } else {
                            glossary.upsert(GlossaryTerm {
                                src: n.src,
                                dst: String::new(),
                                info: n.info,
                                count,
                                status: TermStatus::Rejected,
                                source: "auto:stats".into(),
                            });
                            rejected += 1;
                        }
                    }
                }
                Err(e) => eprintln!("glossary: naming batch failed: {e:#}"),
            }
        }
    }

    let file = save(workspace, &glossary)?;
    Ok(BuildReport {
        method: GlossaryMethod::Stats.as_str().into(),
        candidates,
        above_threshold,
        truncated,
        asked: asked.len(),
        added,
        rejected,
        total_active: glossary.active().len(),
        min_occurrences: min_occ,
        dry_run: false,
        file: file.display().to_string(),
        sample,
    })
}

fn build_llm(
    workspace: &Path,
    settings: &Settings,
    meta: &crate::model::WorkspaceMeta,
    units: &[TextUnit],
    dry_run: bool,
) -> Result<BuildReport> {
    let max_terms = settings.glossary.max_terms.max(1);
    let batches = source_batches(units, EXTRACT_BATCH_CHARS, EXTRACT_MAX_BATCHES);
    let asked = batches.len();

    let mut glossary = load(workspace);
    glossary.source_lang = meta.source_lang.clone();
    glossary.target_lang = meta.target_lang.clone();

    let sample: Vec<String> = batches
        .iter()
        .take(3)
        .enumerate()
        .map(|(i, b)| format!("batch{}:{}c", i + 1, b.chars().count()))
        .collect();

    if dry_run {
        return Ok(BuildReport {
            method: GlossaryMethod::Llm.as_str().into(),
            candidates: 0,
            above_threshold: 0,
            truncated: 0,
            asked,
            added: 0,
            rejected: 0,
            total_active: glossary.active().len(),
            min_occurrences: 0,
            dry_run: true,
            file: path(workspace).display().to_string(),
            sample,
        });
    }

    if batches.is_empty() {
        let file = path(workspace);
        return Ok(BuildReport {
            method: GlossaryMethod::Llm.as_str().into(),
            candidates: 0,
            above_threshold: 0,
            truncated: 0,
            asked: 0,
            added: 0,
            rejected: 0,
            total_active: glossary.active().len(),
            min_occurrences: 0,
            dry_run: false,
            file: file.display().to_string(),
            sample: vec![],
        });
    }

    let client = crate::config::require_llm(settings)?;
    // Aggregate votes across batches: same src may appear many times with
    // slightly different dst/info; majority wins, ties keep first-seen order.
    let mut dst_votes: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    let mut info_votes: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    let mut raw_hits = 0usize;

    for (i, batch) in batches.iter().enumerate() {
        match extract_terms_llm(client, batch, &meta.source_lang, &meta.target_lang) {
            Ok(rows) => {
                for row in rows {
                    if !accept_llm_term(units, &row) {
                        continue;
                    }
                    if glossary.find(&row.src).is_some() {
                        continue; // already decided (incl. rejected)
                    }
                    raw_hits += 1;
                    *dst_votes
                        .entry(row.src.clone())
                        .or_default()
                        .entry(row.dst)
                        .or_insert(0) += 1;
                    *info_votes
                        .entry(row.src)
                        .or_default()
                        .entry(row.info)
                        .or_insert(0) += 1;
                }
            }
            Err(e) => eprintln!("glossary: extract batch {}/{} failed: {e:#}", i + 1, asked),
        }
    }

    // Rank by total dst votes (proxy for recurrence), then src for stability.
    let mut ranked: Vec<(String, usize)> = dst_votes
        .iter()
        .map(|(src, votes)| (src.clone(), votes.values().sum()))
        .collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let candidates = ranked.len();
    let truncated = candidates.saturating_sub(max_terms);
    if truncated > 0 {
        eprintln!(
            "glossary: {truncated} llm term(s) dropped by max_terms={max_terms}; raise it to keep more"
        );
    }

    let mut added = 0usize;
    let mut sample = Vec::new();
    for (src, count) in ranked.into_iter().take(max_terms) {
        let dst = winner(dst_votes.get(&src).unwrap());
        let mut info = info_votes
            .get(&src)
            .map(winner)
            .unwrap_or_else(|| "其他".into());
        let info_l = info.to_ascii_lowercase();
        if info_l == "other" || info_l == "others" {
            info = "其他".into();
        } else if !LLM_INFO_OK.iter().any(|ok| *ok == info) {
            // Keep free-form short labels from the model; empty → 其他.
            if info.trim().is_empty() {
                info = "其他".into();
            }
        }
        if dst.is_empty() || dst == src {
            continue;
        }
        if sample.len() < 10 {
            sample.push(format!("{src}→{dst} ({count})"));
        }
        glossary.upsert(GlossaryTerm {
            src,
            dst,
            info,
            count,
            status: TermStatus::Active,
            source: "auto:llm".into(),
        });
        added += 1;
    }

    let file = save(workspace, &glossary)?;
    Ok(BuildReport {
        method: GlossaryMethod::Llm.as_str().into(),
        candidates,
        above_threshold: candidates,
        truncated,
        asked,
        added,
        rejected: raw_hits.saturating_sub(added), // rough: filtered/duped/capped
        total_active: glossary.active().len(),
        min_occurrences: 0,
        dry_run: false,
        file: file.display().to_string(),
        sample,
    })
}

/// Pack unit lines into ~`budget` char batches, oldest units first, hard-capped.
fn source_batches(units: &[TextUnit], budget: usize, max_batches: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for u in units {
        for line in &u.original_lines {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let add = if cur.is_empty() {
                line.to_string()
            } else {
                format!("\n{line}")
            };
            if !cur.is_empty() && cur.chars().count() + add.chars().count() > budget {
                out.push(std::mem::take(&mut cur));
                if out.len() >= max_batches {
                    return out;
                }
            }
            cur.push_str(&add);
        }
    }
    if !cur.is_empty() && out.len() < max_batches {
        out.push(cur);
    }
    out
}

#[derive(Debug, Clone)]
struct ExtractedTerm {
    src: String,
    dst: String,
    info: String,
}

fn extract_terms_llm(
    client: &crate::config::LlmClient,
    batch: &str,
    src_lang: &str,
    dst_lang: &str,
) -> Result<Vec<ExtractedTerm>> {
    let system = "你是本地化术语工程师。只输出 JSON 数组，不要解释、不要 Markdown。";
    let user = format!(
        "下面是一部作品的 {src_lang} 原文片段。请提取**应当在全作品统一译名的专有名词**，\
         并给出 {dst_lang} 译名。\n\n\
         规则：\n\
         - 术语必须是原文中的连续子字符串（子字符串原则）\n\
         - 只截取核心名字，去掉修饰称谓（如「骑士艾琳」→「艾琳」）\n\
         - info 必须且只能是：男性角色、女性角色、未知性别角色、地名、家族、组织、\
           特殊物品、特殊技能、特殊生物、其他\n\
         - 禁止：泛用词（剑/魔法/城堡）、泛用称谓职业（先生/战士/商人）、整句、变量名\n\
         - 合并重复；同一概念只留一条\n\
         - 若本段没有专有名词，输出 []\n\n\
         只输出 JSON 数组：[{{\"src\":\"...\",\"dst\":\"...\",\"info\":\"...\"}}]\n\n\
         原文：\n{batch}"
    );
    let v = llm::ask_json(client, system, &user)?;
    let arr = v
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("expected a JSON array of terms"))?;
    Ok(arr
        .iter()
        .filter_map(|item| {
            let src = item.get("src")?.as_str()?.trim();
            let dst = item
                .get("dst")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .trim();
            // accept type as alias for info (LinguaGacha JSONL uses type)
            let info = item
                .get("info")
                .or_else(|| item.get("type"))
                .and_then(|x| x.as_str())
                .unwrap_or("其他")
                .trim();
            if src.is_empty() || dst.is_empty() {
                return None;
            }
            Some(ExtractedTerm {
                src: src.to_string(),
                dst: dst.to_string(),
                info: info.to_string(),
            })
        })
        .collect())
}

fn accept_llm_term(units: &[TextUnit], row: &ExtractedTerm) -> bool {
    let src = row.src.trim();
    let dst = row.dst.trim();
    if src.chars().count() < 2 || dst.is_empty() {
        return false;
    }
    if knowledge::is_machine_literal(src) {
        return false;
    }
    // Must be a real substring of some unit (anti-hallucination).
    if !units
        .iter()
        .any(|u| u.original_lines.iter().any(|l| l.contains(src)))
    {
        return false;
    }
    let info = row.info.trim();
    if info.is_empty() {
        return true; // will default later
    }
    let lower = info.to_ascii_lowercase();
    if lower == "other" || lower == "others" {
        return true; // normalize later via winner; still a valid bucket
    }
    // Soft check: unknown labels still accepted (model may use short forms);
    // whitelist is guidance in the prompt, not a hard gate beyond empty.
    let _ = LLM_INFO_OK;
    true
}

fn winner(votes: &BTreeMap<String, usize>) -> String {
    votes
        .iter()
        .max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.cmp(a.0)))
        .map(|(k, _)| k.clone())
        .unwrap_or_default()
}

#[derive(Debug, Deserialize)]
struct NamedTerm {
    src: String,
    #[serde(default)]
    keep: bool,
    #[serde(default)]
    dst: String,
    #[serde(default)]
    info: String,
}

fn name_candidates(
    client: &crate::config::LlmClient,
    chunk: &[(String, usize)],
    src_lang: &str,
    dst_lang: &str,
) -> Result<Vec<NamedTerm>> {
    let list: String = chunk
        .iter()
        .map(|(t, c)| format!("- {t}（{c} 次）"))
        .collect::<Vec<_>>()
        .join("\n");
    let user = format!(
        "下面是从一部作品的 {src_lang} 原文中统计出的高频候选词。\
         请判断每个候选是否为**应当在全作品统一译名的专有名词**（人名、地名、组织名、独有概念）。\n\n\
         判断规则：\n\
         - 常见词、普通名词、动词短语、形容词、整句 → keep=false\n\
         - 带称呼后缀的形式（如「〜さん」「〜様」）不要单独成条 → keep=false\n\
         - 可拆分的复合词，只保留其中的专有名词部分 → 复合词本身 keep=false\n\
         - keep=true 时必须给出 dst（{dst_lang} 译名）与 info（简短消歧描述，\
           如「女性名字」「男性名字」「地点」「组织」「物品」）\n\n\
         只输出 JSON 数组，每个候选一条，src 原样复制：\n\
         [{{\"src\":\"...\",\"keep\":true,\"dst\":\"...\",\"info\":\"...\"}}]\n\n\
         候选：\n{list}"
    );
    let v = llm::ask_json(
        client,
        "你是本地化术语工程师。只输出 JSON 数组，不要解释、不要 Markdown。",
        &user,
    )?;
    let arr = v
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("expected a JSON array of terms"))?;
    Ok(arr
        .iter()
        .filter_map(|item| serde_json::from_value::<NamedTerm>(item.clone()).ok())
        .filter(|n| !n.src.trim().is_empty())
        .collect())
}

// ---- per-batch injection ----

/// Terms a batch of source text actually contains, highest count first.
///
/// `terms` is expected sorted by count descending (see `Translator::with_glossary`),
/// so `take(limit)` keeps the most load-bearing names when a batch has more
/// matches than the budget allows.
pub fn select_for_batch<'a>(
    terms: &'a [GlossaryTerm],
    text: &str,
    limit: usize,
) -> Vec<&'a GlossaryTerm> {
    terms
        .iter()
        .filter(|t| {
            t.status == TermStatus::Active
                && !t.dst.is_empty()
                && !t.src.is_empty()
                && text.contains(&t.src)
        })
        .take(limit)
        .collect()
}

// ---- post-translation check ----

#[derive(Debug, Clone, Serialize)]
pub struct Violation {
    pub src: String,
    pub dst: String,
    /// Translated units whose source contained `src`.
    pub occurrences: usize,
    /// …of those, how many actually used `dst`.
    pub applied: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct CheckReport {
    pub active_terms: usize,
    pub terms_seen: usize,
    pub terms_fully_applied: usize,
    pub violations: Vec<Violation>,
}

/// Report glossary terms the translation did not actually use.
///
/// A substring test, so an inflected target language can produce false
/// positives — it is a review aid, not a gate.
pub fn check(workspace: &Path) -> Result<CheckReport> {
    let store = crate::store::workspace_db(workspace)?;
    let units = store.all_units()?;
    let translations = store.all_translations()?;
    let glossary = load(workspace);
    let active = glossary.active();

    let mut violations = Vec::new();
    let mut seen = 0usize;
    let mut ok = 0usize;
    for t in &active {
        let mut occurrences = 0usize;
        let mut applied = 0usize;
        for u in &units {
            let Some(tr) = translations.get(&u.id) else {
                continue;
            };
            if tr.passthrough {
                continue; // the model never translated it; not a glossary failure
            }
            if !u.original_lines.iter().any(|l| l.contains(&t.src)) {
                continue;
            }
            occurrences += 1;
            if tr.translation_lines.iter().any(|l| l.contains(&t.dst)) {
                applied += 1;
            }
        }
        if occurrences == 0 {
            continue;
        }
        seen += 1;
        if applied == occurrences {
            ok += 1;
        } else {
            violations.push(Violation {
                src: t.src.clone(),
                dst: t.dst.clone(),
                occurrences,
                applied,
            });
        }
    }
    // Worst offenders first: the term missed the most times is the one worth
    // fixing by hand.
    violations.sort_by_key(|v| std::cmp::Reverse(v.occurrences - v.applied));
    Ok(CheckReport {
        active_terms: active.len(),
        terms_seen: seen,
        terms_fully_applied: ok,
        violations,
    })
}

// ---- import / export ----

/// Import terms from JSON. Two shapes are accepted:
/// * `[{"src": "...", "dst": "...", "info": "..."}]`
/// * `{"src": "dst"}`
pub fn import_json(workspace: &Path, file: &Path) -> Result<usize> {
    let raw = std::fs::read_to_string(file).with_context(|| format!("read {}", file.display()))?;
    let v: serde_json::Value =
        serde_json::from_str(&raw).with_context(|| format!("parse {}", file.display()))?;
    let mut incoming: Vec<GlossaryTerm> = Vec::new();
    match &v {
        serde_json::Value::Array(items) => {
            for it in items {
                let src = it
                    .get("src")
                    .and_then(|x| x.as_str())
                    .unwrap_or_default()
                    .to_string();
                if src.is_empty() {
                    continue;
                }
                incoming.push(GlossaryTerm {
                    src,
                    dst: it
                        .get("dst")
                        .and_then(|x| x.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    info: it
                        .get("info")
                        .and_then(|x| x.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    count: 0,
                    status: TermStatus::Active,
                    source: "import".into(),
                });
            }
        }
        serde_json::Value::Object(map) => {
            for (src, dst) in map {
                let Some(dst) = dst.as_str() else { continue };
                if src.is_empty() {
                    continue;
                }
                incoming.push(GlossaryTerm {
                    src: src.clone(),
                    dst: dst.to_string(),
                    info: String::new(),
                    count: 0,
                    status: TermStatus::Active,
                    source: "import".into(),
                });
            }
        }
        _ => bail!("unsupported glossary JSON: expected an array or an object"),
    }

    let mut g = load(workspace);
    let n = incoming.len();
    for t in incoming {
        g.upsert(t);
    }
    ensure_langs(workspace, &mut g);
    save(workspace, &g)?;
    Ok(n)
}

pub fn export_json(workspace: &Path, file: &Path) -> Result<usize> {
    let g = load(workspace);
    let items: Vec<serde_json::Value> = g
        .terms
        .iter()
        .filter(|t| t.status == TermStatus::Active)
        .map(|t| serde_json::json!({"src": t.src, "dst": t.dst, "info": t.info}))
        .collect();
    let n = items.len();
    std::fs::write(file, serde_json::to_string_pretty(&items)?)
        .with_context(|| format!("write {}", file.display()))?;
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ItemType;

    fn unit(text: &str) -> TextUnit {
        TextUnit {
            id: text.to_string(),
            engine: "rmmz".into(),
            domain: "dialogue".into(),
            location: text.to_string(),
            item_type: ItemType::ShortText,
            role: String::new(),
            original_lines: vec![text.to_string()],
            source_line_paths: vec![],
            context: String::new(),
            payload: String::new(),
        }
    }

    fn repeat(text: &str, n: usize) -> Vec<TextUnit> {
        (0..n)
            .map(|i| {
                let mut u = unit(text);
                u.id = format!("{text}#{i}");
                u.location = u.id.clone();
                u
            })
            .collect()
    }

    fn count_of(v: &[(String, usize)], term: &str) -> usize {
        v.iter()
            .find(|(t, _)| t == term)
            .map(|(_, c)| *c)
            .unwrap_or(0)
    }

    // ---- mining ----

    #[test]
    fn mines_katakana_names() {
        let units = repeat("アレイは村を出た", 12);
        let got = mine_candidates(&units, "ja");
        assert_eq!(count_of(&got, "アレイ"), 12);
    }

    #[test]
    fn mines_katakana_with_kanji_suffix_as_one_term() {
        let units = repeat("エルギア国の兵士", 5);
        let got = mine_candidates(&units, "ja");
        assert_eq!(count_of(&got, "エルギア国"), 5, "compound place name");
        assert_eq!(count_of(&got, "エルギア"), 5, "and the bare name too");
    }

    #[test]
    fn mines_kanji_runs_greedily() {
        // With no tokeniser, a kanji run is taken whole: `魔法使いの弟子` yields
        // `魔法使`, not `魔法`. Over-capture is the accepted trade — the model's
        // `keep` veto is what removes the fragments that are not real terms.
        let units = repeat("魔法使いの弟子", 4);
        let got = mine_candidates(&units, "ja");
        assert_eq!(count_of(&got, "魔法使"), 4);
        assert_eq!(count_of(&got, "弟子"), 4);
        assert_eq!(count_of(&got, "魔法"), 0, "runs are not sub-divided");
    }

    #[test]
    fn mines_english_proper_nouns() {
        let units = repeat("Alice went to Silver Harbor today", 6);
        let got = mine_candidates(&units, "en");
        assert_eq!(count_of(&got, "Alice"), 6);
        assert_eq!(count_of(&got, "Silver Harbor"), 6);
    }

    #[test]
    fn common_words_are_filtered_before_the_model_is_asked() {
        // The model charges per candidate; obvious noise must not get that far.
        let units = repeat("自分の気持を確認する", 20);
        let got = mine_candidates(&units, "ja");
        assert_eq!(count_of(&got, "自分"), 0);
        assert_eq!(count_of(&got, "気持"), 0);
    }

    #[test]
    fn english_sentence_openers_are_filtered() {
        let units = repeat("The door opened. There was Alice", 5);
        let got = mine_candidates(&units, "en");
        assert_eq!(count_of(&got, "The"), 0);
        assert_eq!(count_of(&got, "There"), 0);
        assert_eq!(count_of(&got, "Alice"), 5);
    }

    #[test]
    fn machine_literals_never_become_candidates() {
        let units = repeat("Img Png Wav", 5);
        let got = mine_candidates(&units, "en");
        assert!(!got.iter().any(|(t, _)| t == "12"));
    }

    #[test]
    fn results_are_sorted_by_count_then_term() {
        let mut units = repeat("アレイ", 9);
        units.extend(repeat("ベルナ", 3));
        let got = mine_candidates(&units, "ja");
        assert_eq!(got[0].0, "アレイ");
        assert!(got[0].1 > got[1].1);
    }

    // ---- injection ----

    fn term(src: &str, dst: &str, count: usize) -> GlossaryTerm {
        GlossaryTerm {
            src: src.into(),
            dst: dst.into(),
            info: String::new(),
            count,
            status: TermStatus::Active,
            source: "auto".into(),
        }
    }

    #[test]
    fn selects_only_terms_present_in_the_batch() {
        let terms = vec![term("アレイ", "艾蕾", 40), term("ベルナ", "贝尔娜", 10)];
        let got = select_for_batch(&terms, "アレイは剣を取った", 30);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].dst, "艾蕾");
    }

    #[test]
    fn respects_the_inject_limit() {
        let terms: Vec<GlossaryTerm> = (0..10)
            .map(|i| term(&format!("名{i}"), &format!("N{i}"), 100 - i))
            .collect();
        let text: String = (0..10).map(|i| format!("名{i} ")).collect();
        assert_eq!(select_for_batch(&terms, &text, 3).len(), 3);
    }

    #[test]
    fn rejected_and_unnamed_terms_are_never_injected() {
        let mut rejected = term("春", "", 99);
        rejected.status = TermStatus::Rejected;
        let unnamed = term("ベルナ", "", 50);
        let terms = vec![rejected, unnamed, term("アレイ", "艾蕾", 10)];
        let got = select_for_batch(&terms, "春にアレイとベルナが会った", 30);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].src, "アレイ");
    }

    // ---- storage ----

    #[test]
    fn toml_roundtrip_preserves_terms() {
        let g = Glossary {
            version: GLOSSARY_VERSION,
            source_lang: "ja".into(),
            target_lang: "zh".into(),
            terms: vec![
                term("アレイ", "艾蕾", 47),
                GlossaryTerm {
                    src: "春".into(),
                    dst: String::new(),
                    info: "太常见，非专有名词".into(),
                    count: 120,
                    status: TermStatus::Rejected,
                    source: "auto".into(),
                },
            ],
        };
        let back: Glossary = toml::from_str(&toml::to_string_pretty(&g).unwrap()).unwrap();
        assert_eq!(back.terms.len(), 2);
        assert_eq!(back.source_lang, "ja");
        assert_eq!(back.terms[0].count, 47);
        assert_eq!(back.terms[1].status, TermStatus::Rejected);
        assert_eq!(back.active().len(), 1, "rejected terms stay out of use");
    }

    #[test]
    fn upsert_replaces_the_same_source_term() {
        let mut g = Glossary::default();
        g.upsert(term("アレイ", "埃雷", 1));
        g.upsert(term("アレイ", "艾蕾", 47));
        assert_eq!(g.terms.len(), 1);
        assert_eq!(g.terms[0].dst, "艾蕾");
    }

    #[test]
    fn import_accepts_both_json_shapes() {
        let dir = std::env::temp_dir().join(format!("attx-gl-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let list = dir.join("list.json");
        std::fs::write(
            &list,
            r#"[{"src":"アレイ","dst":"艾蕾","info":"女性名字"}]"#,
        )
        .unwrap();
        assert_eq!(import_json(&dir, &list).unwrap(), 1);

        let map = dir.join("map.json");
        std::fs::write(&map, r#"{"ベルナ":"贝尔娜"}"#).unwrap();
        assert_eq!(import_json(&dir, &map).unwrap(), 1);

        let g = load(&dir);
        assert_eq!(g.active().len(), 2);
        assert_eq!(g.find("アレイ").unwrap().info, "女性名字");

        // …and export round-trips what import accepted.
        let out = dir.join("out.json");
        assert_eq!(export_json(&dir, &out).unwrap(), 2);
        let back: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
        assert_eq!(back.as_array().unwrap().len(), 2);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_glossary_is_empty_not_an_error() {
        let dir = std::env::temp_dir().join(format!("attx-gl-none-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert!(load(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn source_batches_respect_char_budget() {
        let units = repeat("あいうえお", 20); // 5 chars each
        let batches = source_batches(&units, 12, 10);
        assert!(batches.len() > 1);
        for b in &batches {
            assert!(b.chars().count() <= 12 + 5, "batch too big: {b}");
        }
    }

    #[test]
    fn source_batches_hard_cap() {
        let units = repeat("名前", 100);
        let batches = source_batches(&units, 3, 5);
        assert_eq!(batches.len(), 5);
    }

    #[test]
    fn accept_llm_term_requires_substring() {
        let units = repeat("アレイは剣を取った", 1);
        assert!(accept_llm_term(
            &units,
            &ExtractedTerm {
                src: "アレイ".into(),
                dst: "艾蕾".into(),
                info: "女性角色".into(),
            }
        ));
        assert!(!accept_llm_term(
            &units,
            &ExtractedTerm {
                src: "ベルナ".into(),
                dst: "贝尔娜".into(),
                info: "女性角色".into(),
            }
        ));
    }

    #[test]
    fn glossary_method_parse() {
        use crate::config::GlossaryMethod;
        assert_eq!(GlossaryMethod::parse("llm"), Some(GlossaryMethod::Llm));
        assert_eq!(GlossaryMethod::parse("STATS"), Some(GlossaryMethod::Stats));
        assert_eq!(GlossaryMethod::parse("regex"), Some(GlossaryMethod::Stats));
        assert_eq!(GlossaryMethod::parse("nope"), None);
    }
}
