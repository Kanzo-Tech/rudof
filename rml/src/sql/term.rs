//! Term maps as SQL: the four columns of a term (`kind`, `value`, `datatype`,
//! `lang`), and the rows for which the term map generates nothing.
//!
//! **The relation's term encoding** is `shacl::validator::sql::term`'s: kinds
//! `I` (IRI), `B` (blank node) and `L` (literal); an IRI or a blank node has an
//! empty datatype and language; a literal always has a datatype, `xsd:string`
//! for a simple one and `rdf:langString` for a language-tagged one, whose tag is
//! in `lang`.
//!
//! **Natural mapping.** RML-Core §11.1 leaves the natural datatype and lexical
//! form of a value to the reference formulation ("defined in the RML-IO-Registry")
//! and §11.3 (non-normative) lists the XSD lexical forms. R2RML §10.2 (its
//! natural mapping of SQL values) is the model, and the datatype is decided
//! *where the value is*: by `typeof()` of the column in the engine, which DuckDB
//! folds to a constant when it plans the query, so the mapping needs no catalog.

use super::build::{
    case, col, concat, eq, function, in_list, is_not_null, lambda, not_eq, string, try_cast_blob, varchar,
};
use crate::ast::{Constant, Expression, LanguageMap, LiteralKind, Position, Template, TemplatePart, TermMap, TermType};
use crate::vocab::{RDF, XSD};
use sqlparser::ast::Expr;

pub const IRI: &str = "I";
pub const BLANK: &str = "B";
pub const LITERAL: &str = "L";

/// A term map evaluated on an iteration: its four columns, and the conditions
/// under which it generates a term at all (§12.2: "if the value is null, empty
/// or missing, then no RDF term is generated"; §7.1.3: a template with an empty
/// reference value is NULL; §8.5: an empty language map generates nothing).
pub struct Term {
    pub kind: Expr,
    pub value: Expr,
    pub datatype: Expr,
    pub lang: Expr,
    pub required: Vec<Expr>,
}

/// The name of the column a reference expression names (RML-IO: for the SQL
/// reference formulations a reference is a column name): a name wholly enclosed
/// in double quotes is a delimited identifier, and anything else is the name.
pub fn column_name(reference: &str) -> String {
    match reference.strip_prefix('"').and_then(|r| r.strip_suffix('"')) {
        Some(inner) if !inner.is_empty() => inner.replace("\"\"", "\""),
        _ => reference.to_owned(),
    }
}

fn typeof_(value: &Expr) -> Expr {
    function("typeof", vec![value.clone()])
}

const INTEGERS: [&str; 10] = [
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
];
const FLOATS: [&str; 2] = ["FLOAT", "DOUBLE"];
const TIMESTAMPS: [&str; 4] = ["TIMESTAMP", "TIMESTAMP_S", "TIMESTAMP_MS", "TIMESTAMP_NS"];

fn xsd(local: &str) -> Expr {
    string(&format!("{XSD}{local}"))
}

/// The natural RDF datatype of a value, by its SQL type (R2RML §10.2): XSD's
/// for the types it names and `xsd:string` otherwise.
pub fn natural_datatype(value: &Expr) -> Expr {
    let ty = typeof_(value);
    case(
        vec![
            (in_list(ty.clone(), &INTEGERS), xsd("integer")),
            (in_list(ty.clone(), &FLOATS), xsd("double")),
            (
                function("starts_with", vec![ty.clone(), string("DECIMAL")]),
                xsd("decimal"),
            ),
            (eq(ty.clone(), string("BOOLEAN")), xsd("boolean")),
            (eq(ty.clone(), string("DATE")), xsd("date")),
            (eq(ty.clone(), string("TIME")), xsd("time")),
            (in_list(ty.clone(), &TIMESTAMPS), xsd("dateTime")),
            (eq(ty, string("BLOB")), xsd("hexBinary")),
        ],
        xsd("string"),
    )
}

/// A value's natural lexical form (§11.1, §11.3): its text, with the corrections
/// XSD needs — `T` between a timestamp's date and time (§11.3: "Convert from
/// SQL by replacing space with T"), a binary's hex (§11.3: "Convert from SQL by
/// applying xsd:hexBinary lexical mapping"), and XSD's spelling of NaN and the
/// infinities, which DuckDB writes `nan`, `inf` and `-inf`.
pub fn lexical(value: &Expr) -> Expr {
    let ty = typeof_(value);
    let text = varchar(value.clone());
    case(
        vec![
            (
                in_list(ty.clone(), &TIMESTAMPS),
                function("replace", vec![text.clone(), string(" "), string("T")]),
            ),
            (
                eq(ty.clone(), string("BLOB")),
                function("hex", vec![try_cast_blob(value.clone())]),
            ),
            (
                in_list(ty, &FLOATS),
                case(
                    vec![
                        (eq(text.clone(), string("nan")), string("NaN")),
                        (eq(text.clone(), string("inf")), string("INF")),
                        (eq(text.clone(), string("-inf")), string("-INF")),
                    ],
                    text.clone(),
                ),
            ),
        ],
        text,
    )
}

/// The characters RFC 3987 calls `iunreserved`: `ALPHA / DIGIT / "-" / "." / "_" / "~" / ucschar`.
const IUNRESERVED: &str = "^[A-Za-z0-9._~\\-\\x{A0}-\\x{D7FF}\\x{F900}-\\x{FDCF}\\x{FDF0}-\\x{FFEF}\
    \\x{10000}-\\x{1FFFD}\\x{20000}-\\x{2FFFD}\\x{30000}-\\x{3FFFD}\\x{40000}-\\x{4FFFD}\\x{50000}-\\x{5FFFD}\
    \\x{60000}-\\x{6FFFD}\\x{70000}-\\x{7FFFD}\\x{80000}-\\x{8FFFD}\\x{90000}-\\x{9FFFD}\\x{A0000}-\\x{AFFFD}\
    \\x{B0000}-\\x{BFFFD}\\x{C0000}-\\x{CFFFD}\\x{D0000}-\\x{DFFFD}\\x{E1000}-\\x{EFFFD}]$";

/// The IRI-safe version of a string (RML-Core §8.3.1): "any character that is
/// not in the `iunreserved` production in RFC 3987" is converted "to a sequence
/// of one or more octets using UTF-8" and each octet percent-encoded. A
/// character outside `unreserved` that is not `iunreserved` is outside both
/// ASCII's unreserved set and `ucschar`, so DuckDB's `url_encode` (which encodes
/// everything but `unreserved`) is exactly the octet encoding for it.
pub fn iri_safe(text: Expr) -> Expr {
    let character = Expr::Identifier(sqlparser::ast::Ident::new("c"));
    let keep = function("regexp_matches", vec![character.clone(), string(IUNRESERVED)]);
    function(
        "array_to_string",
        vec![
            function(
                "list_transform",
                vec![
                    function("string_split", vec![text, string("")]),
                    lambda(
                        "c",
                        case(vec![(keep, character.clone())], function("url_encode", vec![character])),
                    ),
                ],
            ),
            string(""),
        ],
    )
}

/// Whether `text` starts with a URI scheme, `ALPHA *( ALPHA / DIGIT / "+" / "-" / "." ) ":"`
/// (RFC 3986 §3.1): the part of "a valid absolute IRI" (§12.2) that SQL can see.
fn starts_with_scheme(text: &str) -> bool {
    let mut chars = text.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars
            .by_ref()
            .find(|c| !(c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')))
            .is_some_and(|c| c == ':')
}

/// A generated IRI value (§12.2): "if value is a valid absolute IRI, then return
/// an IRI generated from value. Otherwise, prepend value with the base IRI."
/// Validity beyond the scheme is not checked (a data error is not raised).
/// `known_absolute` says the text is absolute whatever the data.
fn iri_value(text: Expr, base: &str, known_absolute: bool) -> Expr {
    if known_absolute {
        return text;
    }
    let absolute = function(
        "regexp_matches",
        vec![text.clone(), string("^[A-Za-z][A-Za-z0-9+.-]*:")],
    );
    concat(vec![case(vec![(absolute, string(""))], string(base)), text])
}

fn constant(c: &Constant) -> Term {
    let (kind, value, datatype, lang) = match c {
        Constant::Iri(iri) => (IRI, iri.as_str().to_owned(), String::new(), String::new()),
        Constant::Literal(l) => match &l.kind {
            LiteralKind::Datatype(d) => (LITERAL, l.lexical_form.clone(), d.as_str().to_owned(), String::new()),
            LiteralKind::Language(tag) => (LITERAL, l.lexical_form.clone(), format!("{RDF}langString"), tag.clone()),
        },
    };
    Term {
        kind: string(kind),
        value: string(&value),
        datatype: string(&datatype),
        lang: string(&lang),
        required: Vec::new(),
    }
}

/// A template's value (§7.1.3), the natural lexical form of each reference
/// concatenated with the text, each IRI-encoded when `encode` (§8.3.1). Its
/// references must all have a value.
fn template(t: &Template, alias: &str, encode: bool) -> (Expr, Vec<Expr>) {
    let mut required = Vec::new();
    let parts =
        t.0.iter()
            .map(|part| match part {
                TemplatePart::Text(text) => string(text),
                TemplatePart::Reference(r) => {
                    let value = col(alias, &column_name(r));
                    required.push(is_not_null(value.clone()));
                    let text = lexical(&value);
                    if encode { iri_safe(text) } else { text }
                },
            })
            .collect();
    (concat(parts), required)
}

/// `map` evaluated on the iteration `alias`, at `position`, with `base` the base
/// IRI of its triples map (§4.1.1).
pub fn term(map: &TermMap, position: Position, alias: &str, base: &str) -> Term {
    let term_type = map.term_type(position);
    let (text, natural, mut required, absolute) = match &map.expression {
        Expression::Constant(c) => return constant(c),
        Expression::Reference(r) => {
            let value = col(alias, &column_name(r));
            (
                lexical(&value),
                natural_datatype(&value),
                vec![is_not_null(value)],
                false,
            )
        },
        Expression::Template(t) => {
            let encode = term_type == TermType::Iri;
            let (text, required) = template(t, alias, encode);
            let absolute = matches!(t.0.first(), Some(TemplatePart::Text(first)) if starts_with_scheme(first));
            (text, xsd("string"), required, absolute)
        },
    };
    match term_type {
        TermType::Iri | TermType::UnsafeIri => Term {
            kind: string(IRI),
            value: iri_value(text, base, absolute),
            datatype: string(""),
            lang: string(""),
            required,
        },
        TermType::BlankNode => Term {
            kind: string(BLANK),
            value: text,
            datatype: string(""),
            lang: string(""),
            required,
        },
        TermType::Literal => {
            let (datatype, lang) = match (&map.language, &map.datatype) {
                (Some(LanguageMap::Constant(tag)), _) => (string(&format!("{RDF}langString")), string(tag)),
                (Some(LanguageMap::Reference(r)), _) => {
                    let tag = col(alias, &column_name(r));
                    // §8.5: the tags are "non-empty"; a missing one generates no term.
                    required.push(is_not_null(tag.clone()));
                    required.push(not_eq(varchar(tag.clone()), string("")));
                    (string(&format!("{RDF}langString")), varchar(tag))
                },
                (None, Some(datatype)) => (string(datatype.as_str()), string("")),
                (None, None) => (natural, string("")),
            };
            Term {
                kind: string(LITERAL),
                value: text,
                datatype,
                lang,
                required,
            }
        },
    }
}
