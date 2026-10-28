//! Message layout primitives shared by adapters and structural checks.
//!
//! RPG Maker control codes and a leading bracketed speaker label are atomic at
//! writeback: a line break may not land inside `\C[2]` or `【名字】`, and a
//! label that occupied its own source slot keeps that slot.

/// End offset of an RPG Maker control code starting at `i`, or `None` when
/// `s[i]` is not a backslash or the escape has no body.
pub(crate) fn control_end(s: &str, i: usize) -> Option<usize> {
    let b = s.as_bytes();
    if i >= b.len() || b[i] != b'\\' {
        return None;
    }
    let rest = &s[i + 1..];
    let first = rest.chars().next()?;
    // Short codes: \. \| \! \> \< \^ \\ \{ \} \$
    if matches!(
        first,
        '.' | '|' | '!' | '>' | '<' | '^' | '\\' | '{' | '}' | '$'
    ) {
        return Some(i + 1 + first.len_utf8());
    }
    // Letter + optional [args]: \C[n] \I[n] \N[n] \V[n] \SE[name] ...
    let mut j = i + 1;
    while j < b.len() && b[j].is_ascii_alphabetic() {
        j += 1;
    }
    if j < b.len() && b[j] == b'[' && let Some(close) = s[j + 1..].find(']') {
        return Some(j + 1 + close + 1);
    }
    (j > i + 1).then_some(j)
}

/// Byte offset after leading whitespace and control tokens.
fn after_controls_and_space(text: &str) -> usize {
    let mut i = 0;
    while i < text.len() {
        let ch = text[i..].chars().next().unwrap();
        if ch.is_whitespace() {
            i += ch.len_utf8();
        } else if let Some(end) = control_end(text, i) {
            i = end;
        } else {
            break;
        }
    }
    i
}

/// Closing bracket for a speaker-label opening bracket. Quotes are deliberately
/// absent: `「」` and `『』` delimit dialogue, not names.
fn closing_bracket(open: char) -> Option<char> {
    Some(match open {
        '【' => '】',
        '〖' => '〗',
        '〔' => '〕',
        '《' => '》',
        '〈' => '〉',
        '［' => '］',
        '[' => ']',
        _ => return None,
    })
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct LabelParts<'a> {
    /// Leading controls/whitespace that do not belong to the visible label.
    pub prefix: &'a str,
    /// Bracketed label including its delimiters.
    pub label: &'a str,
    /// Everything after the closing bracket.
    pub suffix: &'a str,
    /// Byte offset immediately after the closing bracket.
    pub end: usize,
}

/// Split a leading bracketed speaker label from the rest of a line. The label
/// must be non-empty and may not contain a newline or its own delimiters.
pub(crate) fn leading_label(text: &str) -> Option<LabelParts<'_>> {
    let start = after_controls_and_space(text);
    let rest = &text[start..];
    let open = rest.chars().next()?;
    let close = closing_bracket(open)?;
    let tail = &rest[open.len_utf8()..];
    let close_at = tail.find(close)?;
    let inner = &tail[..close_at];
    if inner.trim().is_empty() || inner.contains(['\n', '\r', open, close]) {
        return None;
    }
    let end = start + open.len_utf8() + close_at + close.len_utf8();
    Some(LabelParts {
        prefix: &text[..start],
        label: &text[start..end],
        suffix: &text[end..],
        end,
    })
}

/// A line whose only visible content is one bracketed label, optionally
/// surrounded by controls and whitespace.
pub(crate) fn standalone_label(text: &str) -> Option<LabelParts<'_>> {
    let parts = leading_label(text)?;
    (!has_visible_text(parts.suffix)).then_some(parts)
}

/// Whether the line contains anything visible after controls and whitespace.
pub(crate) fn has_visible_text(text: &str) -> bool {
    let mut i = 0;
    while i < text.len() {
        if let Some(end) = control_end(text, i) {
            i = end;
            continue;
        }
        let ch = text[i..].chars().next().unwrap();
        if !ch.is_whitespace() {
            return true;
        }
        i += ch.len_utf8();
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_pairs_cover_common_speaker_markers() {
        for (open, close) in [
            ('【', '】'),
            ('〖', '〗'),
            ('〔', '〕'),
            ('《', '》'),
            ('〈', '〉'),
            ('［', '］'),
            ('[', ']'),
        ] {
            let text = format!("{open}名字{close}");
            let parts = standalone_label(&text).unwrap();
            assert_eq!(parts.label, text);
            assert!(!has_visible_text(parts.suffix));
            assert_eq!(parts.end, text.len());
        }
    }

    #[test]
    fn controls_and_whitespace_do_not_hide_a_label() {
        let text = r" \C[2]【拉吉】\C[0] ";
        let parts = standalone_label(text).unwrap();
        assert_eq!(parts.prefix, " \\C[2]");
        assert_eq!(parts.label, "【拉吉】");
        assert!(parts.suffix.starts_with(r"\C[0]"));
    }

    #[test]
    fn label_with_body_is_not_standalone() {
        let parts = leading_label(r"\SE[magao]【拉吉】「台词」").unwrap();
        assert_eq!(parts.prefix, r"\SE[magao]");
        assert_eq!(parts.label, "【拉吉】");
        assert_eq!(parts.suffix, "「台词」");
        assert!(standalone_label(r"\SE[magao]【拉吉】「台词」").is_none());
    }

    #[test]
    fn quotes_empty_labels_and_nested_brackets_are_rejected() {
        for text in [r"「拉吉」", "【】", "【  】", "【a\nb】", "【a【b】"] {
            assert!(leading_label(text).is_none(), "{text:?}");
        }
        assert!(leading_label("【拉吉").is_none());
        assert!(leading_label("普通文本").is_none());
    }

    #[test]
    fn control_tokens_are_atomic_and_do_not_swallow_text() {
        assert_eq!(control_end(r"\C[2]x", 0), Some(5));
        assert_eq!(control_end(r"\SE[magao]x", 0), Some(10));
        assert_eq!(control_end(r"\.x", 0), Some(2));
        assert_eq!(control_end(r"\x", 0), Some(2));
        assert_eq!(control_end("x", 0), None);
        assert_eq!(control_end(r"\", 0), None);
    }
}
