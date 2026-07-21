//! Minimal lossless XML/XHTML tree for the epub and docx adapters.
//!
//! Untouched regions round-trip byte-for-byte: raw source slices are stored
//! verbatim and re-emitted on serialize. Only text nodes we *replace* are
//! re-escaped. This deliberately avoids a full XML dependency — EPUB XHTML and
//! OOXML are well-formed, so a strict tokenizer is enough.

use anyhow::{Result, bail};

#[derive(Debug, Clone)]
pub enum Node {
    /// `<tag attr="…">…</tag>` — raw_open/raw_close hold the exact source tags.
    Elem {
        name: String,
        raw_open: String,
        children: Vec<Node>,
        raw_close: String,
    },
    /// `<tag/>` or an HTML void element — verbatim.
    SelfClosing { name: String, raw: String },
    /// Character data, still escaped exactly as in the source.
    Text(String),
    /// Comment / CDATA / doctype / declaration / PI — verbatim passthrough.
    Misc(String),
}

/// Elements that are self-closing in HTML even without a trailing slash.
const HTML_VOID: &[&str] = &[
    "br", "img", "hr", "meta", "link", "input", "wbr", "source", "col", "area", "base", "embed",
    "track",
];

pub fn parse(src: &str) -> Result<Vec<Node>> {
    let bytes = src.as_bytes();
    let mut pos = 0usize;
    // Stack of open elements; the top's children receive new nodes.
    let mut stack: Vec<(String, String, Vec<Node>)> = Vec::new(); // (name, raw_open, children)
    let mut roots: Vec<Node> = Vec::new();

    let push =
        |stack: &mut Vec<(String, String, Vec<Node>)>, roots: &mut Vec<Node>, n: Node| match stack
            .last_mut()
        {
            Some((_, _, children)) => children.push(n),
            None => roots.push(n),
        };

    while pos < bytes.len() {
        if bytes[pos] != b'<' {
            let start = pos;
            while pos < bytes.len() && bytes[pos] != b'<' {
                pos += 1;
            }
            push(
                &mut stack,
                &mut roots,
                Node::Text(src[start..pos].to_string()),
            );
            continue;
        }
        let rest = &src[pos..];
        if let Some(end) = misc_end(rest) {
            push(&mut stack, &mut roots, Node::Misc(rest[..end].to_string()));
            pos += end;
            continue;
        }
        if rest.starts_with("</") {
            let Some(gt) = find_gt(bytes, pos) else {
                bail!("unclosed end tag at byte {pos}");
            };
            let raw_close = src[pos..=gt].to_string();
            let name = tag_name(&raw_close[2..]);
            // Close the nearest matching open element; tolerate stray closers.
            if let Some(open_idx) = stack.iter().rposition(|(n, _, _)| *n == name) {
                // Anything opened after it is implicitly closed (malformed input).
                while stack.len() > open_idx + 1 {
                    let (n, raw_open, children) = stack.pop().unwrap();
                    let node = Node::Elem {
                        name: n,
                        raw_open,
                        children,
                        raw_close: String::new(),
                    };
                    push(&mut stack, &mut roots, node);
                }
                let (n, raw_open, children) = stack.pop().unwrap();
                let node = Node::Elem {
                    name: n,
                    raw_open,
                    children,
                    raw_close,
                };
                push(&mut stack, &mut roots, node);
            } else {
                push(&mut stack, &mut roots, Node::Misc(raw_close));
            }
            pos = gt + 1;
            continue;
        }
        // Start tag.
        let Some(gt) = find_gt(bytes, pos) else {
            bail!("unclosed start tag at byte {pos}");
        };
        let raw = src[pos..=gt].to_string();
        let inner = &raw[1..raw.len() - 1];
        let self_closing = inner.ends_with('/');
        let name = tag_name(inner);
        if self_closing || HTML_VOID.iter().any(|v| v.eq_ignore_ascii_case(&name)) {
            push(&mut stack, &mut roots, Node::SelfClosing { name, raw });
        } else {
            stack.push((name, raw, Vec::new()));
        }
        pos = gt + 1;
    }

    // Close any elements left open (malformed input): emit without close tag.
    while let Some((n, raw_open, children)) = stack.pop() {
        let node = Node::Elem {
            name: n,
            raw_open,
            children,
            raw_close: String::new(),
        };
        match stack.last_mut() {
            Some((_, _, siblings)) => siblings.push(node),
            None => roots.push(node),
        }
    }
    Ok(roots)
}

pub fn serialize(nodes: &[Node], out: &mut String) {
    for n in nodes {
        match n {
            Node::Elem {
                raw_open,
                children,
                raw_close,
                ..
            } => {
                out.push_str(raw_open);
                serialize(children, out);
                out.push_str(raw_close);
            }
            Node::SelfClosing { raw, .. } => out.push_str(raw),
            Node::Text(t) | Node::Misc(t) => out.push_str(t),
        }
    }
}

/// Length of a comment / CDATA / doctype / PI starting at `rest`, if any.
fn misc_end(rest: &str) -> Option<usize> {
    for (open, close) in [
        ("<!--", "-->"),
        ("<![CDATA[", "]]>"),
        ("<?", "?>"),
        ("<!", ">"),
    ] {
        if let Some(rest) = rest.strip_prefix(open) {
            let end = rest.find(close)?;
            return Some(open.len() + end + close.len());
        }
    }
    None
}

/// Find the `>` ending the tag at `pos`, skipping quoted attribute values.
fn find_gt(bytes: &[u8], pos: usize) -> Option<usize> {
    let mut i = pos;
    let mut quote: Option<u8> = None;
    while i < bytes.len() {
        match (quote, bytes[i]) {
            (Some(q), c) if c == q => quote = None,
            (None, b'"') | (None, b'\'') => quote = Some(bytes[i]),
            (None, b'>') => return Some(i),
            _ => {}
        }
        i += 1;
    }
    None
}

fn tag_name(inner: &str) -> String {
    inner
        .trim_start()
        .chars()
        .take_while(|c| !c.is_whitespace() && *c != '/' && *c != '>')
        .collect()
}

/// Decode the five predefined entities plus numeric refs and `&nbsp;`.
/// Unknown entities pass through unchanged.
pub fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let tail = &rest[amp..];
        let Some(semi) = tail.find(';').filter(|&i| i <= 12) else {
            out.push('&');
            rest = &rest[amp + 1..];
            continue;
        };
        let ent = &tail[1..semi];
        let decoded = match ent {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some('\u{00A0}'),
            _ if ent.starts_with("#x") || ent.starts_with("#X") => {
                u32::from_str_radix(&ent[2..], 16)
                    .ok()
                    .and_then(char::from_u32)
            }
            _ if ent.starts_with('#') => ent[1..].parse::<u32>().ok().and_then(char::from_u32),
            _ => None,
        };
        match decoded {
            Some(c) => out.push(c),
            None => out.push_str(&tail[..=semi]),
        }
        rest = &rest[amp + semi + 1..];
    }
    out.push_str(rest);
    out
}

/// Escape text for insertion as XML character data.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
    out
}

/// Address of a node inside a tree: child indices from the root list.
pub type NodePath = Vec<usize>;

pub fn node_at_mut<'a>(nodes: &'a mut [Node], path: &[usize]) -> Option<&'a mut Node> {
    let (&first, rest) = path.split_first()?;
    let mut cur = nodes.get_mut(first)?;
    for &idx in rest {
        match cur {
            Node::Elem { children, .. } => cur = children.get_mut(idx)?,
            _ => return None,
        }
    }
    Some(cur)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = r#"<?xml version="1.0"?><!DOCTYPE html><html><head><title>T</title></head>
<body><p class="a">こんにちは<ruby>直哉<rt>なおや</rt></ruby>だ。<br/>次の行</p><!-- c --><p>A &amp; B</p></body></html>"#;

    #[test]
    fn roundtrip_lossless() {
        let tree = parse(DOC).unwrap();
        let mut out = String::new();
        serialize(&tree, &mut out);
        assert_eq!(out, DOC);
    }

    #[test]
    fn escape_unescape() {
        assert_eq!(
            unescape("A &amp; B &#x41; &#66; &unknown;"),
            "A & B A B &unknown;"
        );
        assert_eq!(escape("a<b>&c"), "a&lt;b&gt;&amp;c");
    }

    #[test]
    fn tolerates_stray_close() {
        let tree = parse("<div><p>x</div>").unwrap();
        let mut out = String::new();
        serialize(&tree, &mut out);
        assert_eq!(out, "<div><p>x</div>");
    }
}
