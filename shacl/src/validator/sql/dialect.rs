//! What differs between SQL engines, behind one trait.
//!
//! The compiler builds standard SQL and asks the dialect only for what has no
//! standard spelling: regular expressions, the names of the types a lexical
//! form is cast to, and whether `WITH RECURSIVE` is available. DuckDB is the
//! one dialect; another engine is one more impl of [`Dialect`], and the
//! compiler does not change.

use crate::validator::sql::SqlCompileError;
use crate::validator::sql::ast::{cast, function, string};
use sqlparser::ast::{CastKind, DataType, Expr, Ident, ObjectName, Statement};

/// A type a lexical form is cast to, to compare it by value or to check that it
/// lies in a datatype's value space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CastTarget {
    Int8,
    Int16,
    Int32,
    Int64,
    UInt8,
    UInt16,
    UInt32,
    UInt64,
    Double,
    TimestampTz,
}

/// The engine-specific parts of the SQL the compiler emits.
pub(crate) trait Dialect {
    /// A short name, e.g. `duckdb`.
    fn name(&self) -> &'static str;

    /// Whether `text` contains a match of `pattern`, under the XPath flags of
    /// `sh:flags` (SPARQL `REGEX` semantics: a match anywhere in the string).
    fn regex_match(&self, text: Expr, pattern: &str, flags: Option<&str>) -> Result<Expr, SqlCompileError>;

    /// `expr` cast to `target`, or `NULL` when it does not convert.
    fn try_cast(&self, expr: Expr, target: CastTarget) -> Expr;

    /// The milliseconds since the Unix epoch of a `TIMESTAMPTZ` value, as an
    /// integer.
    fn epoch_millis(&self, timestamp: Expr) -> Expr;

    /// The length of `text` in characters, as SPARQL `STRLEN` counts them.
    fn char_length(&self, text: Expr) -> Expr {
        function("LENGTH", vec![text])
    }

    /// Whether `WITH RECURSIVE` is available. Without it, `sh:zeroOrMorePath`
    /// and `sh:oneOrMorePath` are refused.
    fn supports_recursive_cte(&self) -> bool {
        true
    }

    /// The text of `statement` for this engine.
    fn render(&self, statement: &Statement) -> String {
        statement.to_string()
    }
}

/// DuckDB: RE2 regular expressions through `regexp_matches`, and its own
/// names for the unsigned integer types.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct DuckDb;

fn custom(name: &str) -> DataType {
    DataType::Custom(ObjectName::from(vec![Ident::new(name)]), Vec::new())
}

/// Translates XPath regex flags (`sh:flags`) into an RE2 inline-flag prefix.
///
/// `i`, `m` and `s` map one to one; `x` removes the pattern's whitespace (as
/// XPath does, outside character classes) and `q` quotes the whole pattern.
fn re2_pattern(pattern: &str, flags: Option<&str>) -> Result<String, SqlCompileError> {
    let flags = flags.unwrap_or_default();
    let mut inline = String::new();
    let mut body = pattern.to_owned();
    for flag in flags.chars() {
        match flag {
            'i' | 'm' | 's' => inline.push(flag),
            'x' => {
                let mut out = String::new();
                let mut in_class = false;
                for c in body.chars() {
                    match c {
                        '[' => in_class = true,
                        ']' => in_class = false,
                        _ => {},
                    }
                    if in_class || !c.is_whitespace() {
                        out.push(c);
                    }
                }
                body = out;
            },
            'q' => body = regex_quote(&body),
            other => {
                return Err(SqlCompileError::Unsupported(format!(
                    "the regular expression flag {other:?} of sh:flags"
                )));
            },
        }
    }
    Ok(if inline.is_empty() {
        body
    } else {
        format!("(?{inline}){body}")
    })
}

fn regex_quote(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if "\\.+*?()|[]{}^$".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

impl Dialect for DuckDb {
    fn name(&self) -> &'static str {
        "duckdb"
    }

    fn regex_match(&self, text: Expr, pattern: &str, flags: Option<&str>) -> Result<Expr, SqlCompileError> {
        Ok(function(
            "regexp_matches",
            vec![text, string(&re2_pattern(pattern, flags)?)],
        ))
    }

    // `epoch_ms`, an integer: DuckDB 1.5's binder rejects arithmetic on the
    // DOUBLE that `epoch` returns ("No function matches (DOUBLE, INTEGER)").
    fn epoch_millis(&self, timestamp: Expr) -> Expr {
        function("epoch_ms", vec![timestamp])
    }

    // A value's SQL type is known only when the query runs, so both choose
    // by `typeof`. Every branch must bind for a column of any type, which is
    // why the lexical forms are string operations on the value's text, the
    // one conversion every type has: a TIMESTAMP's text has a space where XSD
    // writes `T` and a TIMESTAMPTZ's an offset of `+HH` (`Z` for UTC); a
    // BLOB's text escapes bytes as `\xHH`, and its round trip back to BLOB
    // is what `hex` reads; a DOUBLE writes `inf`, `-inf` and `nan`.
    fn try_cast(&self, expr: Expr, target: CastTarget) -> Expr {
        let data_type = match target {
            CastTarget::Int8 => custom("TINYINT"),
            CastTarget::Int16 => custom("SMALLINT"),
            CastTarget::Int32 => custom("INTEGER"),
            CastTarget::Int64 => custom("BIGINT"),
            CastTarget::UInt8 => custom("UTINYINT"),
            CastTarget::UInt16 => custom("USMALLINT"),
            CastTarget::UInt32 => custom("UINTEGER"),
            CastTarget::UInt64 => custom("UBIGINT"),
            CastTarget::Double => custom("DOUBLE"),
            CastTarget::TimestampTz => custom("TIMESTAMPTZ"),
        };
        cast(expr, data_type, CastKind::TryCast)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_become_inline_re2_flags() {
        assert_eq!(re2_pattern("^a", Some("i")).unwrap(), "(?i)^a");
        assert_eq!(re2_pattern("a b", Some("x")).unwrap(), "ab");
        assert_eq!(re2_pattern("a.b", Some("q")).unwrap(), r"a\.b");
        assert!(re2_pattern("a", Some("z")).is_err());
    }
}
