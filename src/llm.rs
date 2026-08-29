use crate::config::{LlmClient, TranslationSection};
use crate::glossary::GlossaryTerm;
use crate::model::{ItemType, TextUnit, Translation, unmask_controls};
use crate::preserve::{self, PreserveSet};
use crate::quality;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Serialize, Deserialize, Clone)]
struct ChatMessage {
    role: String,
    #[serde(default)]
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: ChatMessage,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ModelItem {
    #[serde(deserialize_with = "deserialize_id")]
    id: String,
    translation_lines: Vec<String>,
}

fn deserialize_id<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{self, Visitor};
    use std::fmt;
    struct IdVisitor;
    impl<'de> Visitor<'de> for IdVisitor {
        type Value = String;
        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("string or integer id")
        }
        fn visit_str<E: de::Error>(self, v: &str) -> Result<String, E> {
            Ok(v.to_string())
        }
        fn visit_string<E: de::Error>(self, v: String) -> Result<String, E> {
            Ok(v)
        }
        fn visit_i64<E: de::Error>(self, v: i64) -> Result<String, E> {
            Ok(v.to_string())
        }
        fn visit_u64<E: de::Error>(self, v: u64) -> Result<String, E> {
            Ok(v.to_string())
        }
    }
    deserializer.deserialize_any(IdVisitor)
}

/// Prompt flavor derived from the format adapter — dialogue, prose, subtitles,
/// documents, and UI strings need different registers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    Game,
    Literary,
    Subtitle,
    Document,
    Software,
}

pub fn profile_for_format(format_id: &str) -> Profile {
    match format_id {
        "epub" | "txt" => Profile::Literary,
        "srt" | "vtt" | "lrc" => Profile::Subtitle,
        "docx" | "md" => Profile::Document,
        "po" | "i18next" | "paratranz" => Profile::Software,
        // rmmz, jsonl, mtool, vnt, renpy and unknown formats
        _ => Profile::Game,
    }
}

fn lang_display(code: &str) -> &str {
    match code.to_ascii_lowercase().as_str() {
        "zh" | "zh-cn" | "zh-hans" => "简体中文 (Simplified Chinese)",
        "zh-tw" | "zh-hant" => "繁體中文 (Traditional Chinese)",
        "en" => "English",
        "ja" => "日本語 (Japanese)",
        "ko" => "한국어 (Korean)",
        _ => code,
    }
}

fn profile_line_zh(profile: Profile) -> &'static str {
    match profile {
        Profile::Game => "这是游戏文本：对白自然口语化，系统/UI 文本简洁准确。",
        Profile::Literary => {
            "这是小说/文学文本：译文流畅自然，符合目标语言叙事习惯，保留人名、敬称与语气差异，不添加译注。"
        }
        Profile::Subtitle => "这是字幕/歌词：口语化、简短，适合单行显示，不添加注释。",
        Profile::Document => "这是文档文本：语义准确，术语一致，保持原文格式结构。",
        Profile::Software => "这是软件界面/本地化文本：简洁、术语一致，符合软件界面惯例。",
    }
}

fn profile_line_en(profile: Profile) -> &'static str {
    match profile {
        Profile::Game => "This is game text: natural dialogue, concise UI/system strings.",
        Profile::Literary => {
            "This is literary prose: fluent and natural in the target language; keep names, honorifics and tone; no translator notes."
        }
        Profile::Subtitle => {
            "These are subtitles/lyrics: colloquial and short, fit for one-line display."
        }
        Profile::Document => {
            "This is document text: accurate, consistent terminology, keep formatting."
        }
        Profile::Software => "These are software UI strings: concise, consistent terminology.",
    }
}

/// Build the system prompt. Chinese instructions for zh targets (best model
/// adherence), English otherwise.
fn system_prompt(source_lang: &str, target_lang: &str, profile: Profile) -> String {
    let src = lang_display(source_lang);
    let dst = lang_display(target_lang);
    if target_lang.to_ascii_lowercase().starts_with("zh") {
        format!(
            r#"你是专业本地化译者，将 {src} 翻译为 {dst}。
{profile_line}
规则：
- 忠实保留原意、语气和内容尺度，不净化、不扩写、不遗漏。
- 形如 [CTRL_n] 的标记必须原样保留，数量与相对位置一致，不翻译。
- 文中的占位符与标记（如 {{tag}}、[var]、<tag>、%s、%d、\n）原样保留，不翻译其内部。
- 姓名栏（role/namebox）与正文对同一实体必须使用同一译名。
- 译文中不得残留源语假名或韩文；专有名词按术语表翻译，不要夹注原文。
- 条目前的 prev/next 邻句只供消歧，不要输出它们的译文。
- long_text 可按目标语言语感调整断句；array 必须输出 line_count 行；short_text 的 translation_lines 只有 1 个字符串。
- 顶层输出严格 JSON 数组，不要 Markdown、解释或额外文本。
- 每个元素：{{"id":"<id>","role":"<角色>","translation_lines":["..."]}}
- id 与 role 原样复制输入；没有角色时 role 为空字符串。
"#,
            profile_line = profile_line_zh(profile),
        )
    } else {
        format!(
            r#"You are a professional localizer. Translate {src} into {dst}.
{profile_line}
Rules:
- Keep meaning, tone, and content rating. Do not censor, expand, or omit.
- Tokens like [CTRL_n] must be kept verbatim, same count and relative position.
- Placeholders and markup ({{tag}}, [var], <tag>, %s, %d, \n) stay verbatim; never translate inside them.
- A namebox/role label and body text for the same entity must share one translation.
- Do not leave source-script kana or hangul in the output; use the glossary for names.
- prev/next neighbor lines are context only; do not translate them.
- long_text may reflow lines; array must return exactly line_count lines; short_text has exactly 1 string in translation_lines.
- Output a strict JSON array only. No markdown.
- Each element: {{"id":"<id>","role":"<role>","translation_lines":["..."]}}
- Copy id and role from input.
"#,
            profile_line = profile_line_en(profile),
        )
    }
}

/// Global request pacing shared by all worker threads: each request reserves
/// the next `min_interval` slot. `rpm = 0` disables pacing.
struct RateLimiter {
    min_interval: Duration,
    next_at: Mutex<Instant>,
}

impl RateLimiter {
    fn new(rpm: u32) -> Option<Self> {
        if rpm == 0 {
            return None;
        }
        Some(Self {
            min_interval: Duration::from_secs_f64(60.0 / rpm as f64),
            next_at: Mutex::new(Instant::now()),
        })
    }

    fn wait(&self) {
        let slot = {
            let mut next = self.next_at.lock().expect("rate limiter lock");
            let slot = (*next).max(Instant::now());
            *next = slot + self.min_interval;
            slot
        };
        let now = Instant::now();
        if slot > now {
            thread::sleep(slot - now);
        }
    }
}
#[derive(Clone)]
struct Neighbor {
    id: String,
    role: String,
    original: String,
    translation: Option<String>,
}

pub struct Translator {
    client: LlmClient,
    section: TranslationSection,
    http: reqwest::blocking::Client,
    system: String,
    rate: Option<RateLimiter>,
    /// Active glossary, highest count first. Only the terms a batch actually
    /// contains are injected into it — see `translate_batch`.
    glossary: Vec<GlossaryTerm>,
    inject_limit: usize,
    preserve: PreserveSet,
    neighbors: BTreeMap<String, (Option<Neighbor>, Option<Neighbor>)>,
}

impl Translator {
    pub fn new(
        client: &LlmClient,
        section: &TranslationSection,
        source_lang: &str,
        target_lang: &str,
        profile: Profile,
    ) -> Result<Self> {
        let http = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(client.timeout.max(30)))
            .build()?;
        Ok(Self {
            client: client.clone(),
            section: section.clone(),
            http,
            system: system_prompt(source_lang, target_lang, profile),
            rate: RateLimiter::new(section.rpm),
            glossary: Vec::new(),
            inject_limit: 0,
            preserve: PreserveSet::core().clone(),
            neighbors: BTreeMap::new(),
        })
    }

    /// Append learned `topic = "prompt"` notes to the system prompt.
    pub fn with_notes(mut self, notes: &[String]) -> Self {
        let useful: Vec<&String> = notes.iter().filter(|n| !n.trim().is_empty()).collect();
        if useful.is_empty() {
            return self;
        }
        self.system.push_str("\n# 该格式的既往经验\n");
        for n in useful {
            self.system.push_str(&format!("- {n}\n"));
        }
        self
    }

    /// Attach a glossary. Terms are selected per batch, never dumped wholesale:
    /// a few hundred entries would crowd out the text in every request.
    pub fn with_glossary(mut self, mut terms: Vec<GlossaryTerm>, inject_limit: usize) -> Self {
        if terms.is_empty() || inject_limit == 0 {
            return self;
        }
        terms.sort_by_key(|t| std::cmp::Reverse(t.count));
        self.glossary = terms;
        self.inject_limit = inject_limit;
        self.system
            .push_str("- 正文前若给出「术语表」，其中的译名必须严格采用，不得自行改译。\n");
        self
    }

    pub fn with_preserve(mut self, set: PreserveSet) -> Self {
        self.preserve = set;
        self
    }

    pub fn with_neighbors(
        mut self,
        units: &[TextUnit],
        translations: &BTreeMap<String, Translation>,
    ) -> Self {
        self.neighbors = build_neighbor_map(units, translations);
        self
    }

    pub fn translate_units(
        &self,
        units: &[TextUnit],
        limit: Option<usize>,
    ) -> Result<Vec<Translation>> {
        self.translate_units_with_sink(units, limit, &mut |_batch| Ok(()))
    }

    /// Translate units with up to `worker_count` HTTP batches in parallel.
    /// `on_batch` runs on the *calling* thread as each batch completes, so a
    /// crash mid-run keeps everything already saved (Store is !Sync — workers
    /// hand results over an mpsc channel instead of touching SQLite).
    pub fn translate_units_with_sink<F>(
        &self,
        units: &[TextUnit],
        limit: Option<usize>,
        on_batch: &mut F,
    ) -> Result<Vec<Translation>>
    where
        F: FnMut(&[Translation]) -> Result<()>,
    {
        let slice: Vec<&TextUnit> = units.iter().take(limit.unwrap_or(usize::MAX)).collect();
        if slice.is_empty() {
            return Ok(vec![]);
        }
        let batches = batch_units(
            &slice,
            self.section.batch_chars,
            self.section.max_context_items,
        );
        let total_batches = batches.len();
        let total_units = slice.len();
        let workers = self.section.worker_count.max(1);

        eprintln!(
            "translate: {} units in {} batches, workers={}",
            total_units, total_batches, workers
        );

        // Shared queue of batch indices
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::mpsc;

        let next = Arc::new(AtomicUsize::new(0));
        let batches = Arc::new(
            batches
                .into_iter()
                .map(|b| b.into_iter().cloned().collect::<Vec<TextUnit>>())
                .collect::<Vec<_>>(),
        );
        let done_units = Arc::new(AtomicUsize::new(0));
        let (tx, rx) = mpsc::channel::<Vec<Translation>>();

        let mut out = Vec::new();
        let mut skipped = 0usize;
        let mut sink_err: Option<anyhow::Error> = None;

        std::thread::scope(|scope| {
            for _ in 0..workers {
                let next = Arc::clone(&next);
                let batches = Arc::clone(&batches);
                let done_units = Arc::clone(&done_units);
                let tx = tx.clone();
                let translator = self;
                scope.spawn(move || {
                    loop {
                        let bi = next.fetch_add(1, Ordering::Relaxed);
                        if bi >= batches.len() {
                            break;
                        }
                        let batch_owned = &batches[bi];
                        let refs: Vec<&TextUnit> = batch_owned.iter().collect();
                        eprintln!(
                            "batch {}/{} ({} units, done_units≈{})",
                            bi + 1,
                            total_batches,
                            refs.len(),
                            done_units.load(Ordering::Relaxed)
                        );
                        let mut attempt = 0u32;
                        let items = loop {
                            attempt += 1;
                            match translator.translate_batch_resilient(&refs) {
                                Ok(items) => break items,
                                Err(e) if attempt <= translator.section.retry_count => {
                                    eprintln!("  retry batch {} #{attempt}: {e:#}", bi + 1);
                                    thread::sleep(Duration::from_secs(
                                        translator.section.retry_delay,
                                    ));
                                }
                                Err(e) => {
                                    eprintln!("  SKIP batch {} after retries: {e:#}", bi + 1);
                                    break Vec::new();
                                }
                            }
                        };
                        done_units.fetch_add(items.len(), Ordering::Relaxed);
                        if tx.send(items).is_err() {
                            break; // receiver gone (sink error) — stop early
                        }
                    }
                });
            }
            drop(tx); // workers hold clones; rx ends when they finish

            // Drain on this thread: incremental save as batches arrive.
            for items in rx {
                if items.is_empty() {
                    skipped += 1;
                    continue;
                }
                match on_batch(&items) {
                    Ok(()) => out.extend(items),
                    Err(e) => {
                        sink_err = Some(e);
                        break; // dropping rx makes workers bail on next send
                    }
                }
            }
        });

        if let Some(e) = sink_err {
            return Err(e.context("saving translations"));
        }
        if skipped > 0 {
            eprintln!("finished with {skipped} skipped batch(es); re-run for remaining pending");
        }
        Ok(out)
    }

    /// Try full batch; on hard fail split to singles; single fail → passthrough original.
    fn translate_batch_resilient(&self, batch: &[&TextUnit]) -> Result<Vec<Translation>> {
        match self.translate_batch(batch) {
            Ok(v) if !v.is_empty() => {
                // fill missing with passthrough so pending shrinks
                if v.len() == batch.len() {
                    return Ok(v);
                }
                let got: BTreeMap<_, _> =
                    v.iter().map(|t| (t.unit_id.clone(), t.clone())).collect();
                let mut out = v;
                for u in batch {
                    if !got.contains_key(&u.id) {
                        // try single
                        match self.translate_batch(&[u]) {
                            Ok(mut one) if !one.is_empty() => out.append(&mut one),
                            _ => out.push(passthrough(u)),
                        }
                    }
                }
                Ok(out)
            }
            Ok(_) | Err(_) if batch.len() > 1 => {
                // split in half then recurse / singles
                let mid = batch.len() / 2;
                let mut out = Vec::new();
                for half in [&batch[..mid], &batch[mid..]] {
                    if half.is_empty() {
                        continue;
                    }
                    match self.translate_batch_resilient(half) {
                        Ok(mut v) => out.append(&mut v),
                        Err(_) => {
                            for u in half {
                                match self.translate_batch(&[*u]) {
                                    Ok(mut one) if !one.is_empty() => out.append(&mut one),
                                    _ => out.push(passthrough(u)),
                                }
                            }
                        }
                    }
                }
                Ok(out)
            }
            Ok(_) | Err(_) => {
                // single unit: try once more then passthrough
                if let Ok(v) = self.translate_batch(batch)
                    && !v.is_empty()
                {
                    return Ok(v);
                }
                Ok(batch.iter().map(|u| passthrough(u)).collect())
            }
        }
    }

    fn translate_batch(&self, batch: &[&TextUnit]) -> Result<Vec<Translation>> {
        let mut masks: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
        let mut id_map: BTreeMap<String, &TextUnit> = BTreeMap::new();
        let batch_ids: BTreeSet<&str> = batch.iter().map(|u| u.id.as_str()).collect();

        let mut body = String::from("# 场景\n\n");
        if let Some(c) = batch.first().map(|u| u.context.as_str())
            && !c.is_empty()
        {
            body.push_str(&format!("context: {c}\n"));
        }

        if !self.glossary.is_empty() {
            let source: String = batch
                .iter()
                .flat_map(|u| u.original_lines.iter())
                .cloned()
                .collect::<Vec<_>>()
                .join("\n");
            let hits =
                crate::glossary::select_for_batch(&self.glossary, &source, self.inject_limit);
            if !hits.is_empty() {
                body.push_str("\n# 术语表（必须严格遵守）\n\n");
                for t in hits {
                    if t.info.is_empty() {
                        body.push_str(&format!("{} → {}\n", t.src, t.dst));
                    } else {
                        body.push_str(&format!("{} → {}（{}）\n", t.src, t.dst, t.info));
                    }
                }
            }
        }

        body.push_str("\n# 正文\n\n");

        for (i, u) in batch.iter().enumerate() {
            let pid = (i + 1).to_string();
            id_map.insert(pid.clone(), *u);
            let (masked_lines, unit_map) = self.preserve.mask_unit_lines(&u.original_lines);
            masks.insert(u.id.clone(), unit_map);

            body.push_str(&format!("## {pid}\n"));
            body.push_str(&format!("id: {pid}\n"));
            body.push_str(&format!("type: {}\n", u.item_type.as_str()));
            body.push_str(&format!("role: {}\n", u.role));
            if u.item_type == ItemType::Array {
                body.push_str(&format!("line_count: {}\n", u.original_lines.len()));
            }
            if let Some((prev, next)) = self.neighbors.get(&u.id) {
                if let Some(p) = prev.as_ref().filter(|n| !batch_ids.contains(n.id.as_str())) {
                    body.push_str(&format!("prev: {}\n", format_neighbor(p)));
                }
                if let Some(n) = next.as_ref().filter(|n| !batch_ids.contains(n.id.as_str())) {
                    body.push_str(&format!("next: {}\n", format_neighbor(n)));
                }
            }
            body.push('\n');
            for line in &masked_lines {
                body.push_str(line);
                body.push('\n');
            }
            body.push('\n');
        }

        let raw = self.chat(self.system.as_str(), &body)?;
        let items = parse_model_json(&raw)?;
        let mut translations = Vec::new();
        for item in items {
            let Some(unit) = id_map.get(&item.id) else {
                eprintln!("  drop item with unknown id {:?}", item.id);
                continue;
            };
            let map = masks.get(&unit.id).cloned().unwrap_or_default();
            let mut lines: Vec<String> = item
                .translation_lines
                .into_iter()
                .map(|l| unmask_controls(&l, &map))
                .collect();
            lines = quality::sanitize_lines(unit, lines);
            if let Err(e) = quality::check_unit(unit, &lines) {
                eprintln!("  drop unit {}: {e}", unit.location);
                continue;
            }
            let lost = preserve::lost_token_count(&lines, &map);
            if !map.is_empty() && lost * 2 >= map.len() {
                eprintln!(
                    "  drop unit {}: preserved tokens lost {lost}/{}",
                    unit.location,
                    map.len()
                );
                continue;
            }
            translations.push(Translation {
                unit_id: unit.id.clone(),
                translation_lines: lines,
                source_hash: TextUnit::source_hash(&unit.original_lines),
                passthrough: false,
            });
        }
        if translations.is_empty() {
            bail!("batch produced 0 acceptable translations (model/quality)");
        }
        if translations.len() != batch.len() {
            eprintln!(
                "  partial batch: kept {}/{}",
                translations.len(),
                batch.len()
            );
        }
        Ok(translations)
    }

    fn chat(&self, system: &str, user: &str) -> Result<String> {
        let url = format!(
            "{}/chat/completions",
            self.client.base_url.trim_end_matches('/')
        );
        let req = chat_body(&self.client, system, user, 0.3);
        if let Some(rate) = &self.rate {
            rate.wait();
        }
        let resp = self
            .http
            .post(&url)
            .bearer_auth(&self.client.api_key)
            .json(&req)
            .send()
            .with_context(|| format!("POST {url}"))?;
        read_chat_text(resp, wants_stream(&req))
    }

    pub fn ping(&self) -> Result<String> {
        self.chat("Reply with exactly: pong", "ping")
    }
}

fn batch_units<'a>(
    units: &[&'a TextUnit],
    batch_chars: usize,
    max_items: usize,
) -> Vec<Vec<&'a TextUnit>> {
    let max_items = max_items.max(1);
    let mut batches = Vec::new();
    let mut cur: Vec<&TextUnit> = Vec::new();
    let mut chars = 0usize;
    for u in units {
        let size: usize = u.original_lines.iter().map(|l| l.chars().count()).sum();
        let would_exceed = chars + size > batch_chars && !cur.is_empty();
        let too_many = cur.len() >= max_items;
        if would_exceed || too_many {
            batches.push(std::mem::take(&mut cur));
            chars = 0;
        }
        chars += size;
        cur.push(*u);
    }
    if !cur.is_empty() {
        batches.push(cur);
    }
    batches
}

fn passthrough(u: &TextUnit) -> Translation {
    // Keep the original when the model refuses (policy/empty) so writeback and
    // progress still work; flagged so status reports it and
    // `translate --retry-passthrough` can re-queue.
    eprintln!("  passthrough {}", u.location);
    Translation {
        unit_id: u.id.clone(),
        translation_lines: u.original_lines.clone(),
        source_hash: TextUnit::source_hash(&u.original_lines),
        passthrough: true,
    }
}

fn parse_model_json(raw: &str) -> Result<Vec<ModelItem>> {
    let trimmed = raw.trim();
    // strip ```json fences if model misbehaves
    let body = if trimmed.starts_with("```") {
        let mut lines = trimmed.lines();
        let _ = lines.next();
        let collected: Vec<&str> = lines.collect();
        let s = collected.join("\n");
        s.trim_end_matches('`').trim().to_string()
    } else {
        trimmed.to_string()
    };
    // find array bounds
    let start = body.find('[').unwrap_or(0);
    let end = body.rfind(']').map(|i| i + 1).unwrap_or(body.len());
    let slice = &body[start..end];
    serde_json::from_str(slice)
        .with_context(|| format!("parse model json: {}", truncate(slice, 400)))
}

fn truncate(s: &str, n: usize) -> String {
    let mut t: String = s.chars().take(n).collect();
    if s.chars().count() > n {
        t.push('…');
    }
    t
}

fn snap_neighbor(u: &TextUnit, tr: &BTreeMap<String, Translation>) -> Neighbor {
    Neighbor {
        id: u.id.clone(),
        role: u.role.clone(),
        original: u.joined_text(),
        translation: tr
            .get(&u.id)
            .filter(|t| !t.passthrough)
            .map(|t| t.translation_lines.join("\n")),
    }
}

fn build_neighbor_map(
    units: &[TextUnit],
    tr: &BTreeMap<String, Translation>,
) -> BTreeMap<String, (Option<Neighbor>, Option<Neighbor>)> {
    let mut groups: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (i, u) in units.iter().enumerate() {
        groups.entry(u.context.as_str()).or_default().push(i);
    }
    let mut out = BTreeMap::new();
    for idxs in groups.values() {
        for (k, &i) in idxs.iter().enumerate() {
            let prev = k.checked_sub(1).map(|j| snap_neighbor(&units[idxs[j]], tr));
            let next = idxs.get(k + 1).map(|&j| snap_neighbor(&units[j], tr));
            out.insert(units[i].id.clone(), (prev, next));
        }
    }
    out
}

fn format_neighbor(n: &Neighbor) -> String {
    let orig = truncate(&n.original.replace('\n', " / "), 120);
    let role = if n.role.is_empty() {
        String::new()
    } else {
        format!("{} ", n.role)
    };
    match &n.translation {
        Some(t) => format!("{role}「{orig}」→「{}」", truncate(t, 80)),
        None => format!("{role}「{orig}」"),
    }
}

/// `temperature` is the call-site default (translate 0.3, ask_json 0.0).
/// Client named fields overlay it; `extra` merges last. `messages` in extra
/// is ignored. `stream` is sent only when true.
fn chat_body(client: &LlmClient, system: &str, user: &str, temperature: f64) -> serde_json::Value {
    let mut body = serde_json::json!({
        "model": client.model,
        "temperature": client.temperature.unwrap_or(temperature),
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user}
        ]
    });
    if let Some(effort) = client
        .reasoning_effort
        .as_deref()
        .filter(|s| !s.is_empty())
    {
        body["reasoning_effort"] = serde_json::Value::String(effort.to_string());
    }
    if let Some(max_tokens) = client.max_tokens {
        body["max_tokens"] = serde_json::json!(max_tokens);
    }
    if client.stream {
        body["stream"] = serde_json::json!(true);
    }
    apply_extra(&mut body, &client.extra);
    if body.get("stream").and_then(|v| v.as_bool()) != Some(true) {
        if let Some(obj) = body.as_object_mut() {
            obj.remove("stream");
        }
    }
    body
}

fn apply_extra(body: &mut serde_json::Value, extra: &toml::Table) {
    if extra.is_empty() {
        return;
    }
    let Ok(serde_json::Value::Object(map)) = serde_json::to_value(extra) else {
        return;
    };
    let Some(obj) = body.as_object_mut() else {
        return;
    };
    for (k, v) in map {
        if k == "messages" {
            continue;
        }
        obj.insert(k, v);
    }
}

fn wants_stream(body: &serde_json::Value) -> bool {
    body.get("stream").and_then(|v| v.as_bool()).unwrap_or(false)
}

fn read_chat_text(resp: reqwest::blocking::Response, stream: bool) -> Result<String> {
    let status = resp.status();
    let text = resp.text()?;
    if !status.is_success() {
        bail!("LLM HTTP {status}: {}", truncate(&text, 500));
    }
    decode_chat_content(&text, stream)
}

fn decode_chat_content(text: &str, stream: bool) -> Result<String> {
    // Some gateways ignore stream=true and still return a JSON object.
    if stream && !text.trim_start().starts_with('{') {
        content_from_sse(text)
    } else {
        content_from_json(text)
    }
}

fn content_from_json(text: &str) -> Result<String> {
    let parsed: ChatResponse = serde_json::from_str(text)
        .with_context(|| format!("decode chat: {}", truncate(text, 300)))?;
    let choice = parsed
        .choices
        .first()
        .ok_or_else(|| anyhow::anyhow!("empty choices"))?;
    choice
        .message
        .content
        .clone()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "empty content finish={:?}",
                choice.finish_reason.as_deref().unwrap_or("?")
            )
        })
}

fn content_from_sse(text: &str) -> Result<String> {
    let mut out = String::new();
    for line in text.lines() {
        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };
        let data = data.trim();
        if data.is_empty() || data == "[DONE]" {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(data) else {
            continue;
        };
        if let Some(s) = v["choices"][0]["delta"]["content"].as_str() {
            out.push_str(s);
        }
    }
    if out.is_empty() {
        bail!("empty SSE content: {}", truncate(text, 300));
    }
    Ok(out)
}


/// One-shot JSON request outside the translation pipeline (learning, glossary).
///
/// Kept here rather than duplicated per caller so there is one place that knows
/// how to coax JSON out of a chat endpoint: models wrap it in prose or fences,
/// so the first `{`/`[` to the last `}`/`]` is extracted before parsing.
pub fn ask_json(client: &LlmClient, system: &str, user: &str) -> Result<serde_json::Value> {
    let http = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(client.timeout.max(30)))
        .build()?;
    let url = format!("{}/chat/completions", client.base_url.trim_end_matches('/'));
    let body = chat_body(client, system, user, 0.0);
    let resp = http
        .post(&url)
        .bearer_auth(&client.api_key)
        .json(&body)
        .send()
        .with_context(|| format!("POST {url}"))?;
    let content = read_chat_text(resp, wants_stream(&body))?;
    let slice = extract_json_span(&content)
        .ok_or_else(|| anyhow::anyhow!("no JSON in response: {}", truncate(&content, 200)))?;
    serde_json::from_str(slice).with_context(|| format!("parse json: {}", truncate(slice, 300)))
}

/// The widest `{…}` or `[…]` span in `s`, whichever starts first.
fn extract_json_span(s: &str) -> Option<&str> {
    let obj = s.find('{');
    let arr = s.find('[');
    let (start, close) = match (obj, arr) {
        (Some(o), Some(a)) if a < o => (a, ']'),
        (Some(o), _) => (o, '}'),
        (None, Some(a)) => (a, ']'),
        (None, None) => return None,
    };
    let end = s.rfind(close)? + close.len_utf8();
    if end <= start {
        return None;
    }
    Some(&s[start..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_span_survives_prose_and_fences() {
        assert_eq!(
            extract_json_span("Sure!\n```json\n[{\"a\":1}]\n```\n"),
            Some("[{\"a\":1}]")
        );
        assert_eq!(
            extract_json_span("here you go: {\"ok\": true} — done"),
            Some("{\"ok\": true}")
        );
        assert_eq!(extract_json_span("no json here"), None);
    }

    #[test]
    fn json_span_prefers_whichever_bracket_opens_first() {
        // An object holding an array must not be truncated at the array's `]`.
        assert_eq!(extract_json_span("{\"xs\":[1,2]}"), Some("{\"xs\":[1,2]}"));
    }

    fn client() -> LlmClient {
        LlmClient {
            name: "t".into(),
            provider_type: "openai".into(),
            base_url: "http://x".into(),
            api_key: "k".into(),
            model: "m".into(),
            timeout: 30,
            temperature: None,
            reasoning_effort: None,
            max_tokens: None,
            stream: false,
            extra: toml::Table::new(),
        }
    }

    #[test]
    fn chat_body_omits_optional_keys_by_default() {
        let v = chat_body(&client(), "s", "u", 0.3);
        assert_eq!(v["model"], "m");
        assert_eq!(v["temperature"], 0.3);
        assert!(v.get("reasoning_effort").is_none());
        assert!(v.get("max_tokens").is_none());
        assert!(v.get("stream").is_none());
        assert_eq!(v["messages"][0]["content"], "s");
    }

    #[test]
    fn named_fields_then_extra_overrides_and_adds() {
        let mut c = client();
        c.temperature = Some(0.1);
        c.reasoning_effort = Some("low".into());
        c.max_tokens = Some(100);
        c.extra = toml::from_str(
            r#"
temperature = 0.9
top_p = 0.5
stream = true
messages = []
"#,
        )
        .unwrap();
        let v = chat_body(&c, "s", "u", 0.3);
        assert_eq!(v["temperature"], 0.9);
        assert_eq!(v["reasoning_effort"], "low");
        assert_eq!(v["max_tokens"], 100);
        assert_eq!(v["top_p"], 0.5);
        assert_eq!(v["stream"], true);
        assert_eq!(v["messages"][0]["role"], "system");
        assert_eq!(v["messages"].as_array().map(|a| a.len()), Some(2));
    }

    #[test]
    fn empty_reasoning_effort_is_not_sent() {
        let mut c = client();
        c.reasoning_effort = Some(String::new());
        let v = chat_body(&c, "s", "u", 0.0);
        assert!(v.get("reasoning_effort").is_none());
        assert_eq!(v["temperature"], 0.0);
    }

    #[test]
    fn named_stream_is_sent() {
        let mut c = client();
        c.stream = true;
        let v = chat_body(&c, "s", "u", 0.3);
        assert_eq!(v["stream"], true);
    }

    #[test]
    fn sse_concatenates_delta_content() {
        let raw = concat!(
            "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"hel\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n\n",
            "data: [DONE]\n",
        );
        assert_eq!(decode_chat_content(raw, true).unwrap(), "hello");
    }
    #[test]
    fn stream_flag_still_accepts_json_object() {
        let raw = r#"{"choices":[{"message":{"role":"assistant","content":"hi"},"finish_reason":"stop"}]}"#;
        assert_eq!(decode_chat_content(raw, true).unwrap(), "hi");
    }

    #[test]
    fn neighbors_follow_context_order() {
        fn u(id: &str, ctx: &str, text: &str) -> TextUnit {
            TextUnit {
                id: id.into(),
                engine: "txt".into(),
                domain: "body".into(),
                location: id.into(),
                item_type: ItemType::ShortText,
                role: String::new(),
                original_lines: vec![text.into()],
                source_line_paths: vec![],
                context: ctx.into(),
                payload: String::new(),
            }
        }
        let units = vec![
            u("a", "ch1", "one"),
            u("b", "ch1", "two"),
            u("c", "ch2", "other"),
        ];
        let mut tr = BTreeMap::new();
        tr.insert(
            "a".into(),
            Translation {
                unit_id: "a".into(),
                translation_lines: vec!["一".into()],
                source_hash: String::new(),
                passthrough: false,
            },
        );
        let map = build_neighbor_map(&units, &tr);
        let (prev, next) = map.get("b").unwrap();
        assert_eq!(prev.as_ref().unwrap().original, "one");
        assert_eq!(prev.as_ref().unwrap().translation.as_deref(), Some("一"));
        assert!(next.is_none());
        assert!(map.get("c").unwrap().0.is_none());
        assert!(map.get("c").unwrap().1.is_none());
    }
}
