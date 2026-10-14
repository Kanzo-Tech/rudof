//! `rml`: the RML-Core mapping whose logical sources are SQL tables and
//! queries.
//!
//! A mapping says what the rows of some tables mean as RDF. This crate holds
//! it as a value ([`ast`]), reads and writes it as a mapping graph
//! ([`rdf`]), tells a host what it says about its tables ([`model`]) and, with the
//! feature `sql`, renders it as a query over those tables (`sql`).
//!
//! **The reference** is RML-Core, "Draft Community Group Report, 16 March
//! 2026" (<https://w3id.org/rml/core/spec>), which "is not a W3C Standard nor
//! is it on the W3C Standards Track", with RML-IO for the logical source
//! (<https://w3id.org/rml/io/spec>). A section number in a comment is a
//! clause that was read in that text; where the specification is silent the
//! comment says so. R2RML (W3C Recommendation, 2012) is not the reference: it
//! has no per-row language tag.
//!
//! **What is not here.** A logical source is a SQL table name or a SQL query,
//! which the host resolves; the crate knows no storage layout and no product
//! vocabulary. Nothing in it reads data: `sql` produces the query and the
//! host runs it.
//!
//! The crate builds for `wasm32-unknown-unknown`.

pub mod ast;
pub mod model;
pub mod rdf;
#[cfg(feature = "sql")]
pub mod sql;
pub mod vocab;

pub use ast::{
    Annotation, Constant, Expression, Iterable, JoinCondition, LanguageMap, Literal, LiteralKind, LogicalSource,
    Mapping, Node, ObjectMap, Position, PredicateObjectMap, RefObjectMap, SubjectMap, Template, TemplatePart, TermMap,
    TermType, TriplesMap,
};

/// Why a mapping is not read, written or rendered.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// The mapping does not say what RML-Core requires of one.
    #[error("{at}: {reason}")]
    Invalid { at: String, reason: String },
    /// The mapping says something RML-Core allows and this crate does not model.
    /// It is refused: dropping it would change what the mapping means.
    #[error("{at}: {what} is not supported")]
    Unsupported { at: String, what: String },
    /// A logical source query that is not SQL the DuckDB dialect reads.
    #[error("{at}: the query is not SQL: {reason}")]
    Query { at: String, reason: String },
    /// A name the host gave for a table or a view is not a SQL object name, or the host could not resolve one.
    #[error("the table {name}: {reason}")]
    Table { name: String, reason: String },
    /// The RDF graph could not be read or written.
    #[error("the mapping graph: {0}")]
    Graph(String),
}

impl Error {
    pub(crate) fn invalid(at: &str, reason: &str) -> Self {
        Error::Invalid {
            at: at.to_owned(),
            reason: reason.to_owned(),
        }
    }

    pub(crate) fn unsupported(at: &str, what: &str) -> Self {
        Error::Unsupported {
            at: at.to_owned(),
            what: what.to_owned(),
        }
    }
}
