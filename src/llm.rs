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
    content: String,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: ChatMessage,
}

#[derive(Debug, Deserialize)]
struct ModelItem {
    #[serde(deserialize_with = "deserialize_id")]
    id: String,
    #[serde(default)]
    role: String,
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

const SYSTEM_JA: &str = r#"你是游戏本地化译者，将源语言翻译为简体中文。
规则：
- 对白自然口语化，系统/UI 文本简洁准确。
- 忠实保留原意、语气和内容尺度，不净化、不扩写。
- 形如 [CTRL_n] 的标记必须原样保留，数量一致，不翻译。
- long_text 可按中文语气调整断句；array 必须输出 line_count 行；short_text 的 translation_lines 只有 1 个字符串。
- 顶层输出严格 JSON 数组，不要 Markdown、解释或额外文本。
- 每个元素：{"id":"<id>","role":"<角色>","translation_lines":["..."]}
- id 与 role 原样复制输入；没有角色时 role 为空字符串。
"#;

const SYSTEM_EN: &str = r#"You are a game localizer. Translate source text into Simplified Chinese.
Rules:
- Natural dialogue; concise UI/system text.
- Keep meaning, tone, and content rating. Do not censor or expand.
- Tokens like [CTRL_n] must be kept verbatim with the same count.
- long_text may reflow lines; array must return exactly line_count lines; short_text has exactly 1 string in translation_lines.
- Output a strict JSON array only. No markdown.
- Each element: {"id":"<id>","role":"<role>","translation_lines":["..."]}
- Copy id and role from input.
"#;

pub struct Translator {
    client: LlmClient,
    section: TranslationSection,
    http: reqwest::blocking::Client,
    source_lang: String,
}

impl Translator {
    pub fn new(client: &LlmClient, section: &TranslationSection, source_lang: &str) -> Result<Self> {
        let http = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(client.timeout.max(30)))
            .build()?;
        Ok(Self {
            client: client.clone(),
            section: section.clone(),
            http,
            source_lang: source_lang.to_string(),
        })
    }

    pub fn translate_units(
        &self,
        units: &[TextUnit],
        limit: Option<usize>,
    ) -> Result<Vec<Translation>> {
        let slice: Vec<&TextUnit> = units.iter().take(limit.unwrap_or(usize::MAX)).collect();
        if slice.is_empty() {
            return Ok(vec![]);
        }
        let batches = batch_units(&slice, self.section.batch_chars, self.section.max_context_items);
        let mut out = Vec::new();
        let mut done = 0usize;
        let total = slice.len();
        for (bi, batch) in batches.iter().enumerate() {
            eprintln!(
                "batch {}/{} ({} units, progress {}/{})",
                bi + 1,
                batches.len(),
                batch.len(),
                done,
                total
            );
            let mut attempt = 0u32;
            loop {
                attempt += 1;
                match self.translate_batch(batch) {
                    Ok(mut items) => {
                        done += items.len();
                        out.append(&mut items);
                        break;
                    }
                    Err(e) if attempt <= self.section.retry_count => {
                        eprintln!("  retry {attempt}: {e:#}");
                        thread::sleep(Duration::from_secs(self.section.retry_delay));
                    }
                    Err(e) => return Err(e),
                }
            }
            // crude RPM pacing
            if self.section.rpm > 0 {
                let sleep_ms = 60_000u64 / u64::from(self.section.rpm.max(1));
                thread::sleep(Duration::from_millis(sleep_ms.min(2000)));
            }
        }
        Ok(out)
    }

    fn translate_batch(&self, batch: &[&TextUnit]) -> Result<Vec<Translation>> {
        let mut masks: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
        let mut id_map: BTreeMap<String, &TextUnit> = BTreeMap::new(); // prompt id -> unit

        let mut body = String::from("# 场景\n\n");
        // group hint: first context
        if let Some(c) = batch.first().map(|u| u.context.as_str()) {
            if !c.is_empty() {
                body.push_str(&format!("context: {c}\n"));
            }
        }
        body.push_str("\n# 正文\n\n");

        for (i, u) in batch.iter().enumerate() {
            let pid = (i + 1).to_string();
            id_map.insert(pid.clone(), *u);
            let mut masked_lines = Vec::new();
            let mut unit_map = Vec::new();
            for line in &u.original_lines {
                let (m, map) = mask_controls(line);
                // offset map keys to be unique across lines
                let base = unit_map.len();
                let mut remap = Vec::new();
                let mut line_out = m;
                for (j, (k, v)) in map.into_iter().enumerate() {
                    let nk = format!("[CTRL_{}]", base + j);
                    line_out = line_out.replacen(&k, &nk, 1);
                    remap.push((nk, v));
                }
                unit_map.extend(remap);
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

        let system = if self.source_lang == "en" {
            SYSTEM_EN
        } else {
            SYSTEM_JA
        };
        let raw = self.chat(system, &body)?;
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
            if let Err(e) = quality::check_unit(unit, &lines) {
                bail!("quality failed for {}: {e}", unit.location);
            }
            // short_text force single line
            if unit.item_type == ItemType::ShortText && lines.len() != 1 {
                lines = vec![lines.join("")];
            }
            translations.push(Translation {
                unit_id: unit.id.clone(),
                translation_lines: lines,
                source_hash: TextUnit::source_hash(&unit.original_lines),
            });
        }
        if translations.len() != batch.len() {
            bail!(
                "model returned {} items, expected {}",
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
                    content: system.into(),
                },
                ChatMessage {
                    role: "user".into(),
                    content: user.into(),
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
        let parsed: ChatResponse =
            serde_json::from_str(&text).with_context(|| format!("decode chat: {}", truncate(&text, 300)))?;
        let content = parsed
            .choices
            .first()
            .map(|c| c.message.content.clone())
            .ok_or_else(|| anyhow::anyhow!("empty choices"))?;
        Ok(content)
    }

    pub fn ping(&self) -> Result<String> {
        self.chat("Reply with exactly: pong", "ping")
    }
}

fn batch_units<'a>(
    units: &[&'a TextUnit],
    batch_chars: usize,
    max_context: usize,
) -> Vec<Vec<&'a TextUnit>> {
    let mut batches = Vec::new();
    let mut cur: Vec<&TextUnit> = Vec::new();
    let mut chars = 0usize;
    let mut last_ctx = "";
    for u in units {
        let size: usize = u.original_lines.iter().map(|l| l.chars().count()).sum();
        let same_ctx = u.context == last_ctx || last_ctx.is_empty();
        let would_exceed = chars + size > batch_chars && !cur.is_empty();
        let ctx_break = !same_ctx && !cur.is_empty() && cur.len() >= max_context;
        if would_exceed || ctx_break {
            batches.push(std::mem::take(&mut cur));
            chars = 0;
        }
        last_ctx = u.context.as_str();
        chars += size;
        cur.push(*u);
    }
    if !cur.is_empty() {
        batches.push(cur);
    }
    batches
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
    serde_json::from_str(slice).with_context(|| format!("parse model json: {}", truncate(slice, 400)))
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
