//! Data-driven message layout rules.
//!
//! Slot-aware formats keep structural meaning in line zero: a speaker label,
//! a chapter marker, a sign title. Hard-coding one format's brackets makes the
//! next game rediscover the same problem. A rule is instead an experience entry
//! (`kind = "layout"`) with three pieces of data:
//!
//! * `label` regex: what a leading label looks like.
//! * `control` regex: atomic tokens that are not visible width.
//! * `width`: the reflow budget in half-width cells; `0` disables width reflow.
//!
//! Rules are layered by `knowledge` (builtin, global, workspace), so an agent
//! can add a rule for one game without touching Rust, and a workspace rule
//! overrides a global one.

use crate::knowledge::{Experience, LeadingControls};
use crate::model::TextUnit;
use anyhow::{Result, bail};
use regex::Regex;
use std::borrow::Cow;

#[derive(Clone, Debug)]
pub struct LayoutRule {
    label: Option<Regex>,
    control: Option<Regex>,
    pub width: usize,
    pub leading_controls: LeadingControls,
    pub domain: String,
    pub layer: u8,
    pub source: String,
}

#[derive(Clone, Debug, Default)]
pub struct LayoutRules {
    rules: Vec<LayoutRule>,
}

impl LayoutRules {
    pub fn none() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    pub fn from_experience(exp: &Experience) -> Self {
        let mut rules: Vec<LayoutRule> = exp
            .layout_entries()
            .filter_map(|(layer, entry)| match LayoutRule::compile(layer, entry) {
                Ok(rule) => Some(rule),
                Err(e) => {
                    eprintln!(
                        "layout: ignoring rule `{}` for {}: {e:#}",
                        entry.label, exp.format
                    );
                    None
                }
            })
            .collect();
        // Higher layers first, then a concrete domain before an unrestricted
        // rule, then the larger width. Stable sort keeps declaration order.
        rules.sort_by_key(|r| (std::cmp::Reverse(r.layer), r.domain.is_empty(), std::cmp::Reverse(r.width)));
        Self { rules }
    }

    pub fn for_unit<'a>(&'a self, unit: &TextUnit) -> impl Iterator<Item = &'a LayoutRule> {
        self.rules.iter().filter(move |r| r.applies_to(unit))
    }

    /// First rule whose label matches the start of `text`.
    pub fn find<'a>(&'a self, unit: &TextUnit, text: &str) -> Option<&'a LayoutRule> {
        self.for_unit(unit).find(|r| r.leading_label(text).is_some())
    }

    pub fn width_for(&self, unit: &TextUnit) -> Option<usize> {
        self.for_unit(unit).find(|r| r.width > 0).map(|r| r.width)
    }

    /// Width budget that applies to a domain, independent of a unit. Used by
    /// `learn layout test` so an agent sees the inherited builtin width.
    pub fn effective_width(&self, domain: &str) -> Option<usize> {
        self.rules
            .iter()
            .find(|r| {
                (domain.is_empty() || r.domain.is_empty() || r.domain == domain) && r.width > 0
            })
            .map(|r| r.width)
    }

    /// Resolve a raw line without a unit. Used by `learn layout test`.
    pub fn probe<'a>(&'a self, domain: &str, text: &'a str) -> Option<(&'a LayoutRule, LabelParts<'a>)> {
        self.rules
            .iter()
            .filter(|r| domain.is_empty() || r.domain.is_empty() || r.domain == domain)
            .find_map(|r| r.leading_label(text).map(|parts| (r, parts)))
    }
}

impl LayoutRule {
    fn compile(layer: u8, entry: &crate::knowledge::LayoutEntry) -> Result<Self> {
        let label = compile_optional(&entry.label, "label")?;
        let control = compile_optional(&entry.control, "control")?;
        Ok(Self {
            label,
            control,
            width: entry.width,
            leading_controls: entry.leading_controls,
            domain: entry.domain.clone(),
            layer,
            source: entry.source.clone(),
        })
    }

    pub fn applies_to(&self, unit: &TextUnit) -> bool {
        self.domain.is_empty() || self.domain == unit.domain
    }

    pub fn has_visible_text(&self, text: &str) -> bool {
        self.after_controls_and_space(text, 0) < text.len()
    }

    /// Whether `text` contains at least one control token recognized by this
    /// rule. Used to decide if a leading control belongs to the body without
    /// assuming one engine's escape syntax.
    pub fn contains_control(&self, text: &str) -> bool {
        let mut i = 0;
        while i < text.len() {
            if self.control_end(text, i).is_some() {
                return true;
            }
            i += text[i..].chars().next().unwrap().len_utf8();
        }
        false
    }

    pub fn control_end(&self, text: &str, i: usize) -> Option<usize> {
        let re = self.control.as_ref()?;
        let m = re.find(&text[i..])?;
        (m.start() == 0 && !m.is_empty()).then_some(i + m.end())
    }

    fn after_controls_and_space(&self, text: &str, mut i: usize) -> usize {
        while i < text.len() {
            if let Some(end) = self.control_end(text, i) {
                i = end;
                continue;
            }
            let ch = text[i..].chars().next().unwrap();
            if !ch.is_whitespace() {
                break;
            }
            i += ch.len_utf8();
        }
        i
    }

    /// Split a leading label from the rest of a line. The label match must
    /// start at the first visible character, so a leading colour/expression
    /// control is treated as a prefix, never as part of the label.
    pub fn leading_label<'a>(&self, text: &'a str) -> Option<LabelParts<'a>> {
        let re = self.label.as_ref()?;
        let start = self.after_controls_and_space(text, 0);
        let m = re.find(&text[start..])?;
        if m.start() != 0 || m.is_empty() {
            return None;
        }
        let end = start + m.end();
        let matched = &text[start..end];
        if !label_has_visible_text(matched) {
            return None;
        }
        Some(LabelParts {
            prefix: &text[..start],
            label: matched,
            suffix: &text[end..],
            end,
        })
    }

    /// A line whose only visible content is one label, optionally surrounded
    /// by controls and whitespace.
    pub fn standalone_label<'a>(&self, text: &'a str) -> Option<LabelParts<'a>> {
        let parts = self.leading_label(text)?;
        (!self.has_visible_text(parts.suffix)).then_some(parts)
    }
}

pub fn validate(label: &str, control: &str) -> Result<()> {
    compile_optional(label, "label")?;
    compile_optional(control, "control")?;
    Ok(())
}

fn compile_optional(pattern: &str, field: &str) -> Result<Option<Regex>> {
    if pattern.trim().is_empty() {
        return Ok(None);
    }
    let re = Regex::new(pattern).map_err(|e| anyhow::anyhow!("invalid {field} regex: {e}"))?;
    if re.is_match("") {
        bail!("{field} regex must not match an empty string");
    }
    Ok(Some(re))
}

/// `【  】` is a decorative bracket, not a label. Reject a match whose
/// delimiters contain only whitespace; a data-driven regex cannot express
/// "non-blank inner text" without making every rule unreadable.
fn label_has_visible_text(label: &str) -> bool {
    let mut chars = label.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    let Some(last) = label.chars().next_back() else {
        return false;
    };
    if first.is_alphanumeric() || last.is_alphanumeric() {
        return true;
    }
    let inner_len = label.chars().count().saturating_sub(2);
    let mut has_text = false;
    for ch in label.chars().skip(1).take(inner_len) {
        if ch == first || ch == last {
            return false;
        }
        if !ch.is_whitespace() {
            has_text = true;
        }
    }
    has_text
}

#[derive(Clone, Copy, Debug)]
pub struct LabelParts<'a> {
    /// Leading controls/whitespace that do not belong to the visible label.
    pub prefix: &'a str,
    /// Label text including its delimiters.
    pub label: &'a str,
    /// Everything after the label.
    pub suffix: &'a str,
    /// Byte offset immediately after the label.
    pub end: usize,
}

/// Changes to physical message slots, shared by caching and rendering.
#[derive(Default, Debug)]
pub struct LayoutChange {
    pub reflowed: bool,
    /// Slots wider than the rule's width budget. Overflow is retained, not
    /// truncated, when the available slots cannot fit it.
    pub overflow_lines: usize,
}

/// Fit a translation into `n` fixed slots using the first matching layout
/// rule. Returns the input unchanged when no rule applies.
pub fn fit<'a>(
    unit: &TextUnit,
    lines: &'a [String],
    n: usize,
    rules: &LayoutRules,
) -> (Cow<'a, [String]>, LayoutChange) {
    if n == 0 || lines.is_empty() || rules.is_empty() {
        return (Cow::Borrowed(lines), LayoutChange::default());
    }
    let width = rules.width_for(unit).unwrap_or(0);
    let source_rule = unit
        .original_lines
        .first()
        .and_then(|line| rules.find(unit, line));
    let translated_rule = lines.first().and_then(|line| rules.find(unit, line));
    let Some(rule) = source_rule.or(translated_rule).or_else(|| rules.for_unit(unit).next()) else {
        return (Cow::Borrowed(lines), LayoutChange::default());
    };
    let max_w = if width == 0 { usize::MAX } else { width };
    let fitted = fitted_lines(unit, lines, n, max_w, rule);
    let overflow_lines = if width == 0 {
        0
    } else {
        fitted.iter().filter(|line| display_width(line, rule) > width).count()
    };
    let reflowed = fitted.as_ref() != lines;
    (fitted, LayoutChange { reflowed, overflow_lines })
}

/// Mutate a cached translation when physical slots changed.
pub fn normalize(
    unit: &TextUnit,
    translation: &mut crate::model::Translation,
    n: usize,
    rules: &LayoutRules,
) -> LayoutChange {
    let (fitted, change) = fit(unit, &translation.translation_lines, n, rules);
    if change.reflowed {
        translation.translation_lines = fitted.into_owned();
    }
    change
}

fn fitted_lines<'a>(
    unit: &TextUnit,
    lines: &'a [String],
    n: usize,
    max_w: usize,
    rule: &LayoutRule,
) -> Cow<'a, [String]> {
    if n < 2 || lines.is_empty() {
        return fit_lines_borrowed(lines, n, max_w, rule);
    }
    let first = &lines[0];
    let source_label = unit
        .original_lines
        .first()
        .and_then(|line| rule.standalone_label(line).map(|parts| (line, parts)));
    let source_slot = source_label.map(|(line, _)| line);
    let source_prefix = source_label.map_or("", |(_, parts)| parts.prefix);
    // A label that already owns the first slot stays byte-for-byte intact.
    // A leading control moves back to the body when the source label slot did
    // not carry one, because plugins match the label at the very start.
    if let Some(parts) = rule.standalone_label(first) {
        if source_slot.is_some()
            && rule.leading_controls == LeadingControls::Body
            && !rule.contains_control(source_prefix)
            && rule.contains_control(parts.prefix)
        {
            let mut body = Vec::with_capacity(lines.len());
            let mut first_body = parts.prefix.to_owned();
            if let Some(rest) = lines.get(1) {
                first_body.push_str(rest);
            }
            body.push(first_body);
            body.extend_from_slice(lines.get(2..).unwrap_or_default());
            let fitted_body = fit_lines_borrowed(&body, n - 1, max_w, rule);
            let mut out = Vec::with_capacity(n);
            out.push(parts.label.to_owned());
            out.extend(fitted_body.into_owned());
            return Cow::Owned(out);
        }
        let body = fit_lines_borrowed(&lines[1..], n - 1, max_w, rule);
        if matches!(&body, Cow::Borrowed(_)) {
            return Cow::Borrowed(lines);
        }
        let mut out = Vec::with_capacity(n);
        out.push(first.clone());
        out.extend(body.into_owned());
        return Cow::Owned(out);
    }
    // The source reserved slot zero for a label. Recover the label from a
    // collapsed first line, or fall back to the source label when the model
    // omitted it, and reflow only the body slots.
    if let Some(source_slot) = source_slot {
        let mut body = Vec::with_capacity(lines.len());
        let label = if let Some(parts) = rule.leading_label(first) {
            let mut first_body = String::with_capacity(parts.prefix.len() + parts.suffix.len());
            first_body.push_str(parts.prefix);
            first_body.push_str(parts.suffix);
            if !first_body.is_empty() {
                body.push(first_body);
            }
            body.extend_from_slice(&lines[1..]);
            parts.label.to_owned()
        } else {
            body.extend_from_slice(lines);
            source_slot.clone()
        };
        let fitted_body = fit_lines_borrowed(&body, n - 1, max_w, rule);
        let mut out = Vec::with_capacity(n);
        out.push(label);
        out.extend(fitted_body.into_owned());
        return Cow::Owned(out);
    }
    fit_lines_borrowed(lines, n, max_w, rule)
}

/// Keep safe boundaries and reflow only the suffix needing more room.
/// The final slot may overflow; event command indices never move.
fn fit_lines_borrowed<'a>(
    lines: &'a [String],
    n: usize,
    max_w: usize,
    rule: &LayoutRule,
) -> Cow<'a, [String]> {
    if n == 0 {
        return Cow::Owned(Vec::new());
    }
    let mut reflow_from = None;
    for (i, line) in lines.iter().enumerate().take(n.saturating_sub(1)) {
        let too_wide = display_width(line, rule) > max_w;
        // A width-0 rule protects slot structure without imposing a line
        // budget, so orphan punctuation alone must not rewrite the lines.
        let orphaned = max_w != usize::MAX
            && lines
                .get(i + 1)
                .is_some_and(|next| starts_closing_punctuation(next, rule));
        if too_wide || orphaned {
            reflow_from = Some(i);
            break;
        }
    }
    if let Some(start) = reflow_from {
        let mut merged = String::with_capacity(lines[start..].iter().map(String::len).sum());
        for line in &lines[start..] {
            merged.push_str(line);
        }
        let mut out = Vec::with_capacity(n);
        out.extend_from_slice(&lines[..start]);
        out.extend(reflow_to_n(&merged, n - start, max_w, rule));
        return Cow::Owned(out);
    }
    if lines.len() == n {
        return Cow::Borrowed(lines);
    }
    let mut out = Vec::with_capacity(n);
    out.extend_from_slice(&lines[..lines.len().min(n - 1)]);
    if lines.len() >= n {
        let mut last = String::with_capacity(lines[n - 1..].iter().map(String::len).sum());
        for line in &lines[n - 1..] {
            last.push_str(line);
        }
        out.push(last);
    }
    out.resize(n, String::new());
    Cow::Owned(out)
}

fn display_width(s: &str, rule: &LayoutRule) -> usize {
    let mut w = 0usize;
    let mut i = 0;
    while i < s.len() {
        if let Some(end) = rule.control_end(s, i) {
            i = end;
            continue;
        }
        let ch = s[i..].chars().next().unwrap();
        w += if spawns_half_width(ch) { 1 } else { 2 };
        i += ch.len_utf8();
    }
    w
}

fn spawns_half_width(ch: char) -> bool {
    (ch as u32) < 128
}

fn is_closing_punctuation(ch: char) -> bool {
    matches!(
        ch,
        '，' | '。' | '！' | '？' | '、' | '」' | '』' | '；' | '：' | '…' | '〗' | '〕'
            | '》' | '〉' | '］' | '｝' | ',' | '.' | '!' | '?' | ';' | ':' | ')' | '）' | '】'
            | ']' | '}' | '”' | '’'
    )
}

fn is_opening_punctuation(ch: char) -> bool {
    matches!(
        ch,
        '「' | '『' | '（' | '(' | '【' | '〖' | '〔' | '《' | '〈' | '［' | '[' | '｛' | '{'
            | '“' | '‘' | '"' | '\''
    )
}

fn starts_closing_punctuation(text: &str, rule: &LayoutRule) -> bool {
    let mut i = 0;
    while i < text.len() {
        if let Some(end) = rule.control_end(text, i) {
            i = end;
            continue;
        }
        let ch = text[i..].chars().next().unwrap();
        if !ch.is_whitespace() {
            return is_closing_punctuation(ch);
        }
        i += ch.len_utf8();
    }
    false
}

struct TokenSpan {
    start: usize,
    end: usize,
    width: usize,
    visible: Option<char>,
    next_is_closing: bool,
}

fn tokenize_controls(text: &str, rule: &LayoutRule) -> Vec<TokenSpan> {
    let mut tokens = Vec::new();
    let mut start = 0;
    while start < text.len() {
        if let Some(end) = rule.control_end(text, start) {
            tokens.push(TokenSpan {
                start,
                end,
                width: 0,
                visible: None,
                next_is_closing: false,
            });
            start = end;
        } else {
            let ch = text[start..].chars().next().unwrap();
            let end = start + ch.len_utf8();
            tokens.push(TokenSpan {
                start,
                end,
                width: if spawns_half_width(ch) { 1 } else { 2 },
                visible: Some(ch),
                next_is_closing: false,
            });
            start = end;
        }
    }
    let mut next_is_closing = false;
    for token in tokens.iter_mut().rev() {
        token.next_is_closing = next_is_closing;
        if let Some(ch) = token.visible
            && !ch.is_whitespace()
        {
            next_is_closing = is_closing_punctuation(ch);
        }
    }
    tokens
}

fn reflow_to_n(text: &str, n: usize, max_w: usize, rule: &LayoutRule) -> Vec<String> {
    if n == 0 {
        return Vec::new();
    }
    let text = if text.contains('\n') {
        Cow::Owned(text.replace('\n', ""))
    } else {
        Cow::Borrowed(text)
    };
    if n == 1 {
        return vec![text.into_owned()];
    }
    // A leading label is one visible unit: line breaks may follow it but never
    // land inside it, even when the label alone exceeds the width budget.
    let label_end = rule.leading_label(&text).map_or(0, |parts| parts.end);
    let tokens = tokenize_controls(&text, rule);
    let mut remaining_width: usize = tokens.iter().map(|token| token.width).sum();
    let mut lines = Vec::with_capacity(n);
    let mut start = 0;
    while start < tokens.len() {
        if lines.len() + 1 == n {
            lines.push(text[tokens[start].start..].to_owned());
            break;
        }
        let mut width = 0;
        let mut latest_safe = None;
        let mut latest_any = None;
        let mut latest_space = None;
        let mut latest_punctuation = None;
        let mut latest_label = None;
        let mut end = start;
        while end < tokens.len()
            && (width + tokens[end].width <= max_w || tokens[end].start < label_end)
        {
            let token = &tokens[end];
            width += token.width;
            if let Some(ch) = token.visible {
                if token.end == label_end && token.end > 0 {
                    latest_label = Some((end + 1, width));
                }
                if token.end >= label_end && !token.next_is_closing {
                    latest_any = Some((end + 1, width));
                    let next = text[token.end..].chars().next();
                    let mid_word = ch.is_ascii_alphanumeric()
                        && next.is_some_and(|next| next.is_ascii_alphanumeric());
                    if !mid_word && !is_opening_punctuation(ch) {
                        latest_safe = Some((end + 1, width));
                        if ch.is_whitespace() {
                            latest_space = latest_safe;
                        }
                        if is_closing_punctuation(ch) {
                            latest_punctuation = latest_safe;
                        }
                    }
                }
            }
            end += 1;
        }
        if end == tokens.len() {
            lines.push(text[tokens[start].start..].to_owned());
            break;
        }
        let remaining_capacity = (n - lines.len() - 1).saturating_mul(max_w);
        let fits = |&(_, used_width): &(usize, usize)| {
            remaining_width.saturating_sub(used_width) <= remaining_capacity
                || remaining_width > (n - lines.len()).saturating_mul(max_w)
        };
        let preferred = latest_punctuation.filter(fits).or_else(|| latest_space.filter(fits));
        let Some((split, used_width)) =
            preferred.or(latest_safe).or(latest_any).or(latest_label)
        else {
            while lines.len() + 1 < n {
                lines.push(String::new());
            }
            lines.push(text[tokens[start].start..].to_owned());
            break;
        };
        lines.push(text[tokens[start].start..tokens[split - 1].end].to_owned());
        remaining_width = remaining_width.saturating_sub(used_width);
        start = split;
    }
    lines.resize(n, String::new());
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge::{LayoutEntry, Status};
    use crate::model::{ItemType, Translation};

    fn entry(label: &str, control: &str, width: usize) -> LayoutEntry {
        let mut e = LayoutEntry::new(label, control, width, LeadingControls::Body, "");
        e.status = Status::Approved;
        e.source = "test".into();
        e
    }

    fn rules(label: &str, control: &str, width: usize) -> LayoutRules {
        let mut exp = Experience {
            format: "rmmz".into(),
            entries: Vec::new(),
        };
        exp.entries.push((0, crate::knowledge::Entry::Layout(entry(label, control, width))));
        LayoutRules::from_experience(&exp)
    }

    fn builtin() -> LayoutRules {
        rules(
            r"^[【〖〔《〈［\[]([^】〗〕》〉］\]\r\n]+)[】〗〕》〉］\]]",
            r"\\{2}|\\[A-Za-z]+\[[^\]\r\n]*\]|\\[A-Za-z]",
            44,
        )
    }

    fn unit(source: &[&str], slots: usize) -> TextUnit {
        TextUnit {
            id: "layout-test".into(),
            engine: "rmmz".into(),
            domain: "dialogue".into(),
            location: "Map040.json/7/0/315".into(),
            item_type: ItemType::LongText,
            role: "旁白".into(),
            original_lines: source.iter().map(|line| (*line).to_owned()).collect(),
            source_line_paths: (0..slots).map(|i| format!("Map040.json/7/0/{i}")).collect(),
            context: String::new(),
            payload: String::new(),
        }
    }

    fn translation(lines: &[&str]) -> Translation {
        Translation {
            unit_id: "layout-test".into(),
            translation_lines: lines.iter().map(|line| (*line).to_owned()).collect(),
            source_hash: "hash".into(),
            passthrough: false,
        }
    }

    fn fit_lines(lines: &[String], n: usize, width: usize) -> Vec<String> {
        let rule = rules("", "", width);
        fit_lines_borrowed(lines, n, width, &rule.rules[0]).into_owned()
    }

    #[test]
    fn label_pairs_cover_common_speaker_markers() {
        let rules = builtin();
        let u = unit(&["【ラージ】", "こんにちは。"], 2);
        for text in [
            "【名字】",
            "〖名字〗",
            "〔名字〕",
            "《名字》",
            "〈名字〉",
            "［名字］",
            "[名字]",
        ] {
            let rule = rules.find(&u, text).unwrap();
            let parts = rule.standalone_label(text).unwrap();
            assert_eq!(parts.label, text);
        }
    }

    #[test]
    fn controls_and_whitespace_do_not_hide_a_label() {
        let rules = builtin();
        let u = unit(&["【ラージ】", "こんにちは。"], 2);
        let text = r" \C[2]【拉吉】\C[0] ";
        let rule = rules.find(&u, text).unwrap();
        let parts = rule.standalone_label(text).unwrap();
        assert_eq!(parts.prefix, " \\C[2]");
        assert_eq!(parts.label, "【拉吉】");
        assert!(parts.suffix.starts_with(r"\C[0]"));
    }

    #[test]
    fn label_with_body_is_not_standalone() {
        let rules = builtin();
        let u = unit(&["【ラージ】", "こんにちは。"], 2);
        let text = r"\SE[magao]【拉吉】「台词」";
        let rule = rules.find(&u, text).unwrap();
        let parts = rule.leading_label(text).unwrap();
        assert_eq!(parts.prefix, r"\SE[magao]");
        assert_eq!(parts.label, "【拉吉】");
        assert_eq!(parts.suffix, "「台词」");
        assert!(rule.standalone_label(text).is_none());
    }

    #[test]
    fn quotes_empty_labels_and_nested_brackets_are_rejected() {
        let rules = builtin();
        let u = unit(&["【ラージ】", "こんにちは。"], 2);
        for text in [r"「拉吉」", "【】", "【  】", "【a\nb】", "【a【b】"] {
            assert!(rules.find(&u, text).is_none(), "{text:?}");
        }
        assert!(rules.find(&u, "【拉吉").is_none());
        assert!(rules.find(&u, "普通文本").is_none());
    }

    #[test]
    fn agent_rule_can_protect_a_nonstandard_label_style() {
        let mut exp = Experience {
            format: "rmmz".into(),
            entries: Vec::new(),
        };
        let mut rule = LayoutEntry::new("^「[^」]+」", "", 0, LeadingControls::Body, "dialogue");
        rule.status = Status::Approved;
        rule.source = "learn:agent:layout".into();
        exp.entries.push((2, crate::knowledge::Entry::Layout(rule)));
        let rules = LayoutRules::from_experience(&exp);
        let u = unit(&["「ラージ」", "こんにちは。"], 2);
        let mut tr = translation(&["「拉吉」「你好。」"]);
        assert!(normalize(&u, &mut tr, 2, &rules).reflowed);
        assert_eq!(tr.translation_lines, vec!["「拉吉」", "「你好。」"]);

        let mut scroll = unit(&["「ラージ」", "こんにちは。"], 2);
        scroll.domain = "scroll".into();
        let mut tr = translation(&["「拉吉」「你好。」"]);
        assert!(!normalize(&scroll, &mut tr, 2, &rules).reflowed);
        assert_eq!(tr.translation_lines, vec!["「拉吉」「你好。」"]);
    }

    #[test]
    fn pending_rules_never_apply() {
        let mut exp = Experience {
            format: "rmmz".into(),
            entries: Vec::new(),
        };
        let mut rule = LayoutEntry::new("^「[^」]+」", "", 0, LeadingControls::Body, "dialogue");
        rule.status = Status::Pending;
        exp.entries.push((2, crate::knowledge::Entry::Layout(rule)));
        let rules = LayoutRules::from_experience(&exp);
        assert!(rules.is_empty());
    }

    #[test]
    fn invalid_regex_is_rejected_before_it_reaches_a_workspace() {
        assert!(validate("(", "").is_err());
        assert!(validate("", "").is_ok());
        assert!(validate("^$", "").is_err(), "empty-match regex is unsafe");
    }

    #[test]
    fn speaker_label_styles_and_controls_keep_slot_zero() {
        let rules = builtin();
        let body = "「这是一段超过消息窗口宽度的台词，用来确认写回只会重排正文槽。」";
        for (open, close) in [
            ('【', '】'),
            ('〖', '〗'),
            ('〔', '〕'),
            ('《', '》'),
            ('〈', '〉'),
            ('［', '］'),
            ('[', ']'),
        ] {
            let source_label = format!("{open}ラージ{close}");
            let translated_label = format!("{open}拉吉{close}");
            let u = unit(&[&source_label, "こんにちは。"], 2);
            let mut tr = translation(&[&format!("{translated_label}{body}")]);
            let change = normalize(&u, &mut tr, 2, &rules);
            assert!(change.reflowed, "{open}{close}");
            assert_eq!(tr.translation_lines[0], translated_label, "{open}{close}");
            assert_eq!(tr.translation_lines[1], body, "{open}{close}");
            assert!(!normalize(&u, &mut tr, 2, &rules).reflowed, "{open}{close}");
        }
    }

    #[test]
    fn control_wrapped_label_is_recovered_and_never_split() {
        let rules = builtin();
        let u = unit(&[r"\C[2]【ラージ】\C[0]", "こんにちは。"], 2);
        let mut tr = translation(&[r"\C[2]【拉吉】\C[0]「你好，这是一段很长的正文。」"]);
        assert!(normalize(&u, &mut tr, 2, &rules).reflowed);
        assert_eq!(tr.translation_lines[0], "【拉吉】");
        assert_eq!(tr.translation_lines[1], r"\C[2]\C[0]「你好，这是一段很长的正文。」");
        assert!(!normalize(&u, &mut tr, 2, &rules).reflowed);

        let plain = unit(&["【ラージ】", r"\SE[magao]「こんにちは。」"], 2);
        let mut moved = translation(&[r"\SE[magao]【拉吉】", "「你好。」"]);
        assert!(normalize(&plain, &mut moved, 2, &rules).reflowed);
        assert_eq!(moved.translation_lines[0], "【拉吉】");
        assert_eq!(moved.translation_lines[1], r"\SE[magao]「你好。」");
    }

    #[test]
    fn embedded_label_is_atomic_when_source_has_no_name_slot() {
        let rules = builtin();
        let u = unit(
            &["【ラージ】「とても長い台詞がここに続きます。」", "二行目。"],
            2,
        );
        let mut tr = translation(&[
            "【拉吉】「这是一段非常长的台词需要重新排版而不能把名字标签拆开。」",
            "第二行。",
        ]);
        assert!(normalize(&u, &mut tr, 2, &rules).reflowed);
        let out = &tr.translation_lines;
        assert_eq!(out.len(), 2);
        assert!(out[0].starts_with("【拉吉】"), "out={out:?}");
        assert_eq!(
            out.iter().filter(|line| line.contains('【') || line.contains('】')).count(),
            1,
            "out={out:?}"
        );
        assert_eq!(
            out.join(""),
            "【拉吉】「这是一段非常长的台词需要重新排版而不能把名字标签拆开。」第二行。"
        );
    }

    #[test]
    fn reflow_prefers_word_boundaries_and_opening_punctuation_is_never_orphaned() {
        let rule = rules("", "", 12);
        let out = reflow_to_n("alpha beta gamma delta", 2, 12, &rule.rules[0]);
        assert_eq!(out, vec!["alpha beta ", "gamma delta"]);
        let out = reflow_to_n("甲乙「丙丁戊己", 2, 4, &rule.rules[0]);
        assert_eq!(out, vec!["甲乙", "「丙丁戊己"]);
        assert_eq!(out.join(""), "甲乙「丙丁戊己");
    }

    #[test]
    fn fit_lines_reflows_long_cjk_into_slots() {
        let long = "上午的课就先到这里。下午会进行结合实践的课程，请大家到训练场集合。".to_string();
        let out = fit_lines(std::slice::from_ref(&long), 3, 44);
        assert_eq!(out.len(), 3);
        assert!(out.iter().filter(|l| !l.is_empty()).count() >= 2, "out={out:?}");
        assert_eq!(out.join(""), long.replace('\n', ""));
    }

    #[test]
    fn fit_lines_keeps_equal_width_safe_lines() {
        let lines = vec!["短い一行。".into(), "もう一行。".into(), "三行目。".into()];
        assert_eq!(fit_lines(&lines, 3, 44), lines);
    }

    #[test]
    fn fit_lines_does_not_split_control_codes() {
        let rules = builtin();
        let s = "\\C[27]你好世界，这是一段比较长的测试文本用来检查控制符。".to_string();
        let out = fit_lines_borrowed(
            &[s],
            2,
            20,
            &rules.rules[0],
        )
        .into_owned();
        assert!(out.iter().any(|l| l.contains("\\C[27]")), "out={out:?}");
        assert!(!out.iter().any(|l| l.contains("\\C[2") && !l.contains("\\C[27]")));
    }

    #[test]
    fn latest_punctuation_break_wins_over_punctuation_type_order() {
        let rule = rules("", "", 12);
        let text = "甲。乙，丙丁戊己";
        let out = reflow_to_n(text, 2, 12, &rule.rules[0]);
        assert_eq!(out, vec!["甲。乙，", "丙丁戊己"]);
        assert_eq!(out.join(""), text);
    }

    #[test]
    fn closing_punctuation_never_starts_a_reflowed_line() {
        let rule = rules("", "", 0);
        for punctuation in ['，', '。', '！', '？', '、', '」', '』'] {
            let text = format!("甲乙{punctuation}丙丁");
            let out = reflow_to_n(&text, 2, 4, &rule.rules[0]);
            assert_eq!(out.join(""), text);
            assert_eq!(out[0], "甲");
            assert!(!starts_closing_punctuation(&out[1], &rule.rules[0]));
        }
        let out = reflow_to_n("甲。！？乙", 2, 2, &rule.rules[0]);
        assert_eq!(out, vec!["", "甲。！？乙"]);
    }

    #[test]
    fn early_punctuation_does_not_force_avoidable_last_slot_overflow() {
        let rule = rules("", "", 12);
        let text = "甲。乙丙丁戊己庚辛壬";
        let out = reflow_to_n(text, 2, 12, &rule.rules[0]);
        assert_eq!(out.join(""), text);
        assert!(out.iter().all(|line| display_width(line, &rule.rules[0]) <= 12), "out={out:?}");
        assert_eq!(out[0], "甲。乙丙丁戊");
    }

    #[test]
    fn name_is_not_reused_when_body_exceeds_multiple_available_slots() {
        let rules = builtin();
        let u = unit(&["【ラージ】", "一行。", "二行。", "三行。"], 4);
        let text = r"\SE[magao]甲乙丙丁戊己庚辛壬癸".repeat(12);
        let mut tr = translation(&["【拉吉】", &text]);
        let first = normalize(&u, &mut tr, 4, &rules);
        assert!(first.reflowed);
        assert_eq!(first.overflow_lines, 1);
        assert_eq!(tr.translation_lines[0], "【拉吉】");
        assert!(tr.translation_lines[1].starts_with(r"\SE[magao]"));
        assert_eq!(tr.translation_lines[1..].join(""), text);
        let fitted = tr.translation_lines.clone();
        let second = normalize(&u, &mut tr, 4, &rules);
        assert!(!second.reflowed);
        assert_eq!(second.overflow_lines, 1);
        assert_eq!(tr.translation_lines, fitted);
    }
}
