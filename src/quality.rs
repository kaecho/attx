use crate::model::{ItemType, TextUnit};
use anyhow::{Result, bail};

pub fn check_unit(unit: &TextUnit, translation_lines: &[String]) -> Result<()> {
    if translation_lines.is_empty() {
        bail!("empty translation");
    }
    // Allow blank lines (model often emits "" for empty 401 slots / line breaks).
    // Only reject if every line is empty/whitespace.
    if translation_lines.iter().all(|l| l.trim().is_empty()) {
        bail!("empty translation");
    }
    match unit.item_type {
        ItemType::Array => {
            if translation_lines.len() != unit.original_lines.len() {
                bail!(
                    "array line_count mismatch: got {} want {}",
                    translation_lines.len(),
                    unit.original_lines.len()
                );
            }
        }
        ItemType::ShortText => {
            if translation_lines.len() != 1 {
                bail!(
                    "short_text must have 1 line, got {}",
                    translation_lines.len()
                );
            }
        }
        ItemType::LongText => {}
    }
    let src_total: usize = unit
        .original_lines
        .iter()
        .map(|l| count_raw_controls(l))
        .sum();
    let dst_total: usize = translation_lines
        .iter()
        .map(|l| count_raw_controls(l))
        .sum();
    if src_total > 0 && dst_total < src_total && dst_total * 2 < src_total {
        bail!("control codes likely lost: src={src_total} dst={dst_total}");
    }
    Ok(())
}

/// Soft-fix common model issues so full runs can continue.
pub fn sanitize_lines(unit: &TextUnit, lines: Vec<String>) -> Vec<String> {
    let mut lines = lines;
    if unit.item_type == ItemType::ShortText {
        if lines.len() != 1 {
            lines = vec![lines.join("")];
        }
        if lines[0].trim().is_empty() {
            // fall back to original rather than failing the batch
            lines = unit.original_lines.clone();
        }
        return lines;
    }
    if unit.item_type == ItemType::Array {
        if lines.len() < unit.original_lines.len() {
            lines.resize(unit.original_lines.len(), String::new());
        } else if lines.len() > unit.original_lines.len() {
            let n = unit.original_lines.len();
            let mut out = lines[..n.saturating_sub(1)].to_vec();
            if n > 0 {
                out.push(lines[n.saturating_sub(1)..].join(""));
            }
            lines = out;
        }
        for (i, l) in lines.iter_mut().enumerate() {
            if l.trim().is_empty()
                && let Some(src) = unit.original_lines.get(i)
            {
                *l = src.clone();
            }
        }
        return lines;
    }
    // long_text: drop pure-empty trailing lines if we have at least one non-empty
    if lines.iter().any(|l| !l.trim().is_empty()) {
        while lines.len() > 1 && lines.last().is_some_and(|l| l.trim().is_empty()) {
            lines.pop();
        }
        // replace internal empty with single space so writeback keeps a slot
        for l in &mut lines {
            if l.is_empty() {
                *l = " ".to_string();
            }
        }
    }
    lines
}

fn count_raw_controls(s: &str) -> usize {
    // count backslash-letter patterns
    let mut n = 0;
    let b = s.as_bytes();
    let mut i = 0;
    while i + 1 < b.len() {
        if b[i] == b'\\' {
            let c = b[i + 1];
            if c.is_ascii_alphabetic()
                || matches!(
                    c,
                    b'.' | b'!' | b'|' | b'>' | b'{' | b'}' | b'$' | b'^' | b'\\'
                )
            {
                n += 1;
                i += 2;
                continue;
            }
        }
        i += 1;
    }
    n
}
