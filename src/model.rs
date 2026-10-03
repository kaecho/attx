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



/// Kana letters and sound marks, not separators such as `・` or `゠`.
pub fn has_kana(text: &str) -> bool {
    text.chars().any(is_kana)
}

pub(crate) fn is_kana(c: char) -> bool {
    matches!(c,
        '\u{3041}'..='\u{3096}' | '\u{3099}'..='\u{309F}' |
        '\u{30A1}'..='\u{30FA}' | '\u{30FC}'..='\u{30FF}' |
        '\u{31F0}'..='\u{31FF}' | '\u{FF66}'..='\u{FF9F}' |
        '\u{1B000}'..='\u{1B122}' | '\u{1B132}' | '\u{1B150}'..='\u{1B152}' |
        '\u{1B155}' | '\u{1B164}'..='\u{1B167}'
    )
}

pub fn has_hangul(text: &str) -> bool {
    text.chars().any(|c| c.is_alphabetic() && matches!(c,
        '\u{AC00}'..='\u{D7AF}' | '\u{1100}'..='\u{11FF}' |
        '\u{3131}'..='\u{318E}' | '\u{A960}'..='\u{A97F}' |
        '\u{D7B0}'..='\u{D7FF}' | '\u{FFA0}'..='\u{FFDC}'
    ))
}

pub(crate) fn is_cjk(c: char) -> bool {
    matches!(c, '\u{3400}'..='\u{4DBF}' | '\u{4E00}'..='\u{9FFF}' |
        '\u{F900}'..='\u{FAFF}' | '\u{20000}'..='\u{2FA1F}' | '\u{30000}'..='\u{323AF}')
}

/// Rough JP/CJK source-text probe (default ja profile).
pub fn looks_like_source_ja(text: &str) -> bool {
    text.chars().any(|c| is_kana(c) || is_cjk(c))
}

pub fn looks_like_source_en(text: &str) -> bool {
    text.chars().filter(char::is_ascii_alphabetic).take(3).count() >= 3
}

pub fn needs_translation(text: &str, src: &str) -> bool {
    let lang = src.trim().split(['-', '_']).next().unwrap_or(src);
    match lang.to_ascii_lowercase().as_str() {
        "ja" | "jp" | "jpn" => looks_like_source_ja(text),
        "zh" | "zho" | "chi" | "cn" => text.chars().any(is_cjk),
        "en" | "eng" => looks_like_source_en(text),
        "ko" | "kor" => has_hangul(text),
        "ru" | "uk" | "bg" | "be" | "sr" | "mk" => text.chars().any(|c| c.is_alphabetic() && matches!(c, '\u{0400}'..='\u{052F}')),
        "ar" | "fa" | "ur" => text.chars().any(|c| c.is_alphabetic() && matches!(c, '\u{0600}'..='\u{06FF}' | '\u{0750}'..='\u{077F}' | '\u{08A0}'..='\u{08FF}')),
        "hi" | "mr" | "ne" => text.chars().any(|c| c.is_alphabetic() && matches!(c, '\u{0900}'..='\u{097F}')),
        "th" => text.chars().any(|c| c.is_alphabetic() && matches!(c, '\u{0E00}'..='\u{0E7F}')),
        "de" | "fr" | "es" | "it" | "pt" | "nl" | "pl" | "cs" | "sk" | "ro" |
        "hu" | "tr" | "vi" | "id" | "ms" | "sv" | "no" | "da" | "fi" => text.chars().any(|c| {
            c.is_alphabetic() && matches!(c, '\u{0041}'..='\u{024F}' | '\u{1E00}'..='\u{1EFF}')
        }),
        // Unknown and automatic profiles cannot assume a particular script.
        _ => text.chars().any(char::is_alphabetic),
    }
}

#[cfg(test)]
mod tests {
    use super::*;



    #[test]
    fn ja_detect() {
        assert!(looks_like_source_ja("村を出る"));
        assert!(!looks_like_source_ja("ABC"));
        assert!(has_kana("アレイ离开了"));
        assert!(!has_kana("艾蕾离开了"));
        assert!(has_hangul("안녕"));
        assert!(!has_hangul("你好"));
        assert!(!has_kana("・禁止挑食 乔・约翰尼 ゠ ･"));
        assert!(has_kana("ｶﾅ"));
        assert!(has_kana("ー"));
        assert!(needs_translation("안녕", "ko-KR"));
        assert!(needs_translation("Привет", "ru"));
    }

    #[test]
    fn source_languages_use_their_scripts() {
        for (lang, positive) in [
            ("ZH_tw", "你好"), ("ja-JP", "開始"), ("KO_kr", "안녕"),
            ("RU-ru", "Привет"), ("uk", "Привіт"), ("ar", "مرحبا"),
            ("hi", "नमस्ते"), ("th", "สวัสดี"), ("fr", "été"),
            ("vi", "Tiếng Việt"), ("auto", "Hello"), ("auto", "你好"),
        ] {
            assert!(needs_translation(positive, lang), "{lang}: {positive}");
            assert!(!needs_translation("123・!?", lang), "{lang}");
        }
        assert!(!needs_translation("こんにちは", "zh-tw"));
        assert!(!needs_translation("你好", "ko"));
        assert!(!needs_translation("١٢٣", "ar"));
        assert!(!needs_translation("๑๒๓", "th"));
        assert!(!looks_like_source_ja("・"));
        assert!(looks_like_source_ja("ｶﾅ"));
    }
}
