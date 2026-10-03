//! RDF terms as SQL values.
//!
//! A term is four text columns — kind, lexical form, datatype and language —
//! and every comparison the engine makes over them follows RDF/SPARQL
//! semantics, never SQL typing. Kinds are `I` (IRI), `B` (blank node) and `L`
//! (literal); a non-literal has an empty datatype and language, and a literal
//! always has a datatype (`xsd:string` for a simple literal, `rdf:langString`
//! for a language-tagged one). Empty strings rather than `NULL` keep term
//! equality a plain conjunction of `=`.

use crate::validator::sql::SqlCompileError;
use crate::validator::sql::ast::{
    and, and_all, boolean, case, col, compare, eq, in_list, item, not_eq, null, number, or_all, string,
};
use crate::validator::sql::dialect::{CastTarget, SqlDialect};
use oxrdf::{BlankNode, Literal, NamedNode, Term};
use rudof_rdf::term::Object;
use sqlparser::ast::{BinaryOperator, Expr, SelectItem};

pub const IRI: &str = "I";
pub const BLANK: &str = "B";
pub const LITERAL: &str = "L";

const XSD: &str = "http://www.w3.org/2001/XMLSchema#";
pub const XSD_STRING: &str = "http://www.w3.org/2001/XMLSchema#string";
pub const RDF_LANG_STRING: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#langString";

/// The text of a term, as the engine stores it: `[kind, lexical, datatype, language]`.
pub type EncodedTerm = [String; 4];

/// Encodes an RDF term into the four columns.
pub fn encode(term: &Term) -> Result<EncodedTerm, SqlCompileError> {
    Ok(match term {
        Term::NamedNode(n) => [IRI.to_owned(), n.as_str().to_owned(), String::new(), String::new()],
        Term::BlankNode(b) => [BLANK.to_owned(), b.as_str().to_owned(), String::new(), String::new()],
        Term::Literal(l) => [
            LITERAL.to_owned(),
            l.value().to_owned(),
            l.datatype().as_str().to_owned(),
            l.language().unwrap_or_default().to_owned(),
        ],
        #[allow(unreachable_patterns)]
        other => {
            return Err(SqlCompileError::Unsupported(format!(
                "the term {other} (only IRIs, blank nodes and literals have a SQL encoding)"
            )));
        },
    })
}

/// Encodes a shapes-graph value (`sh:hasValue`, `sh:in`, a bound...).
pub fn encode_object(object: &Object) -> Result<EncodedTerm, SqlCompileError> {
    encode(&Term::from(object.clone()))
}

/// Decodes the four columns back into the term the native engine would have
/// built for it (through the same `oxrdf::Term → Object` conversion).
pub fn decode(kind: &str, lexical: &str, datatype: &str, lang: &str) -> Result<Object, String> {
    let term: Term = match kind {
        IRI => NamedNode::new_unchecked(lexical).into(),
        BLANK => BlankNode::new_unchecked(lexical).into(),
        LITERAL if !lang.is_empty() => Literal::new_language_tagged_literal_unchecked(lexical, lang).into(),
        LITERAL if datatype.is_empty() || datatype == XSD_STRING => Literal::new_simple_literal(lexical).into(),
        LITERAL => Literal::new_typed_literal(lexical, NamedNode::new_unchecked(datatype)).into(),
        other => return Err(format!("unknown term kind {other:?}")),
    };
    Object::try_from(term).map_err(|e| e.to_string())
}

/// A term as four SQL expressions.
#[derive(Debug, Clone)]
pub struct TermExpr {
    pub kind: Expr,
    pub lex: Expr,
    pub datatype: Expr,
    pub lang: Expr,
}

impl TermExpr {
    /// The term held in `alias.{prefix}_k`, … columns.
    pub fn columns(alias: &str, prefix: &str) -> Self {
        let c = |s: &str| col(alias, &format!("{prefix}_{s}"));
        Self {
            kind: c("k"),
            lex: c("v"),
            datatype: c("d"),
            lang: c("l"),
        }
    }

    pub fn constant(encoded: &EncodedTerm) -> Self {
        Self {
            kind: string(&encoded[0]),
            lex: string(&encoded[1]),
            datatype: string(&encoded[2]),
            lang: string(&encoded[3]),
        }
    }

    pub fn object(object: &Object) -> Result<Self, SqlCompileError> {
        Ok(Self::constant(&encode_object(object)?))
    }

    /// `NULL` in every column: the value of a result that has none.
    pub fn null() -> Self {
        Self {
            kind: null(),
            lex: null(),
            datatype: null(),
            lang: null(),
        }
    }

    /// Projects the term as `{prefix}_k`, … .
    pub fn items(&self, prefix: &str) -> Vec<SelectItem> {
        vec![
            item(self.kind.clone(), &format!("{prefix}_k")),
            item(self.lex.clone(), &format!("{prefix}_v")),
            item(self.datatype.clone(), &format!("{prefix}_d")),
            item(self.lang.clone(), &format!("{prefix}_l")),
        ]
    }

    /// RDF term equality.
    pub fn same(&self, other: &TermExpr) -> Expr {
        and_all([
            eq(self.kind.clone(), other.kind.clone()),
            eq(self.lex.clone(), other.lex.clone()),
            eq(self.datatype.clone(), other.datatype.clone()),
            eq(self.lang.clone(), other.lang.clone()),
        ])
    }

    pub fn is_kind(&self, kind: &str) -> Expr {
        eq(self.kind.clone(), string(kind))
    }

    pub fn is_not_kind(&self, kind: &str) -> Expr {
        not_eq(self.kind.clone(), string(kind))
    }

    /// Membership in a list of constant terms.
    pub fn in_terms(&self, terms: &[EncodedTerm]) -> Expr {
        or_all(terms.iter().map(|t| self.same(&TermExpr::constant(t))))
    }
}

/// How the native engine reads a literal's lexical form against its datatype.
///
/// `check_literal_datatype` in `rudof_rdf` parses a fixed set of XSD types and
/// turns an ill-formed lexical form into a `WrongDatatypeLiteral`: a value of
/// the wrong datatype for `sh:datatype`, and incomparable for the range and
/// property-pair components. Every other datatype is taken as well-formed.
#[derive(Clone, Copy)]
enum Lexical {
    /// `[+-]?digits`, optionally range-checked by a cast.
    Integer(Option<CastTarget>),
    /// An integer whose value must satisfy a sign constraint, by pattern.
    SignedInteger(&'static str),
    Decimal,
    Double,
    Boolean,
    DateTime,
}

/// The datatypes the engine checks, with their lexical spaces.
const CHECKED: [(&str, Lexical); 18] = [
    ("integer", Lexical::Integer(None)),
    ("long", Lexical::Integer(Some(CastTarget::Int64))),
    ("int", Lexical::Integer(Some(CastTarget::Int32))),
    ("short", Lexical::Integer(Some(CastTarget::Int16))),
    ("byte", Lexical::Integer(Some(CastTarget::Int8))),
    ("unsignedLong", Lexical::Integer(Some(CastTarget::UInt64))),
    ("unsignedInt", Lexical::Integer(Some(CastTarget::UInt32))),
    ("unsignedShort", Lexical::Integer(Some(CastTarget::UInt16))),
    ("unsignedByte", Lexical::Integer(Some(CastTarget::UInt8))),
    ("nonNegativeInteger", Lexical::SignedInteger(r"^(\+?[0-9]+|-0+)$")),
    ("positiveInteger", Lexical::SignedInteger(r"^\+?0*[1-9][0-9]*$")),
    ("nonPositiveInteger", Lexical::SignedInteger(r"^(-[0-9]+|\+?0+)$")),
    ("negativeInteger", Lexical::SignedInteger(r"^-0*[1-9][0-9]*$")),
    ("decimal", Lexical::Decimal),
    ("double", Lexical::Double),
    ("float", Lexical::Double),
    ("boolean", Lexical::Boolean),
    ("dateTime", Lexical::DateTime),
];

const INTEGER_PATTERN: &str = r"^[+-]?[0-9]+$";
const DECIMAL_PATTERN: &str = r"^[+-]?([0-9]+(\.[0-9]*)?|\.[0-9]+)$";
const DOUBLE_PATTERN: &str = r"^([+-]?([0-9]+(\.[0-9]*)?|\.[0-9]+)([eE][+-]?[0-9]+)?|-?INF|NaN)$";
const BOOLEAN_PATTERN: &str = r"^(true|false|1|0)$";
const DATE_TIME_PATTERN: &str = r"^-?[0-9]{4,}-(0[1-9]|1[0-2])-(0[1-9]|[12][0-9]|3[01])T([01][0-9]|2[0-3]):[0-5][0-9]:[0-5][0-9](\.[0-9]+)?(Z|[+-]([01][0-9]|1[0-4]):[0-5][0-9])?$";

fn xsd(local: &str) -> String {
    format!("{XSD}{local}")
}

fn is_numeric(lexical: Lexical) -> bool {
    matches!(
        lexical,
        Lexical::Integer(_) | Lexical::SignedInteger(_) | Lexical::Decimal | Lexical::Double
    )
}

/// The well-formedness of `lex` under one lexical space.
fn lexical_check<D: SqlDialect + ?Sized>(dialect: &D, lex: &Expr, lexical: Lexical) -> Result<Expr, SqlCompileError> {
    let matches = |pattern: &str| dialect.regex_match(lex.clone(), pattern, None);
    Ok(match lexical {
        Lexical::Integer(None) => matches(INTEGER_PATTERN)?,
        Lexical::Integer(Some(target)) => and(
            matches(INTEGER_PATTERN)?,
            crate::validator::sql::ast::is_not_null(dialect.try_cast(lex.clone(), target)),
        ),
        Lexical::SignedInteger(pattern) => matches(pattern)?,
        Lexical::Decimal => matches(DECIMAL_PATTERN)?,
        Lexical::Double => matches(DOUBLE_PATTERN)?,
        Lexical::Boolean => matches(BOOLEAN_PATTERN)?,
        Lexical::DateTime => matches(DATE_TIME_PATTERN)?,
    })
}

/// Whether the literal `t` is well-formed for its own datatype, considering
/// only the datatypes in `only` (all checked ones when `None`). A literal of an
/// unchecked datatype is well-formed.
fn well_formed_among<D: SqlDialect + ?Sized>(
    dialect: &D,
    t: &TermExpr,
    keep: impl Fn(Lexical) -> bool,
    otherwise: bool,
) -> Result<Expr, SqlCompileError> {
    let mut branches = Vec::new();
    for (local, lexical) in CHECKED {
        if keep(lexical) {
            branches.push((
                eq(t.datatype.clone(), string(&xsd(local))),
                lexical_check(dialect, &t.lex, lexical)?,
            ));
        }
    }
    Ok(case(branches, boolean(otherwise)))
}

/// A literal well-formed for its datatype, restricted to `datatypes`: the
/// check `sh:datatype` makes once it knows the datatype is one of them.
pub fn well_formed_for<D: SqlDialect + ?Sized>(
    dialect: &D,
    t: &TermExpr,
    datatypes: &[String],
) -> Result<Expr, SqlCompileError> {
    let mut branches = Vec::new();
    for (local, lexical) in CHECKED {
        let iri = xsd(local);
        if datatypes.contains(&iri) {
            branches.push((
                eq(t.datatype.clone(), string(&iri)),
                lexical_check(dialect, &t.lex, lexical)?,
            ));
        }
    }
    Ok(case(branches, boolean(true)))
}

fn numeric_datatypes() -> Vec<Expr> {
    CHECKED
        .iter()
        .filter(|(_, l)| is_numeric(*l))
        .map(|(local, _)| string(&xsd(local)))
        .collect()
}

fn checked_datatypes() -> Vec<Expr> {
    CHECKED.iter().map(|(local, _)| string(&xsd(local))).collect()
}

fn is_numeric_literal<D: SqlDialect + ?Sized>(dialect: &D, t: &TermExpr) -> Result<Expr, SqlCompileError> {
    Ok(and(
        in_list(t.datatype.clone(), numeric_datatypes()),
        well_formed_among(dialect, t, is_numeric, false)?,
    ))
}

fn is_string_literal(t: &TermExpr) -> Expr {
    in_list(t.datatype.clone(), vec![string(XSD_STRING), string(RDF_LANG_STRING)])
}

fn has_datatype<D: SqlDialect + ?Sized>(
    dialect: &D,
    t: &TermExpr,
    local: &str,
    lexical: Lexical,
) -> Result<Expr, SqlCompileError> {
    Ok(and(
        eq(t.datatype.clone(), string(&xsd(local))),
        lexical_check(dialect, &t.lex, lexical)?,
    ))
}

fn has_timezone<D: SqlDialect + ?Sized>(dialect: &D, t: &TermExpr) -> Result<Expr, SqlCompileError> {
    dialect.regex_match(t.lex.clone(), r"(Z|[+-][0-9]{2}:[0-9]{2})$", None)
}

fn rank(t: &TermExpr) -> Expr {
    case(
        vec![(t.is_kind(IRI), number(0)), (t.is_kind(BLANK), number(1))],
        number(2),
    )
}

fn boolean_value(t: &TermExpr) -> Expr {
    in_list(t.lex.clone(), vec![string("true"), string("1")])
}

/// `a op b` under the order the native engine uses (`Object::sparql_compare`
/// over `ConcreteLiteral::sparql_compare`): a boolean, or `NULL` when the two
/// terms are incomparable.
///
/// - Terms of different kinds order IRI < blank node < literal; two IRIs or two
///   blank nodes compare by their text.
/// - Literals compare by value when both are numeric (any XSD numeric type, by
///   value across types, as SPARQL does), both strings (`xsd:string` or
///   `rdf:langString`, lexically), both `xsd:dateTime` (chronologically) or both
///   `xsd:boolean`; two literals of one other datatype compare lexically; any
///   ill-formed literal of a checked datatype, and every other pair, is
///   incomparable.
pub fn compare_terms<D: SqlDialect + ?Sized>(
    dialect: &D,
    a: &TermExpr,
    op: BinaryOperator,
    b: &TermExpr,
) -> Result<Expr, SqlCompileError> {
    let num = |t: &TermExpr| dialect.try_cast(t.lex.clone(), CastTarget::Double);
    let time = |t: &TermExpr| dialect.try_cast(t.lex.clone(), CastTarget::TimestampTz);
    let branches = vec![
        (
            not_eq(a.kind.clone(), b.kind.clone()),
            compare(rank(a), op.clone(), rank(b)),
        ),
        (
            a.is_not_kind(LITERAL),
            compare(a.lex.clone(), op.clone(), b.lex.clone()),
        ),
        (
            and(is_numeric_literal(dialect, a)?, is_numeric_literal(dialect, b)?),
            compare(num(a), op.clone(), num(b)),
        ),
        (
            and(is_string_literal(a), is_string_literal(b)),
            compare(a.lex.clone(), op.clone(), b.lex.clone()),
        ),
        // Two dateTimes compare chronologically when both or neither carry a
        // timezone; a mixed pair is taken as incomparable (XSD orders it only
        // when the two lie more than 14 hours apart).
        (
            and_all([
                has_datatype(dialect, a, "dateTime", Lexical::DateTime)?,
                has_datatype(dialect, b, "dateTime", Lexical::DateTime)?,
                eq(has_timezone(dialect, a)?, has_timezone(dialect, b)?),
            ]),
            compare(time(a), op.clone(), time(b)),
        ),
        (
            and(
                eq(a.datatype.clone(), string(&xsd("dateTime"))),
                eq(b.datatype.clone(), string(&xsd("dateTime"))),
            ),
            null(),
        ),
        (
            and(
                has_datatype(dialect, a, "boolean", Lexical::Boolean)?,
                has_datatype(dialect, b, "boolean", Lexical::Boolean)?,
            ),
            compare(boolean_value(a), op.clone(), boolean_value(b)),
        ),
        (
            crate::validator::sql::ast::or(
                in_list(a.datatype.clone(), checked_datatypes()),
                in_list(b.datatype.clone(), checked_datatypes()),
            ),
            null(),
        ),
        (
            eq(a.datatype.clone(), b.datatype.clone()),
            compare(a.lex.clone(), op, b.lex.clone()),
        ),
    ];
    Ok(case(branches, null()))
}
