use crate::config::{LlmClient, TranslationSection};
use crate::model::{ItemType, TextUnit, Translation, mask_controls, unmask_controls};
use crate::quality;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;
use std::thread;
use std::time::Duration;

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    temperature: f32,
}

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
- 形如 [CTRL_n] 的标记必须原样保留，数量一致，不翻译。
- 文中的占位符与标记（如 {{tag}}、[var]、<tag>、%s、%d、\n）原样保留，不翻译其内部。
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
- Tokens like [CTRL_n] must be kept verbatim with the same count.
- Placeholders and markup ({{tag}}, [var], <tag>, %s, %d, \n) stay verbatim; never translate inside them.
- long_text may reflow lines; array must return exactly line_count lines; short_text has exactly 1 string in translation_lines.
- Output a strict JSON array only. No markdown.
- Each element: {{"id":"<id>","role":"<role>","translation_lines":["..."]}}
- Copy id and role from input.
"#,
            profile_line = profile_line_en(profile),
        )
    }
}

pub struct Translator {
    client: LlmClient,
    section: TranslationSection,
    http: reqwest::blocking::Client,
    system: String,
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
        })
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
        let mut id_map: BTreeMap<String, &TextUnit> = BTreeMap::new(); // prompt id -> unit

        let mut body = String::from("# 场景\n\n");
        if let Some(c) = batch.first().map(|u| u.context.as_str())
            && !c.is_empty()
        {
            body.push_str(&format!("context: {c}\n"));
        }
        body.push_str("\n# 正文\n\n");

        for (i, u) in batch.iter().enumerate() {
            let pid = (i + 1).to_string();
            id_map.insert(pid.clone(), *u);
            let mut masked_lines = Vec::new();
            let mut unit_map = Vec::new();
            for line in &u.original_lines {
                let (m, map) = mask_controls(line);
                let base = unit_map.len();
                let mut line_out = m;
                for (j, (k, v)) in map.into_iter().enumerate() {
                    let nk = format!("[CTRL_{}]", base + j);
                    line_out = line_out.replacen(&k, &nk, 1);
                    unit_map.push((nk, v));
                }
                masked_lines.push(line_out);
            }
            masks.insert(u.id.clone(), unit_map);

            body.push_str(&format!("## {pid}\n"));
            body.push_str(&format!("id: {pid}\n"));
            body.push_str(&format!("type: {}\n", u.item_type.as_str()));
            body.push_str(&format!("role: {}\n", u.role));
            if u.item_type == ItemType::Array {
                body.push_str(&format!("line_count: {}\n", u.original_lines.len()));
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
            let unit = id_map
                .get(&item.id)
                .with_context(|| format!("model returned unknown id {}", item.id))?;
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
            translations.push(Translation {
                unit_id: unit.id.clone(),
                translation_lines: lines,
                source_hash: TextUnit::source_hash(&unit.original_lines),
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
        let req = ChatRequest {
            model: self.client.model.clone(),
            messages: vec![
                ChatMessage {
                    role: "system".into(),
                    content: Some(system.into()),
                },
                ChatMessage {
                    role: "user".into(),
                    content: Some(user.into()),
                },
            ],
            temperature: 0.3,
        };
        let resp = self
            .http
            .post(&url)
            .bearer_auth(&self.client.api_key)
            .json(&req)
            .send()
            .with_context(|| format!("POST {url}"))?;
        let status = resp.status();
        let text = resp.text()?;
        if !status.is_success() {
            bail!("LLM HTTP {status}: {}", truncate(&text, 500));
        }
        let parsed: ChatResponse = serde_json::from_str(&text)
            .with_context(|| format!("decode chat: {}", truncate(&text, 300)))?;
        let choice = parsed
            .choices
            .first()
            .ok_or_else(|| anyhow::anyhow!("empty choices"))?;
        let content = choice
            .message
            .content
            .clone()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "empty content finish={:?}",
                    choice.finish_reason.as_deref().unwrap_or("?")
                )
            })?;
        Ok(content)
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
    // ponytail: keep original when model refuses (policy/empty); writeback still works
    eprintln!("  passthrough {}", u.location);
    Translation {
        unit_id: u.id.clone(),
        translation_lines: u.original_lines.clone(),
        source_hash: TextUnit::source_hash(&u.original_lines),
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

#[allow(dead_code)]
pub fn dummy_request_body() -> serde_json::Value {
    json!({"ok": true})
}
