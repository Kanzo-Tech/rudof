//! What the SQL tests share: reading a case's expected N-Triples and comparing
//! them with the rows of the relation as sets of terms.

#![cfg(feature = "sql")]
#![allow(dead_code)]

use sqlparser::ast::{Ident, ObjectName};
use std::collections::BTreeSet;

pub const XSD_STRING: &str = "http://www.w3.org/2001/XMLSchema#string";
pub const RDF_LANG_STRING: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#langString";

/// The tables a test runs over are named as the mapping names them.
pub fn identity(table: &str) -> ObjectName {
    ObjectName::from(vec![Ident::with_quote('"', table)])
}

/// One triple as a comparable string: the kind, value, datatype and language of each term.
/// A language tag compares case-insensitively (RDF 1.1 Concepts §3.3).
pub fn key(row: [&str; 7]) -> String {
    let [s_type, s_value, p, o_type, o_value, o_datatype, o_lang] = row;
    format!(
        "{s_type} {s_value} | {p} | {o_type} {o_value} | {o_datatype} | {}",
        o_lang.to_lowercase()
    )
}

/// The triples of a (loosely written) N-Triples document, as [`key`]s: whitespace before the final
/// dot is optional, as the suite's expected outputs have it, and `#` lines are comments.
pub fn expected(nt: &str) -> BTreeSet<String> {
    nt.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|line| triple(line).unwrap_or_else(|e| panic!("{e}: {line}")))
        .collect()
}

fn triple(line: &str) -> Result<String, String> {
    let mut rest = line;
    let (s_type, s_value) = node(&mut rest)?;
    let (_, p) = node(&mut rest)?;
    rest = rest.trim_start();
    let (o_type, o_value, o_datatype, o_lang) = if rest.starts_with('"') {
        let (value, after) = quoted(rest)?;
        rest = after;
        if let Some(r) = rest.strip_prefix("^^") {
            rest = r;
            let (_, datatype) = node(&mut rest)?;
            ("L".to_owned(), value, datatype, String::new())
        } else if let Some(r) = rest.strip_prefix('@') {
            let end = r
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
                .unwrap_or(r.len());
            rest = &r[end..];
            ("L".to_owned(), value, RDF_LANG_STRING.to_owned(), r[..end].to_owned())
        } else {
            ("L".to_owned(), value, XSD_STRING.to_owned(), String::new())
        }
    } else {
        let (kind, value) = node(&mut rest)?;
        (kind, value, String::new(), String::new())
    };
    if rest.trim() != "." {
        return Err(format!("expected '.', found {rest:?}"));
    }
    Ok(key([&s_type, &s_value, &p, &o_type, &o_value, &o_datatype, &o_lang]))
}

/// `<iri>` or `_:label`, and the rest.
fn node(rest: &mut &str) -> Result<(String, String), String> {
    let text = rest.trim_start();
    if let Some(r) = text.strip_prefix('<') {
        let end = r.find('>').ok_or("unterminated IRI")?;
        *rest = &r[end + 1..];
        Ok(("I".to_owned(), r[..end].to_owned()))
    } else if let Some(r) = text.strip_prefix("_:") {
        let end = r.find(|c: char| c.is_whitespace()).unwrap_or(r.len());
        *rest = &r[end..];
        Ok(("B".to_owned(), r[..end].to_owned()))
    } else {
        Err(format!("expected a node at {text:?}"))
    }
}

/// A double-quoted string with `\"`, `\\`, `\n`, `\r`, `\t` escapes, and the rest.
fn quoted(text: &str) -> Result<(String, &str), String> {
    let mut out = String::new();
    let mut chars = text[1..].char_indices();
    while let Some((i, c)) = chars.next() {
        match c {
            '"' => return Ok((out, &text[i + 2..])),
            '\\' => match chars.next().map(|(_, e)| e) {
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some(e @ ('"' | '\\')) => out.push(e),
                other => return Err(format!("unsupported escape {other:?}")),
            },
            c => out.push(c),
        }
    }
    Err("unterminated string".to_owned())
}
