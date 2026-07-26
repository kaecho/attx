use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Engine-agnostic translatable unit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextUnit {
    /// Stable identity (engine+path hash or explicit id)
    pub id: String,
    /// Engine adapter id that produced this unit
    pub engine: String,
    /// Domain within engine (dialogue, system, base, jsonl, ...)
    pub domain: String,
    /// Human / machine locator inside the game (e.g. Map003.json/2/0/5)
    pub location: String,
    pub item_type: ItemType,
    pub role: String,
    pub original_lines: Vec<String>,
    /// Per-line writeback anchors; same length as original_lines when used
    #[serde(default)]
    pub source_line_paths: Vec<String>,
    /// Optional scene/context grouping key for batching
    #[serde(default)]
    pub context: String,
    /// Extra engine payload (JSON object string)
    #[serde(default)]
    pub payload: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemType {
    LongText,
    Array,
    ShortText,
}

impl ItemType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LongText => "long_text",
            Self::Array => "array",
            Self::ShortText => "short_text",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "array" => Self::Array,
            "short_text" => Self::ShortText,
            _ => Self::LongText,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Translation {
    pub unit_id: String,
    pub translation_lines: Vec<String>,
    /// sha256 of original_lines joined — cache key fragment
    pub source_hash: String,
    /// True when the model failed/refused and the original text was kept as a
    /// placeholder. Tracked so `status` can report it and
    /// `translate --retry-passthrough` can re-queue these units.
    #[serde(default)]
    pub passthrough: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceMeta {
    pub engine: String,
    pub game_path: String,
    pub content_root: String,
    pub source_lang: String,
    pub target_lang: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonlRecord {
    pub id: String,
    pub text: String,
    #[serde(default)]
    pub context: String,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub item_type: Option<String>,
    #[serde(default)]
    pub translation: Option<String>,
    #[serde(default)]
    pub translation_lines: Option<Vec<String>>,
}

impl TextUnit {
    pub fn compute_id(engine: &str, location: &str, original_lines: &[String]) -> String {
        let mut h = Sha256::new();
        h.update(engine.as_bytes());
        h.update(b"\0");
        h.update(location.as_bytes());
        h.update(b"\0");
        for line in original_lines {
            h.update(line.as_bytes());
            h.update(b"\n");
        }
        format!("{:x}", h.finalize())[..24].to_string()
    }

    pub fn source_hash(original_lines: &[String]) -> String {
        let mut h = Sha256::new();
        for line in original_lines {
            h.update(line.as_bytes());
            h.update(b"\n");
        }
        format!("{:x}", h.finalize())
    }

    pub fn joined_text(&self) -> String {
        self.original_lines.join("\n")
    }
}

/// RMMZ control codes → semantic placeholders for the model.
pub fn mask_controls(text: &str) -> (String, Vec<(String, String)>) {
    // Match \ followed by letter/symbol forms used by RMMV/MZ: \C[1], \n[1], \., \!, \>, \G, \\
    let re = regex::Regex::new(
        r"(?x)
        \\{2}                                   # escaped backslash
        | \\[VvNnCcGg]\[\d+\]                   # \V[n] \N[n] \C[n] \G[n] (case variants)
        | \\[VvNnCcGg]                          # bare
        | \\[!.>|{\}\\\$\^]                     # single-char controls
        | \\[A-Za-z]\[\d+\]                     # other letter[n]
        | \\[A-Za-z]                            # other letter
        ",
    )
    .expect("control regex");

    let mut map = Vec::new();
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for (i, m) in re.find_iter(text).enumerate() {
        out.push_str(&text[last..m.start()]);
        let key = format!("[CTRL_{i}]");
        map.push((key.clone(), m.as_str().to_string()));
        out.push_str(&key);
        last = m.end();
    }
    out.push_str(&text[last..]);
    (out, map)
}

pub fn unmask_controls(text: &str, map: &[(String, String)]) -> String {
    let mut out = text.to_string();
    for (k, v) in map {
        out = out.replace(k, v);
    }
    out
}

/// Mask all lines of a unit with a single, unit-wide `[CTRL_n]` numbering.
///
/// Renaming a line's per-line keys (`[CTRL_0]`, `[CTRL_1]`, …) to their
/// unit-wide index must go highest-first: the new index is always ≥ the old
/// one, so renaming low keys first could produce a token that collides with a
/// key not yet renamed on the same line — the next `replacen` would then hit
/// the freshly renamed token and the control codes would swap positions.
pub fn mask_unit_lines(lines: &[String]) -> (Vec<String>, Vec<(String, String)>) {
    let mut unit_map: Vec<(String, String)> = Vec::new();
    let mut masked_lines = Vec::with_capacity(lines.len());
    for line in lines {
        let (m, map) = mask_controls(line);
        let base = unit_map.len();
        let mut line_out = m;
        let mut renamed = vec![(String::new(), String::new()); map.len()];
        for (j, (k, v)) in map.into_iter().enumerate().rev() {
            let nk = format!("[CTRL_{}]", base + j);
            line_out = line_out.replacen(&k, &nk, 1);
            renamed[j] = (nk, v);
        }
        unit_map.extend(renamed);
        masked_lines.push(line_out);
    }
    (masked_lines, unit_map)
}

/// Rough JP/CJK source-text probe (default ja profile).
pub fn looks_like_source_ja(text: &str) -> bool {
    text.chars().any(|c| {
        matches!(c,
            '\u{3040}'..='\u{309F}' | // hiragana
            '\u{30A0}'..='\u{30FF}' | // katakana + prolonged sound
            '\u{4E00}'..='\u{9FFF}'   // CJK
        )
    })
}

pub fn looks_like_source_en(text: &str) -> bool {
    let letters: String = text.chars().filter(|c| c.is_ascii_alphabetic()).collect();
    letters.len() >= 3
}

pub fn needs_translation(text: &str, src: &str) -> bool {
    match src {
        "en" => looks_like_source_en(text),
        _ => looks_like_source_ja(text),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_roundtrip() {
        let s = r"\C[1]こんにちは\n[1]";
        let (m, map) = mask_controls(s);
        assert!(m.contains("[CTRL_"));
        assert_eq!(unmask_controls(&m, &map), s);
    }

    #[test]
    fn mask_unit_no_renumber_collision() {
        // Line 0 contributes 1 control, line 1 contributes 2 — the naive
        // low-first rename used to swap \C[2] and \V[7] on line 1.
        let lines = vec![r"\C[1]おはよう".to_string(), r"\C[2]やあ\V[7]".to_string()];
        let (masked, map) = mask_unit_lines(&lines);
        assert_eq!(masked[0], "[CTRL_0]おはよう");
        assert_eq!(masked[1], "[CTRL_1]やあ[CTRL_2]");
        assert_eq!(unmask_controls(&masked[0], &map), lines[0]);
        assert_eq!(unmask_controls(&masked[1], &map), lines[1]);
        let m: std::collections::BTreeMap<_, _> = map.into_iter().collect();
        assert_eq!(m["[CTRL_1]"], r"\C[2]");
        assert_eq!(m["[CTRL_2]"], r"\V[7]");
    }

    #[test]
    fn ja_detect() {
        assert!(looks_like_source_ja("村を出る"));
        assert!(!looks_like_source_ja("ABC"));
    }
}
