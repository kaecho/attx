use crate::layout;
use crate::model::{self, ItemType, TextUnit};
use crate::preserve::{self, PreserveSet};
use anyhow::{Result, bail};
use regex::Regex;
use std::sync::LazyLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KanaKind {
    Edge,
    Mixed,
    Untranslated,
}

pub fn check_unit(unit: &TextUnit, translation_lines: &[String]) -> Result<()> {
    if translation_lines.is_empty() || translation_lines.iter().all(|l| l.trim().is_empty()) {
        bail!("empty translation");
    }
    match unit.item_type {
        ItemType::Array => {
            if translation_lines.len() != unit.original_lines.len() {
                bail!(
                    "array line_count mismatch: got {} want {}",
                    translation_lines.len(), unit.original_lines.len()
                );
            }
            for (i, (src, dst)) in unit.original_lines.iter().zip(translation_lines).enumerate() {
                if !src.trim().is_empty() && dst.trim().is_empty() {
                    bail!("empty translation for array choice {}", i + 1);
                }
            }
        }
        ItemType::ShortText if translation_lines.len() != 1 => {
            bail!("short_text must have 1 line, got {}", translation_lines.len());
        }
        _ => {}
    }
    let map = protected_tokens(&unit.original_lines, PreserveSet::core());
    let lost = preserve::lost_token_count(translation_lines, &map);
    if lost > 0 {
        bail!("preserved tokens lost: {lost}/{}", map.len());
    }
    Ok(())
}

/// Normalize sanitized target lines, restore preserved-only source lines, then
/// apply mechanical kana cleanup. Returns the number of changed lines so callers
/// can report cache normalization.
pub fn normalize_lines(unit: &TextUnit, lines: &mut Vec<String>, target_lang: &str, preserve: &PreserveSet) -> usize {
    let mut changed = sanitize_in_place(unit, lines);
    changed += restore_preserved_lines(unit, lines, preserve);
    changed += normalize_target_lines(lines, target_lang, preserve);
    changed
}

/// Model-emitted line-break normalization. Models occasionally return `\r\n`
/// or embed a line break inside one JSON string; the writeback slots are
/// single-line, so the break is structural damage, not formatting.
fn sanitize_in_place(unit: &TextUnit, lines: &mut Vec<String>) -> usize {
    let mut changed = 0;
    for line in lines.iter_mut() {
        if line.contains('\r') {
            *line = line.replace("\r\n", "\n").replace('\r', "\n");
            changed += 1;
        }
    }
    match unit.item_type {
        ItemType::ShortText if lines.iter().any(|line| line.contains('\n')) => {
            for line in lines.iter_mut() {
                *line = join_short_line(line);
            }
            changed += 1;
        }
        ItemType::Array | ItemType::LongText if lines.iter().any(|line| line.contains('\n')) => {
            let mut expanded = Vec::with_capacity(lines.len());
            for line in lines.drain(..) {
                expanded.extend(line.split('\n').map(str::to_string));
            }
            *lines = expanded;
            changed += 1;
        }
        _ => {}
    }
    if unit.item_type == ItemType::LongText && lines.len() > 1 && lines.iter().any(|line| !line.trim().is_empty()) {
        while lines.len() > 1 && lines.last().is_some_and(|line| line.trim().is_empty()) {
            lines.pop();
            changed += 1;
        }
    }
    changed
}

/// A single-line slot must stay single: CJK breaks join directly, while Latin
/// words keep a space so `Hello\nworld` does not become `Helloworld`.
fn join_short_line(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    for (index, part) in line.split('\n').enumerate() {
        if index > 0 {
            let previous = out.chars().next_back();
            let next = part.chars().next();
            let keep_whitespace = previous.is_some_and(char::is_whitespace) || next.is_some_and(char::is_whitespace);
            if !keep_whitespace && !(previous.is_some_and(is_wide) && next.is_some_and(is_wide)) {
                out.push(' ');
            }
        }
        out.push_str(part);
    }
    out
}

fn is_wide(c: char) -> bool {
    model::is_kana(c)
        || model::is_cjk(c)
        || matches!(c,
            '\u{AC00}'..='\u{D7AF}' | '\u{1100}'..='\u{11FF}' |
            '\u{3000}'..='\u{303F}' | '\u{FF01}'..='\u{FF60}' | '\u{FFE0}'..='\u{FFE6}'
        )
}

/// Restore source lines that carry no translatable text — engine controls,
/// markup or placeholders only. They are evidence, not translation targets, and
/// models sometimes drop them entirely. Restoration is deliberately narrow: line
/// counts must agree and the target slot must be blank, so an intentional reflow
/// or model-written content is never overwritten.
pub fn restore_preserved_lines(unit: &TextUnit, lines: &mut [String], preserve: &PreserveSet) -> usize {
    if lines.len() != unit.original_lines.len() {
        return 0;
    }
    let mut restored = 0;
    for (target, source) in lines.iter_mut().zip(&unit.original_lines) {
        let (masked, map) = preserve.mask_line(source);
        if map.is_empty() || !only_mask_tokens(&masked) {
            continue;
        }
        if !target.trim().is_empty() {
            continue;
        }
        *target = source.clone();
        restored += 1;
    }
    restored
}

fn only_mask_tokens(masked: &str) -> bool {
    let mut rest = masked;
    loop {
        let trimmed = rest.trim_start();
        if trimmed.is_empty() {
            return true;
        }
        let Some(close) = trimmed.find(']') else { return false };
        if !trimmed.starts_with("[CTRL_") || !trimmed[6..close].bytes().all(|b| b.is_ascii_digit()) {
            return false;
        }
        rest = &trimmed[close + 1..];
    }
}

/// Normalize only mechanical kana in lines with positive Chinese-language evidence.
pub fn normalize_target_lines(lines: &mut [String], target_lang: &str, preserve: &PreserveSet) -> usize {
    if language(target_lang) != "zh" {
        return 0;
    }
    let mut modified = 0;
    for line in lines {
        if !model::has_kana(line) {
            continue;
        }
        let mut spans = protected_spans(line, preserve);
        let visible = without_spans(line, &spans);
        // Unknown kana words make mechanical changes too ambiguous. Quoted glyph
        // examples remain visible to review, but are not normalization candidates.
        let quotes = japanese_quote_spans(&visible);
        let context = without_spans(&visible, &quotes);
        if !model::has_kana(&context) || !clear_chinese_evidence(&context)
            || kana_runs(&context).any(|run| !mechanical(run))
        {
            continue;
        }
        spans.extend(QUOTES.find_iter(line).filter_map(|m| {
            let quoted = without_spans(m.as_str(), &protected_spans(m.as_str(), preserve));
            (model::has_kana(&quoted) && !chinese_context(&quoted)).then_some((m.start(), m.end()))
        }));
        merge_spans(&mut spans);
        let mut out = String::with_capacity(line.len());
        let mut cursor = 0;
        for (start, end) in spans {
            normalize_segment(&line[cursor..start], &mut out);
            out.push_str(&line[start..end]);
            cursor = end;
        }
        normalize_segment(&line[cursor..], &mut out);
        if out != *line {
            *line = out;
            modified += 1;
        }
    }
    modified
}

/// Return one disjoint category per unit, choosing its most severe visible line.
pub fn kana_kind(lines: &[String], target_lang: &str, preserve: &PreserveSet) -> Option<KanaKind> {
    let target = language(target_lang);
    if target == "ja" {
        return None;
    }
    kana_kind_visible(&visible_lines(lines, preserve), &target)
}

pub(crate) fn kana_kind_visible(lines: &[String], target: &str) -> Option<KanaKind> {
    if target == "ja" {
        return None;
    }
    let mut kind = None;
    for line in lines {
        if !model::has_kana(line) {
            continue;
        }
        let current = if target == "zh" {
            if !chinese_context(line) {
                KanaKind::Untranslated
            } else if japanese_quote_spans(line).is_empty()
                && kana_runs(line).all(|run| run.chars().all(|c| matches!(c, 'っ' | 'ッ' | 'ー')))
            {
                KanaKind::Edge
            } else {
                KanaKind::Mixed
            }
        } else if line.chars().any(|c| c.is_ascii_alphabetic() || matches!(c, '\u{AC00}'..='\u{D7AF}' | '\u{1100}'..='\u{11FF}')) {
            KanaKind::Mixed
        } else {
            KanaKind::Untranslated
        };
        kind = Some(match (kind, current) {
            (Some(KanaKind::Untranslated), _) | (_, KanaKind::Untranslated) => KanaKind::Untranslated,
            (Some(KanaKind::Mixed), _) | (_, KanaKind::Mixed) => KanaKind::Mixed,
            _ => KanaKind::Edge,
        });
    }
    kind
}

pub fn check_translation(
    unit: &TextUnit,
    lines: &[String],
    source_lang: &str,
    target_lang: &str,
    preserve: &PreserveSet,
) -> Result<()> {
    check_unit(unit, lines)?;
    let map = protected_tokens(&unit.original_lines, preserve);
    let lost = preserve::lost_token_count(lines, &map);
    if lost > 0 {
        bail!("preserved tokens lost: {lost}/{}", map.len());
    }
    preserve::check_preserved_literals(lines, &map, preserve)?;
    if language(source_lang) == language(target_lang) {
        return Ok(());
    }
    let src = visible_lines(&unit.original_lines, preserve).join("\n");
    let dst = visible_lines(lines, preserve).join("\n");
    if unit.engine == "rmmz"
        && unit
            .original_lines
            .first()
            .is_some_and(|line| layout::standalone_label(line).is_some())
    {
        let head = lines.first().map(String::as_str).unwrap_or_default();
        let Some(label) = layout::leading_label(head) else {
            bail!("missing translated speaker label");
        };
        let source_has_body = unit
            .original_lines
            .iter()
            .skip(1)
            .any(|line| layout::has_visible_text(line));
        let translated_has_body = layout::has_visible_text(label.suffix)
            || lines
                .iter()
                .skip(1)
                .any(|line| layout::has_visible_text(line));
        if source_has_body && !translated_has_body {
            bail!("missing dialogue after speaker label");
        }
    }
    if src.trim() == dst.trim() && identical_requires_translation(&src, source_lang, target_lang) {
        bail!("translation identical to translatable source");
    }
    if language(target_lang) == "zh" && model::has_kana(&dst) {
        bail!("Chinese translation still contains unprotected Japanese kana");
    }
    if let Some(script) = residual_script(&dst, source_lang, target_lang) {
        bail!("translation still contains source {script}");
    }
    Ok(())
}

pub(crate) fn identical_requires_translation(text: &str, source: &str, target: &str) -> bool {
    let source_base = language(source);
    let target_base = language(target);
    if source_base == target_base { return false }
    // Kanji-only UI labels can be correct unchanged in either shared script.
    let shared_cjk = matches!((source_base.as_str(), target_base.as_str()), ("ja", "zh") | ("zh", "ja"));
    if shared_cjk && !model::has_kana(text) { return false }
    model::needs_translation(text, source)
}

pub(crate) fn language(lang: &str) -> String {
    let base = lang.trim().split(['-', '_']).next().unwrap_or(lang).to_ascii_lowercase();
    match base.as_str() {
        "jp" | "jpn" => "ja".into(),
        "zho" | "chi" | "cn" => "zh".into(),
        "eng" => "en".into(),
        "kor" => "ko".into(),
        _ => base,
    }
}

pub(crate) fn residual_script(text: &str, source_lang: &str, target_lang: &str) -> Option<&'static str> {
    let src = language(source_lang);
    let dst = language(target_lang);
    if src == dst {
        return None;
    }
    match src.as_str() {
        "ja" if dst != "ja" && model::has_kana(text) => Some("kana"),
        "ko" if dst != "ko" && model::has_hangul(text) => Some("hangul"),
        "zh" if !matches!(dst.as_str(), "zh" | "ja") && text.chars().any(is_han) => Some("CJK"),
        "ru" | "uk" | "bg" | "be" | "sr" | "mk"
            if !matches!(dst.as_str(), "ru" | "uk" | "bg" | "be" | "sr" | "mk")
                && model::needs_translation(text, "ru") => Some("Cyrillic"),
        "ar" | "fa" | "ur" if !matches!(dst.as_str(), "ar" | "fa" | "ur")
            && model::needs_translation(text, "ar") => Some("Arabic"),
        "hi" | "mr" | "ne" if !matches!(dst.as_str(), "hi" | "mr" | "ne")
            && model::needs_translation(text, "hi") => Some("Devanagari"),
        "th" if dst != "th" && model::needs_translation(text, "th") => Some("Thai"),
        _ => None,
    }
}

static QUOTES: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new("「[^「」\\r\\n]*」|『[^『』\\r\\n]*』|“[^“”\\r\\n]*”|‘[^‘’\\r\\n]*’|\"[^\"\\r\\n]*\"|'[^'\\r\\n]*'|`[^`\\r\\n]*`").expect("quality quote regex")
});

fn protected_spans(text: &str, preserve: &PreserveSet) -> Vec<(usize, usize)> {
    let (masked, map) = preserve.mask_line(text);
    let mut spans = Vec::new();
    let mut cursor = 0;
    let mut masked_cursor = 0;
    for (key, literal) in map {
        let mut search = masked_cursor;
        while let Some(offset) = masked[search..].find(&key) {
            let key_start = search + offset;
            let start = cursor + key_start - masked_cursor;
            if text.get(start..).is_some_and(|tail| tail.starts_with(&literal)) {
                cursor = start + literal.len();
                masked_cursor = key_start + key.len();
                spans.push((start, cursor));
                break;
            }
            search = key_start + key.len();
        }
    }
    spans
}

fn merge_spans(spans: &mut Vec<(usize, usize)>) {
    spans.sort_unstable();
    let mut n = 0;
    for i in 0..spans.len() {
        let (start, end) = spans[i];
        if n > 0 && start < spans[n - 1].1 {
            spans[n - 1].1 = spans[n - 1].1.max(end);
        } else {
            spans[n] = (start, end);
            n += 1;
        }
    }
    spans.truncate(n);
}

fn without_spans(text: &str, spans: &[(usize, usize)]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0;
    for &(start, end) in spans {
        out.push_str(&text[cursor..start]);
        out.push(' ');
        cursor = end;
    }
    out.push_str(&text[cursor..]);
    out
}

pub(crate) fn visible_lines(lines: &[String], preserve: &PreserveSet) -> Vec<String> {
    lines.iter().map(|line| without_spans(line, &protected_spans(line, preserve))).collect()
}

pub(crate) fn protected_tokens(lines: &[String], preserve: &PreserveSet) -> Vec<(String, String)> {
    let mut tokens = Vec::new();
    for line in lines {
        for (start, end) in protected_spans(line, preserve) {
            tokens.push((format!("[CTRL_{}]", tokens.len()), line[start..end].to_string()));
        }
    }
    tokens
}

fn japanese_quote_spans(text: &str) -> Vec<(usize, usize)> {
    QUOTES.find_iter(text)
        .filter(|m| model::has_kana(m.as_str()) && !chinese_context(m.as_str()))
        .map(|m| (m.start(), m.end()))
        .collect()
}

fn is_han(c: char) -> bool {
    model::is_cjk(c)
}

fn chinese_marker(text: &str) -> bool {
    text.chars().any(|c| "这這吗嗎么麼你妳们們唔呜嗚咯哟喲哦呀啊咱啥哪没沒请热说话让别还对现怀爷译该难觉实爱开".contains(c))
}

fn clear_chinese_evidence(text: &str) -> bool {
    chinese_marker(text) || ["小心", "舒服", "羞耻", "羞恥", "不要", "不行", "务必", "務必", "字形", "今天", "现在", "現在", "谢谢", "謝謝"]
        .iter().any(|word| text.contains(word))
}

fn chinese_context(text: &str) -> bool {
    let han = text.chars().filter(|&c| is_han(c)).count();
    if han == 0 {
        return false;
    }
    // Require language evidence, not just shared Japanese/Chinese ideographs.
    let strong = chinese_marker(text);
    let evidence = clear_chinese_evidence(text)
        || (han >= 2 && text.chars().any(|c| matches!(c, '了' | '的')));
    let japanese_grammar = kana_runs(text).any(|run| matches!(run, "の" | "は" | "を" | "が" | "に" | "で" | "と" | "も" | "へ"));
    let other_kana = kana_runs(text).filter(|run| !mechanical(run)).map(|run| run.chars().count()).sum::<usize>();
    evidence && han >= other_kana && (strong || !japanese_grammar)
}

fn kana_runs(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| !model::is_kana(c)).filter(|s| !s.is_empty())
}

fn mechanical(run: &str) -> bool {
    let mut remaining = run;
    while !remaining.is_empty() {
        if let Some(tail) = remaining.strip_prefix("っすよ").or_else(|| remaining.strip_prefix("っす")) {
            remaining = tail;
        } else {
            let c = remaining.chars().next().expect("nonempty kana run");
            if !matches!(c, 'っ' | 'ッ' | 'ー') {
                return false;
            }
            remaining = &remaining[c.len_utf8()..];
        }
    }
    true
}

fn normalize_segment(text: &str, out: &mut String) {
    let mut iter = text.char_indices().peekable();
    let mut cursor = 0;
    while let Some((start, c)) = iter.next() {
        if !model::is_kana(c) {
            continue;
        }
        let mut end = start + c.len_utf8();
        while let Some(&(offset, next)) = iter.peek() {
            if !model::is_kana(next) {
                break;
            }
            end = offset + next.len_utf8();
            iter.next();
        }
        out.push_str(&text[cursor..start]);
        let run = &text[start..end];
        if mechanical(run) {
            let mut remaining = run;
            while !remaining.is_empty() {
                if let Some(tail) = remaining.strip_prefix("っすよ").or_else(|| remaining.strip_prefix("っす")) {
                    out.push('哦');
                    remaining = tail;
                } else {
                    let c = remaining.chars().next().expect("nonempty mechanical run");
                    if c == 'ー' {
                        out.push('～');
                    }
                    remaining = &remaining[c.len_utf8()..];
                }
            }
        } else {
            out.push_str(run);
        }
        cursor = end;
    }
    out.push_str(&text[cursor..]);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(values: &[&str]) -> Vec<String> {
        values.iter().map(|s| (*s).into()).collect()
    }

    fn unit(item_type: ItemType, values: &[&str]) -> TextUnit {
        TextUnit {
            id: "test".into(), engine: "rmmz".into(), domain: "dialogue".into(),
            location: "Map001.json/events/1".into(), item_type, role: String::new(),
            original_lines: lines(values), source_line_paths: Vec::new(),
            context: String::new(), payload: String::new(),
        }
    }

    #[test]
    fn reported_chinese_samples_and_aliases() {
        for lang in ["zh", "zh-tw", "ZH_CN", "zh-Hant", "cn", "zho"] {
            let mut output = lines(&[
                "「呀呜，这、这种姿势……", "唔，不要，好、好羞耻……っ」",
                "啊呜……ッ", "各位请务必小心っす。", "没问题っすよ",
                "哟，奥伦在吗ー？", "啊ー太舒服了", "咯咯ーー", "・禁止挑食", "乔・约翰尼",
            ]);
            assert_eq!(normalize_target_lines(&mut output, lang, PreserveSet::core()), 7);
            assert_eq!(output, lines(&[
                "「呀呜，这、这种姿势……", "唔，不要，好、好羞耻……」",
                "啊呜……", "各位请务必小心哦。", "没问题哦",
                "哟，奥伦在吗～？", "啊～太舒服了", "咯咯～～", "・禁止挑食", "乔・约翰尼",
            ]));
            assert_eq!(normalize_target_lines(&mut output, lang, PreserveSet::core()), 0);
        }
    }

    #[test]
    fn japanese_sentences_names_and_semantic_quotes_are_untouched() {
        let mut output = lines(&[
            "今日はいい天気っ", "オーレンいるかー？", "各位、お気をつけっす。",
            "王っ", "アーレン", "反弓成「く」字形", "这是「っ」这个日文字形",
            "这里用『っすよ』表示日语口癖", "不要の態度っ",
            "这里的'っ'不能删除", "用`ー`表示长音", "日文‘っすよ’是口癖",
        ]);
        let original = output.clone();
        assert_eq!(normalize_target_lines(&mut output, "zh", PreserveSet::core()), 0);
        assert_eq!(output, original);
        assert_eq!(kana_kind(&lines(&["反弓成「く」字形"]), "zh", PreserveSet::core()), Some(KanaKind::Mixed));
        assert_eq!(kana_kind(&lines(&["今日はいい天気っ"]), "zh", PreserveSet::core()), Some(KanaKind::Untranslated));
        assert_eq!(kana_kind(&lines(&["好羞耻……っ"]), "zh", PreserveSet::core()), Some(KanaKind::Edge));
        assert_eq!(kana_kind(&lines(&["小心っす"]), "zh", PreserveSet::core()), Some(KanaKind::Mixed));
        assert_eq!(kana_kind(&lines(&["ｶﾅ"]), "zh", PreserveSet::core()), Some(KanaKind::Untranslated));
    }

    #[test]
    fn combined_mechanical_suffixes_leave_semantic_quotes_intact() {
        let mut output = lines(&["小心っすよー", "这是「っ」字形っ", "啊にっ"]);
        assert_eq!(normalize_target_lines(&mut output, "zh", PreserveSet::core()), 2);
        assert_eq!(output, lines(&["小心哦～", "这是「っ」字形", "啊にっ"]));
    }

    #[test]
    fn preserve_internals_and_non_chinese_targets() {
        let mut output = lines(&[r"你好\SE[カナ]っ", r"「啊\SE[カナ]ー」", r"{name}・\SE[っすよー]"]);
        assert_eq!(normalize_target_lines(&mut output, "zh", PreserveSet::core()), 2);
        assert_eq!(output, lines(&[r"你好\SE[カナ]", r"「啊\SE[カナ]～」", r"{name}・\SE[っすよー]"]));
        assert_eq!(kana_kind(&output, "zh", PreserveSet::core()), None);
        let control = unit(ItemType::ShortText, &[r"\SE[カナ]"]);
        assert!(check_translation(&control, &control.original_lines, "ja", "zh", PreserveSet::core()).is_ok());
        for lang in ["en", "ja", "ko", "ru"] {
            let mut text = lines(&["小心っすよー"]);
            let original = text.clone();
            assert_eq!(normalize_target_lines(&mut text, lang, PreserveSet::core()), 0);
            assert_eq!(text, original);
        }
    }

    #[test]
    fn missing_choices_and_wrong_structure_are_not_sanitized_into_success() {
        let sanitized = |unit: &TextUnit, candidate: Vec<String>| {
            let mut candidate = candidate;
            sanitize_in_place(unit, &mut candidate);
            candidate
        };
        let choices = unit(ItemType::Array, &["はい", "いいえ"]);
        for candidate in [vec![], lines(&["是"]), lines(&["是", ""]), lines(&["是", "否", "多余"])] {
            let out = sanitized(&choices, candidate.clone());
            assert_eq!(out, candidate);
            assert!(check_unit(&choices, &out).is_err());
        }
        let short = unit(ItemType::ShortText, &["はい"]);
        for candidate in [vec![], lines(&[""]), lines(&["是", "否"])] {
            let out = sanitized(&short, candidate.clone());
            assert_eq!(out, candidate);
            assert!(check_unit(&short, &out).is_err());
        }
        let blanks = unit(ItemType::Array, &["", "はい"]);
        assert!(check_unit(&blanks, &lines(&["", "是"])).is_ok());
    }

    #[test]
    fn embedded_newlines_are_normalized_without_inventing_structure() {
        let sanitized = |unit: &TextUnit, candidate: Vec<String>| {
            let mut candidate = candidate;
            assert!(sanitize_in_place(unit, &mut candidate) > 0);
            candidate
        };
        let short = unit(ItemType::ShortText, &["はい"]);
        assert_eq!(sanitized(&short, lines(&["第一行\n第二行"])), lines(&["第一行第二行"]));
        let latin = unit(ItemType::ShortText, &["Hello"]);
        assert_eq!(sanitized(&latin, lines(&["Hello\nworld"])), lines(&["Hello world"]));
        let long = unit(ItemType::LongText, &["一行目", "二行目"]);
        assert_eq!(sanitized(&long, lines(&["第一行\r\n第二行"])), lines(&["第一行", "第二行"]));
        let choices = unit(ItemType::Array, &["はい", "いいえ"]);
        assert_eq!(sanitized(&choices, lines(&["是\n否"])), lines(&["是", "否"]));
    }

    #[test]
    fn speaker_label_check_accepts_controls_and_bracket_variants() {
        let source = |label: &str| unit(ItemType::LongText, &[label, "こんにちは。"]);
        for (source_label, translated_label) in [
            ("【ラージ】", "【拉吉】"),
            ("〖ラージ〗", "〖拉吉〗"),
            ("〔ラージ〕", "〔拉吉〕"),
            ("《ラージ》", "《拉吉》"),
            ("〈ラージ〉", "〈拉吉〉"),
            ("［ラージ］", "［拉吉］"),
            ("[ラージ]", "[拉吉]"),
        ] {
            let u = source(source_label);
            assert!(
                check_translation(&u, &lines(&[translated_label, "你好。"]), "ja", "zh", PreserveSet::core()).is_ok(),
                "source={source_label} translated={translated_label}"
            );
            // Dropping the label is structural damage regardless of its style.
            assert!(
                check_translation(&u, &lines(&["你好。", ""]), "ja", "zh", PreserveSet::core()).is_err(),
                "missing label for source={source_label}"
            );
        }
        let u = source("【ラージ】");
        let controlled_body = unit(ItemType::LongText, &["【ラージ】", r"\SE[magao]「こんにちは。」"]);
        assert!(
            check_translation(
                &controlled_body,
                &lines(&["【拉吉】", r"\SE[magao]「你好。」"]),
                "ja",
                "zh",
                PreserveSet::core()
            )
            .is_ok()
        );
        // The check reads raw slots, so a preserved control before the label
        // does not mask the label.
        let moved_control = lines(&[r"\SE[magao]【拉吉】", "「你好。」"]);
        let result = check_translation(&controlled_body, &moved_control, "ja", "zh", PreserveSet::core());
        assert!(result.is_ok(), "{moved_control:?}: {result:?}");
        let collapsed = lines(&["【拉吉】你好。"]);
        let result = check_translation(&u, &collapsed, "ja", "zh", PreserveSet::core());
        assert!(result.is_ok(), "{collapsed:?}: {result:?}");
        let controlled = unit(ItemType::LongText, &[r"\C[2]【ラージ】\C[0]", "こんにちは。"]);
        assert!(
            check_translation(
                &controlled,
                &lines(&[r"\C[2]【拉吉】\C[0]", "你好。"]),
                "ja",
                "zh",
                PreserveSet::core()
            )
            .is_ok()
        );
        assert!(check_translation(&u, &lines(&["【拉吉】"]), "ja", "zh", PreserveSet::core()).is_err());
        let plain = unit(ItemType::LongText, &["こんにちは。", "元気ですか。"]);
        assert!(check_translation(&plain, &lines(&["你好。", "你好吗。"]), "ja", "zh", PreserveSet::core()).is_ok());
    }

    #[test]
    fn preserved_only_source_lines_are_restored_verbatim() {
        let source = unit(ItemType::LongText, &[r"\C[1]", "こんにちは"]);
        let mut target = lines(&["", "你好"]);
        assert_eq!(normalize_lines(&source, &mut target, "zh", PreserveSet::core()), 1);
        assert_eq!(target, lines(&[r"\C[1]", "你好"]));
        // A genuine long-text reflow changes the line count; leave it alone.
        let mut reflowed = lines(&[r"\C[1]你好"]);
        assert_eq!(normalize_lines(&source, &mut reflowed, "zh", PreserveSet::core()), 0);
        assert_eq!(reflowed, lines(&[r"\C[1]你好"]));
    }

    #[test]
    fn rejects_visible_source_scripts_and_copies_but_not_protected_literals() {
        for (src_lang, src, dst) in [
            ("ja-JP", "こんにちは", "你好こんにちは"),
            ("ko_KR", "안녕", "你好안녕"),
            ("ru", "Привет", "你好Привет"),
            ("ar", "مرحبا", "你好مرحبا"),
            ("hi", "नमस्ते", "你好नमस्ते"),
            ("th", "สวัสดี", "你好สวัสดี"),
        ] {
            let source = unit(ItemType::ShortText, &[src]);
            assert!(check_translation(&source, &lines(&[dst]), src_lang, "zh-tw", PreserveSet::core()).is_err());
            assert!(check_translation(&source, &lines(&["你好"]), src_lang, "zh-tw", PreserveSet::core()).is_ok());
            assert!(check_translation(&source, &source.original_lines, src_lang, "zh-tw", PreserveSet::core()).is_err());
        }
        let english = unit(ItemType::ShortText, &["Hello"]);
        assert!(check_translation(&english, &english.original_lines, "EN_us", "zh", PreserveSet::core()).is_err());
        assert!(check_translation(&english, &english.original_lines, "en", "EN-gb", PreserveSet::core()).is_ok());
        let escaped_line = unit(ItemType::ShortText, &[r"\nHello"]);
        assert_eq!(visible_lines(&escaped_line.original_lines, PreserveSet::core()), lines(&[" Hello"]));
        assert!(check_translation(&escaped_line, &escaped_line.original_lines, "en", "zh", PreserveSet::core()).is_err());
        let placeholders = unit(ItemType::ShortText, &[r"{item} %s \SE[カナ]"]);
        assert!(check_translation(&placeholders, &placeholders.original_lines, "en", "zh", PreserveSet::core()).is_ok());
        let source = unit(ItemType::ShortText, &[r"こんにちは {a} {b} {c}"]);
        assert!(check_translation(&source, &lines(&["你好 {a} {b}"]), "ja", "zh", PreserveSet::core()).is_err());
        assert!(check_translation(&source, &lines(&["你好 {c} {a} {b}"]), "ja", "zh", PreserveSet::core()).is_ok());
    }

    #[test]
    fn independent_speaker_and_dialogue_cannot_be_omitted() {
        let source = unit(ItemType::LongText, &["【アレイ】", "こんにちは"]);
        for invalid in [lines(&["你好"]), lines(&["【艾蕾】"]), lines(&["【【艾蕾】你好"])] {
            assert!(check_translation(&source, &invalid, "ja", "zh", PreserveSet::core()).is_err());
        }
        assert!(check_translation(&source, &lines(&["【艾蕾】", "你好"]), "ja", "zh", PreserveSet::core()).is_ok());
        assert!(check_translation(&source, &lines(&["【艾蕾】你好"]), "ja", "zh", PreserveSet::core()).is_ok());
    }

    #[test]
    fn shared_kanji_ui_labels_are_not_forced_into_failed_translation() {
        for label in ["魔法", "通信", "【王】", "HP回復"] {
            let source = unit(ItemType::ShortText, &[label]);
            assert!(check_translation(&source, &source.original_lines, "ja", "zh-tw", PreserveSet::core()).is_ok());
        }
        let japanese = unit(ItemType::ShortText, &["今日は暑い"]);
        assert!(check_translation(&japanese, &japanese.original_lines, "ja", "zh", PreserveSet::core()).is_err());
    }
}
