//! What differs between SQL engines, behind one trait.
//!
//! The compiler builds standard SQL and asks the dialect only for what has no
//! standard spelling: regular expressions, the names of the types a lexical
//! form is cast to, and whether `WITH RECURSIVE` is available. DuckDB is the
//! first dialect; another engine is one more impl of [`Dialect`] and a variant
//! of [`SqlDialect`], and the compiler does not change.

use crate::validator::sql::SqlCompileError;
use crate::validator::sql::ast::{case, cast, compare, eq, function, in_list, not_eq, number, string};
use sqlparser::ast::{BinaryOperator, CastKind, DataType, Expr, Ident, ObjectName, Statement};

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

    /// The natural RDF lexical form of a column value (R2RML §10.2):
    /// `2020-01-02T03:04:05Z` for a timestamp, hex digits for binary, `INF`
    /// for an infinite double, … It is the lexical form whether the datatype
    /// is natural or overridden by `rr:datatype`: an override replaces only
    /// the datatype IRI (§10.3).
    fn natural_lexical(&self, value: Expr) -> Expr;

    /// The natural RDF datatype of a column value, from its SQL type: an
    /// integer is an `xsd:integer`, a boolean an `xsd:boolean`, binary an
    /// `xsd:hexBinary`, text an `xsd:string`, and so on.
    fn natural_datatype(&self, value: Expr) -> Expr;

    /// `text` made IRI-safe (R2RML §7.3): every character outside RFC 3987's
    /// `iunreserved` percent-encoded. `%` goes first, so no escape is escaped
    /// twice; the characters encoded are ASCII (NUL, which no SQL string
    /// holds, aside) and the C1 controls, and every
    /// other non-ASCII character is kept as the `ucschar` it nearly always is
    /// (the private-use and noncharacter code points are the exception).
    fn iri_safe(&self, text: Expr) -> Expr {
        let unreserved = |c: char| c.is_ascii_alphanumeric() || "-._~".contains(c);
        std::iter::once('%')
            .chain(
                (1u32..=0x9f)
                    .filter_map(char::from_u32)
                    .filter(|&c| c != '%' && !unreserved(c) && (c.is_ascii() || c.is_control())),
            )
            .fold(text, |text, c| {
                let mut utf8 = [0; 4];
                let escaped: String = c.encode_utf8(&mut utf8).bytes().map(|b| format!("%{b:02X}")).collect();
                function("REPLACE", vec![text, string(&c.to_string()), string(&escaped)])
            })
    }

    /// `expr` as text: a host column becomes a lexical form.
    fn to_text(&self, expr: Expr) -> Expr {
        cast(expr, DataType::Varchar(None), CastKind::Cast)
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

/// The engines the SQL is written for. Adding one is a variant here, an impl
/// of the internal dialect trait and an arm of [`compile`](super::compile);
/// no caller (facade, CLI, wasm binding) changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SqlDialect {
    /// DuckDB, the default.
    #[default]
    DuckDb,
}

impl std::fmt::Display for SqlDialect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SqlDialect::DuckDb => write!(f, "duckdb"),
        }
    }
}

impl std::str::FromStr for SqlDialect {
    type Err = SqlCompileError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "duckdb" => Ok(SqlDialect::DuckDb),
            other => Err(SqlCompileError::Unsupported(format!(
                "the SQL dialect '{other}' (supported: duckdb)"
            ))),
        }
    }
}

/// DuckDB: RE2 regular expressions through `regexp_matches`, and its own
/// names for the unsigned integer types.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct DuckDb;

/// `expr LIKE 'prefix%'`.
fn like(expr: &Expr, prefix: &str) -> Expr {
    Expr::Like {
        negated: false,
        any: false,
        expr: Box::new(expr.clone()),
        pattern: Box::new(string(&format!("{prefix}%"))),
        escape_char: None,
    }
}

/// The canonical `xsd:double` lexical form (XSD 1.0 §3.2.5.2: `3.0E1`,
/// `-1.23E-3`, `0.0E0`) of DuckDB's text of a finite double, which is
/// either positional (`30.0`, `0.00123`) or scientific (`1.5e-07`).
fn canonical_double(text: Expr) -> Expr {
    let f = |name: &str, args: Vec<Expr>| function(name, args);
    let concat = |parts: Vec<Expr>| {
        parts
            .into_iter()
            .reduce(|a, b| compare(a, BinaryOperator::StringConcat, b))
            .unwrap_or_else(|| string(""))
    };
    let negative = like(&text, "-");
    let sign = case(vec![(negative, string("-"))], string(""));
    let abs = f("ltrim", vec![text, string("-")]);
    let integer = cast(
        f("regexp_extract", vec![abs.clone(), string("e(.*)$"), number(1)]),
        custom("INTEGER"),
        CastKind::Cast,
    );
    let scientific = concat(vec![
        sign.clone(),
        f("regexp_extract", vec![abs.clone(), string("^([0-9])"), number(1)]),
        string("."),
        f(
            "coalesce",
            vec![
                f(
                    "nullif",
                    vec![
                        f(
                            "rtrim",
                            vec![
                                f(
                                    "regexp_extract",
                                    vec![abs.clone(), string("^[0-9]\\.?([0-9]*)e"), number(1)],
                                ),
                                string("0"),
                            ],
                        ),
                        string(""),
                    ],
                ),
                string("0"),
            ],
        ),
        string("E"),
        cast(integer, DataType::Varchar(None), CastKind::Cast),
    ]);
    // Positional: the significant digits, and the exponent from where the
    // first of them sits.
    let whole = f("split_part", vec![abs.clone(), string("."), number(1)]);
    let fraction = f("split_part", vec![abs.clone(), string("."), number(2)]);
    let digits = f(
        "rtrim",
        vec![
            f(
                "ltrim",
                vec![f("replace", vec![abs.clone(), string("."), string("")]), string("0")],
            ),
            string("0"),
        ],
    );
    let exponent = case(
        vec![(
            not_eq(whole.clone(), string("0")),
            compare(f("length", vec![whole]), BinaryOperator::Minus, number(1)),
        )],
        compare(
            compare(
                f("length", vec![f("ltrim", vec![fraction.clone(), string("0")])]),
                BinaryOperator::Minus,
                f("length", vec![fraction]),
            ),
            BinaryOperator::Minus,
            number(1),
        ),
    );
    let positional = concat(vec![
        sign.clone(),
        f("left", vec![digits.clone(), number(1)]),
        string("."),
        f(
            "coalesce",
            vec![
                f("nullif", vec![f("substr", vec![digits.clone(), number(2)]), string("")]),
                string("0"),
            ],
        ),
        string("E"),
        cast(exponent, DataType::Varchar(None), CastKind::Cast),
    ]);
    case(
        vec![
            (f("regexp_matches", vec![abs.clone(), string("e")]), scientific),
            (eq(digits, string("")), concat(vec![sign, string("0.0E0")])),
        ],
        positional,
    )
}

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
    fn natural_lexical(&self, value: Expr) -> Expr {
        let ty = function("typeof", vec![value.clone()]);
        let text = self.to_text(value);
        let blob = cast(text.clone(), custom("BLOB"), CastKind::Cast);
        let regexp_replace =
            |e: Expr, pattern: &str, with: &str| function("regexp_replace", vec![e, string(pattern), string(with)]);
        let timestamp = regexp_replace(
            regexp_replace(regexp_replace(text.clone(), " ", "T"), "([+-][0-9][0-9])$", "\\1:00"),
            "[+-]00:00$",
            "Z",
        );
        let double = case(
            vec![
                (eq(text.clone(), string("inf")), string("INF")),
                (eq(text.clone(), string("-inf")), string("-INF")),
                (eq(text.clone(), string("nan")), string("NaN")),
            ],
            canonical_double(text.clone()),
        );
        case(
            vec![
                (eq(ty.clone(), string("BLOB")), function("hex", vec![blob])),
                (like(&ty, "TIMESTAMP"), timestamp),
                (in_list(ty, vec![string("DOUBLE"), string("FLOAT")]), double),
            ],
            text,
        )
    }

    fn natural_datatype(&self, value: Expr) -> Expr {
        let ty = function("typeof", vec![value]);
        let is = |names: &[&str]| in_list(ty.clone(), names.iter().map(|n| string(n)).collect());
        let xsd = |local: &str| string(&format!("http://www.w3.org/2001/XMLSchema#{local}"));
        case(
            vec![
                (
                    is(&[
                        "TINYINT",
                        "SMALLINT",
                        "INTEGER",
                        "BIGINT",
                        "HUGEINT",
                        "UTINYINT",
                        "USMALLINT",
                        "UINTEGER",
                        "UBIGINT",
                        "UHUGEINT",
                    ]),
                    xsd("integer"),
                ),
                (is(&["DOUBLE", "FLOAT"]), xsd("double")),
                (like(&ty, "DECIMAL"), xsd("decimal")),
                (is(&["BOOLEAN"]), xsd("boolean")),
                (is(&["DATE"]), xsd("date")),
                (like(&ty, "TIMESTAMP"), xsd("dateTime")),
                (like(&ty, "TIME"), xsd("time")),
                (is(&["BLOB"]), xsd("hexBinary")),
            ],
            xsd("string"),
        )
    }

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
