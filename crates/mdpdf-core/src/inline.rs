//! Inline parser: code spans, escapes, links, images, autolinks, emphasis.
//!
//! Emphasis follows the CommonMark delimiter-run algorithm (flanking rules,
//! "rule of 3"); `~~` strikethrough uses the same machinery.

use crate::ast::{plain_text, Inline};
use std::collections::HashMap;

pub type Refs = HashMap<String, String>;

enum Tok {
    N(Inline),
    D { ch: char, n: usize, open: bool, close: bool },
}

pub fn normalize_label(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

pub fn parse_inlines(s: &str, refs: &Refs) -> Vec<Inline> {
    let chars: Vec<char> = s.chars().collect();
    inline(&chars, refs)
}

fn is_punct(c: char) -> bool {
    c.is_ascii_punctuation() || matches!(c, '\u{a1}'..='\u{bf}' | '\u{2010}'..='\u{2027}' | '\u{3000}'..='\u{303f}')
}

fn flush(buf: &mut String, toks: &mut Vec<Tok>) {
    if !buf.is_empty() {
        toks.push(Tok::N(Inline::Text(std::mem::take(buf))));
    }
}

fn inline(c: &[char], refs: &Refs) -> Vec<Inline> {
    let mut toks: Vec<Tok> = Vec::new();
    let mut buf = String::new();
    let mut i = 0;
    while i < c.len() {
        let ch = c[i];
        match ch {
            '\\' => {
                match c.get(i + 1) {
                    Some(&n) if n.is_ascii_punctuation() => {
                        buf.push(n);
                        i += 2;
                    }
                    Some('\n') => {
                        flush(&mut buf, &mut toks);
                        toks.push(Tok::N(Inline::HardBreak));
                        i += 2;
                        while i < c.len() && c[i] == ' ' {
                            i += 1;
                        }
                    }
                    _ => {
                        buf.push('\\');
                        i += 1;
                    }
                }
            }
            '`' => {
                let n = run_len(c, i, '`');
                match find_closing_ticks(c, i + n, n) {
                    Some(end) => {
                        flush(&mut buf, &mut toks);
                        let raw: String = c[i + n..end].iter().collect::<String>().replace('\n', " ");
                        let code = if raw.len() > 2
                            && raw.starts_with(' ')
                            && raw.ends_with(' ')
                            && raw.trim() != ""
                        {
                            raw[1..raw.len() - 1].to_string()
                        } else {
                            raw
                        };
                        toks.push(Tok::N(Inline::Code(code)));
                        i = end + n;
                    }
                    None => {
                        for _ in 0..n {
                            buf.push('`');
                        }
                        i += n;
                    }
                }
            }
            '\n' => {
                let trailing = buf.chars().rev().take_while(|&x| x == ' ').count();
                let hard = trailing >= 2;
                let keep = buf.len() - trailing;
                buf.truncate(keep);
                flush(&mut buf, &mut toks);
                toks.push(Tok::N(if hard { Inline::HardBreak } else { Inline::SoftBreak }));
                i += 1;
                while i < c.len() && c[i] == ' ' {
                    i += 1;
                }
            }
            '*' | '_' | '~' => {
                let n = run_len(c, i, ch);
                if ch == '~' && n != 2 {
                    for _ in 0..n {
                        buf.push('~');
                    }
                    i += n;
                    continue;
                }
                let prev = if i == 0 { ' ' } else { c[i - 1] };
                let next = c.get(i + n).copied().unwrap_or(' ');
                let left = !next.is_whitespace() && (!is_punct(next) || prev.is_whitespace() || is_punct(prev));
                let right = !prev.is_whitespace() && (!is_punct(prev) || next.is_whitespace() || is_punct(next));
                let (open, close) = if ch == '_' {
                    (left && (!right || is_punct(prev)), right && (!left || is_punct(next)))
                } else {
                    (left, right)
                };
                flush(&mut buf, &mut toks);
                toks.push(Tok::D { ch, n, open, close });
                i += n;
            }
            '[' | '!' => {
                let is_img = ch == '!';
                if is_img && c.get(i + 1) != Some(&'[') {
                    buf.push('!');
                    i += 1;
                    continue;
                }
                let open = if is_img { i + 1 } else { i };
                match try_link(c, open, refs) {
                    Some((url, label, end)) => {
                        flush(&mut buf, &mut toks);
                        let content = inline(label, refs);
                        toks.push(Tok::N(if is_img {
                            Inline::Image { url, alt: plain_text(&content) }
                        } else {
                            Inline::Link { url, content }
                        }));
                        i = end;
                    }
                    None => {
                        buf.push(ch);
                        i += 1;
                    }
                }
            }
            '<' => {
                if let Some(end) = c[i..].iter().position(|&x| x == '>').map(|p| i + p) {
                    let inner: String = c[i + 1..end].iter().collect();
                    let low = inner.to_ascii_lowercase();
                    if low == "br" || low == "br/" || low == "br /" {
                        flush(&mut buf, &mut toks);
                        toks.push(Tok::N(Inline::HardBreak));
                        i = end + 1;
                        continue;
                    }
                    if is_autolink(&inner) {
                        flush(&mut buf, &mut toks);
                        toks.push(Tok::N(Inline::Link {
                            url: inner.clone(),
                            content: vec![Inline::Text(inner)],
                        }));
                        i = end + 1;
                        continue;
                    }
                }
                buf.push('<');
                i += 1;
            }
            '&' => {
                if let Some((s, len)) = entity(&c[i..]) {
                    buf.push_str(&s);
                    i += len;
                } else {
                    buf.push('&');
                    i += 1;
                }
            }
            'h' | 'w' => {
                let boundary = i == 0 || !c[i - 1].is_alphanumeric();
                if boundary && starts_with(c, i, "http://") || boundary && starts_with(c, i, "https://") || boundary && starts_with(c, i, "www.") {
                    let mut end = i;
                    while end < c.len() && !c[end].is_whitespace() && c[end] != '<' {
                        end += 1;
                    }
                    while end > i {
                        let l = c[end - 1];
                        let unmatched_paren = l == ')' && c[i..end].iter().filter(|&&x| x == '(').count() < c[i..end].iter().filter(|&&x| x == ')').count();
                        if matches!(l, '.' | ',' | ':' | ';' | '!' | '?' | '\'' | '"' | '*' | '_' | '~') || unmatched_paren {
                            end -= 1;
                        } else {
                            break;
                        }
                    }
                    let text: String = c[i..end].iter().collect();
                    if text.len() > 8 {
                        flush(&mut buf, &mut toks);
                        let url = if text.starts_with("www.") { format!("http://{text}") } else { text.clone() };
                        toks.push(Tok::N(Inline::Link { url, content: vec![Inline::Text(text)] }));
                        i = end;
                        continue;
                    }
                }
                buf.push(ch);
                i += 1;
            }
            _ => {
                buf.push(ch);
                i += 1;
            }
        }
    }
    flush(&mut buf, &mut toks);
    process_emphasis(&mut toks);
    let mut out: Vec<Inline> = Vec::new();
    for t in toks {
        push_merged(&mut out, tok_to_inline(t));
    }
    out
}

fn push_merged(out: &mut Vec<Inline>, i: Inline) {
    if let (Inline::Text(t), Some(Inline::Text(prev))) = (&i, out.last_mut()) {
        prev.push_str(t);
        return;
    }
    out.push(i);
}

fn tok_to_inline(t: Tok) -> Inline {
    match t {
        Tok::N(i) => i,
        Tok::D { ch, n, .. } => Inline::Text(ch.to_string().repeat(n)),
    }
}

fn process_emphasis(toks: &mut Vec<Tok>) {
    let mut i = 0;
    while i < toks.len() {
        let (ch, cn, copen, cclose) = match &toks[i] {
            Tok::D { ch, n, open, close } => (*ch, *n, *open, *close),
            _ => {
                i += 1;
                continue;
            }
        };
        if !cclose || cn == 0 {
            i += 1;
            continue;
        }
        let mut found = None;
        let mut j = i;
        while j > 0 {
            j -= 1;
            if let Tok::D { ch: oc, n: on, open: oo, close: ocl } = &toks[j] {
                if *oc != ch || !*oo || *on == 0 {
                    continue;
                }
                if ch == '~' {
                    if *on < 2 || cn < 2 {
                        continue;
                    }
                } else if (copen || *ocl) && (on + cn) % 3 == 0 && !(on % 3 == 0 && cn % 3 == 0) {
                    continue;
                }
                found = Some((j, *on));
                break;
            }
        }
        let Some((j, on)) = found else {
            i += 1;
            continue;
        };
        let used = if ch == '~' || (on >= 2 && cn >= 2) { 2 } else { 1 };
        let inner: Vec<Inline> = {
            let mut v = Vec::new();
            for t in toks.drain(j + 1..i) {
                push_merged(&mut v, tok_to_inline(t));
            }
            v
        };
        let wrapped = match (ch, used) {
            ('~', _) => Inline::Strike(inner),
            (_, 2) => Inline::Strong(inner),
            _ => Inline::Emph(inner),
        };
        toks.insert(j + 1, Tok::N(wrapped));
        let mut ci = j + 2;
        if let Tok::D { n, .. } = &mut toks[ci] {
            *n -= used;
            if *n == 0 {
                toks.remove(ci);
            }
        }
        if let Tok::D { n, .. } = &mut toks[j] {
            *n -= used;
            if *n == 0 {
                toks.remove(j);
                ci -= 1;
            }
        }
        i = ci;
    }
}

fn run_len(c: &[char], i: usize, ch: char) -> usize {
    c[i..].iter().take_while(|&&x| x == ch).count()
}

fn starts_with(c: &[char], i: usize, s: &str) -> bool {
    let mut k = i;
    for sc in s.chars() {
        if c.get(k) != Some(&sc) {
            return false;
        }
        k += 1;
    }
    true
}

fn find_closing_ticks(c: &[char], from: usize, n: usize) -> Option<usize> {
    let mut i = from;
    while i < c.len() {
        if c[i] == '`' {
            let r = run_len(c, i, '`');
            if r == n {
                return Some(i);
            }
            i += r;
        } else {
            i += 1;
        }
    }
    None
}

fn is_autolink(s: &str) -> bool {
    if s.contains(char::is_whitespace) || s.is_empty() {
        return false;
    }
    if let Some(p) = s.find(':') {
        let scheme = &s[..p];
        return scheme.len() >= 2
            && scheme.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
            && scheme.chars().all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c))
            && p + 1 < s.len();
    }
    // e-mail
    s.contains('@') && s.contains('.') && !s.starts_with('@')
}

fn entity(c: &[char]) -> Option<(String, usize)> {
    let semi = c.iter().take(12).position(|&x| x == ';')?;
    let name: String = c[1..semi].iter().collect();
    let s = match name.as_str() {
        "amp" => "&".to_string(),
        "lt" => "<".to_string(),
        "gt" => ">".to_string(),
        "quot" => "\"".to_string(),
        "apos" => "'".to_string(),
        "nbsp" => "\u{a0}".to_string(),
        "copy" => "\u{a9}".to_string(),
        "reg" => "\u{ae}".to_string(),
        "mdash" => "\u{2014}".to_string(),
        "ndash" => "\u{2013}".to_string(),
        "hellip" => "\u{2026}".to_string(),
        n if n.starts_with("#x") || n.starts_with("#X") => {
            char::from_u32(u32::from_str_radix(&n[2..], 16).ok()?)?.to_string()
        }
        n if n.starts_with('#') => char::from_u32(n[1..].parse().ok()?)?.to_string(),
        _ => return None,
    };
    Some((s, semi + 1))
}

/// Finds the `]` matching the `[` at `open`, skipping escapes and code spans.
fn matching_bracket(c: &[char], open: usize) -> Option<usize> {
    let mut depth = 0;
    let mut i = open;
    while i < c.len() {
        match c[i] {
            '\\' => i += 1,
            '`' => {
                let n = run_len(c, i, '`');
                if let Some(e) = find_closing_ticks(c, i + n, n) {
                    i = e + n - 1;
                } else {
                    i += n - 1;
                }
            }
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Tries to parse a link starting at the `[` at `open`.
/// Returns (url, label slice, index just past the construct).
fn try_link<'a>(c: &'a [char], open: usize, refs: &Refs) -> Option<(String, &'a [char], usize)> {
    let close = matching_bracket(c, open)?;
    let label = &c[open + 1..close];
    match c.get(close + 1) {
        Some('(') => {
            let mut i = close + 2;
            while i < c.len() && c[i].is_whitespace() {
                i += 1;
            }
            let mut url = String::new();
            if c.get(i) == Some(&'<') {
                i += 1;
                while i < c.len() && c[i] != '>' && c[i] != '\n' {
                    url.push(c[i]);
                    i += 1;
                }
                if c.get(i) != Some(&'>') {
                    return None;
                }
                i += 1;
            } else {
                let mut depth = 0;
                while i < c.len() && !c[i].is_whitespace() {
                    match c[i] {
                        '\\' if i + 1 < c.len() && c[i + 1].is_ascii_punctuation() => {
                            i += 1;
                            url.push(c[i]);
                        }
                        '(' => {
                            depth += 1;
                            url.push('(');
                        }
                        ')' => {
                            if depth == 0 {
                                break;
                            }
                            depth -= 1;
                            url.push(')');
                        }
                        x => url.push(x),
                    }
                    i += 1;
                }
            }
            while i < c.len() && c[i].is_whitespace() {
                i += 1;
            }
            if let Some(&q) = c.get(i) {
                if q == '"' || q == '\'' || q == '(' {
                    let endq = if q == '(' { ')' } else { q };
                    i += 1;
                    while i < c.len() && c[i] != endq {
                        i += 1;
                    }
                    i += 1;
                    while i < c.len() && c[i].is_whitespace() {
                        i += 1;
                    }
                }
            }
            if c.get(i) == Some(&')') {
                Some((url, label, i + 1))
            } else {
                None
            }
        }
        Some('[') => {
            let end = c[close + 2..].iter().position(|&x| x == ']')? + close + 2;
            let key: String = c[close + 2..end].iter().collect();
            let key = if key.trim().is_empty() { label.iter().collect() } else { key };
            refs.get(&normalize_label(&key)).map(|u| (u.clone(), label, end + 1))
        }
        _ => {
            let key: String = label.iter().collect();
            refs.get(&normalize_label(&key)).map(|u| (u.clone(), label, close + 1))
        }
    }
}
