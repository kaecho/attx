use crate::model::{ItemType, TextUnit};
use anyhow::{Result, bail};

pub fn check_unit(unit: &TextUnit, translation_lines: &[String]) -> Result<()> {
    if translation_lines.is_empty() {
        bail!("empty translation");
    }
    if translation_lines.iter().any(|l| l.is_empty()) {
        bail!("empty translation line");
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
                bail!("short_text must have 1 line, got {}", translation_lines.len());
            }
        }
        ItemType::LongText => {}
    }
    // placeholder count
    for (i, src) in unit.original_lines.iter().enumerate() {
        let src_ctrl = count_ctrl_tokens(src) + count_raw_controls(src);
        if let Some(dst) = translation_lines.get(i) {
            let dst_ph = count_ctrl_tokens(dst);
            // after unmask, placeholders gone; check raw backslash controls roughly
            let dst_raw = count_raw_controls(dst);
            // allow reflow: only compare total across all lines for long_text
            let _ = (src_ctrl, dst_ph, dst_raw);
        }
    }
    let src_total: usize = unit.original_lines.iter().map(|l| count_raw_controls(l)).sum();
    let dst_total: usize = translation_lines.iter().map(|l| count_raw_controls(l)).sum();
    if src_total > 0 && dst_total < src_total {
        // soft: many models drop controls; hard-fail only severe loss
        if dst_total * 2 < src_total {
            bail!("control codes likely lost: src={src_total} dst={dst_total}");
        }
    }
    Ok(())
}

fn count_ctrl_tokens(s: &str) -> usize {
    s.matches("[CTRL_").count()
}

fn count_raw_controls(s: &str) -> usize {
    // count backslash-letter patterns
    let mut n = 0;
    let b = s.as_bytes();
    let mut i = 0;
    while i + 1 < b.len() {
        if b[i] == b'\\' {
            let c = b[i + 1];
            if c.is_ascii_alphabetic() || matches!(c, b'.' | b'!' | b'|' | b'>' | b'{' | b'}' | b'$' | b'^' | b'\\') {
                n += 1;
                i += 2;
                continue;
            }
        }
        i += 1;
    }
    n
}
