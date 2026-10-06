//! The SQL engine: SHACL Core compiled to set-based SQL.
//!
//! The native and SPARQL engines walk the data node by node (the SPARQL one
//! asks one `ASK` per value node). This one compiles the whole shapes graph,
//! once, into relational queries — one `SELECT` per shape, constraint
//! component and context — that a host runs on its own engine over its own
//! tables. One semantics, a second execution strategy: the W3C suite holds the
//! reports of both engines equal.
//!
//! ```text
//! compile_sql(&IRSchema, &impl RelationalMapping, &impl SqlDialect) -> SqlPlan
//! SqlPlan::execute(&impl SqlExecutor) -> rows       (the host's engine)
//! SqlPlan::report(&IRSchema, rows) -> ValidationReport
//! ```
//!
//! - [`RelationalMapping`] says where the RDF terms live: [`TripleTable`] for
//!   arbitrary RDF in one `(s, p, o)` table, [`Tables`] for ordinary tables
//!   described by an RML mapping ([`Tables::from_rml`]).
//! - [`SqlDialect`] holds what differs between engines; [`DuckDb`] is the
//!   first.
//! - The queries are `sqlparser` ASTs, never text; a host renders them.
//! - [`SqlExecutor`] is implemented by the host: rudof links no engine. A
//!   DuckDB one exists behind the native-only `duckdb` feature.
//!
//! Everything SHACL Core defines compiles ([`COVERAGE`]). What does not is
//! **refused** when the plan is compiled, never skipped: recursive shapes
//! (their semantics is undefined), SHACL-SPARQL, and the SHACL 1.2
//! `sh:targetWhere` and reifier shapes.
//!
//! The module builds for wasm: it depends on no engine, thread or I/O.

mod ast;
mod coverage;
mod dialect;
#[cfg(all(feature = "duckdb", not(target_family = "wasm")))]
mod duckdb_host;
mod mapping;
mod plan;
mod render;
mod rml;
mod tables;
mod term;
mod triple_table;

pub use coverage::{COVERAGE, Coverage};
pub use dialect::{CastTarget, DuckDb, SqlDialect, SqlDialectName};
#[cfg(all(feature = "duckdb", not(target_family = "wasm")))]
pub use duckdb_host::{DuckDbExecutor, validate_with_duckdb};
pub use mapping::{PREDICATE_COLUMN, PredicateRel, Relation, RelationalMapping, SqlMapping};
pub use plan::{Row, SqlCheck, SqlExecutor, SqlPlan, SqlRowError, SqlRunError};
pub use render::RESULT_COLUMNS;
/// The SQL AST crate the extension traits ([`RelationalMapping`],
/// [`SqlDialect`]) speak: implementing either means building its AST, so
/// their signatures follow its versions. The host-facing surface ([`SqlPlan`],
/// [`SqlExecutor`]) is text and does not.
pub use sqlparser;
pub use tables::Tables;
pub use term::{EncodedTerm, decode, encode};
pub use triple_table::{TRIPLE_TABLE_COLUMNS, TripleTable};

use crate::algebra::{DenoteError, denote};
use crate::ir::IRSchema;
use render::Renderer;

/// Why a shapes graph does not compile to SQL.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SqlCompileError {
    /// A feature the engine refuses rather than skip.
    #[error("the SQL engine does not compile {0}")]
    Unsupported(String),
    /// The shapes refer to themselves; SHACL leaves their semantics undefined.
    #[error("recursive shapes are refused (their SHACL semantics is undefined): {0}")]
    RecursiveShapes(String),
    #[error("malformed target: {0}")]
    MalformedTarget(String),
    #[error("invalid relational mapping: {0}")]
    Mapping(String),
    /// An RML term outside the subset the engine reads, named first.
    #[error("the SQL engine does not read the RML term {0}")]
    UnsupportedRml(String),
    #[error("invalid data: {0}")]
    Data(String),
    #[error("internal error of the SQL compiler: {0}")]
    Internal(String),
}

impl From<DenoteError> for SqlCompileError {
    fn from(e: DenoteError) -> Self {
        match e {
            DenoteError::Unsupported(m) => SqlCompileError::Unsupported(m),
            DenoteError::RecursiveShapes(m) => SqlCompileError::RecursiveShapes(m),
            DenoteError::MalformedTarget(m) => SqlCompileError::MalformedTarget(m),
            DenoteError::Internal(m) => SqlCompileError::Internal(m),
        }
    }
}

/// Compiles `schema` into the SQL checks that find its validation results in
/// the tables `mapping` describes, written for `dialect`.
///
/// Shapes are compiled in the order of the dependency graph's levels, a shape
/// after the shapes it refers to. A shape with targets yields the checks of
/// its components and of its property shapes; a shape without targets is
/// reached only through another (as a property shape, or by `sh:node`,
/// `sh:and`, …). Deactivated shapes yield nothing and conform everywhere.
pub fn compile_sql<M, D>(schema: &IRSchema, mapping: &M, dialect: &D) -> Result<SqlPlan, SqlCompileError>
where
    M: RelationalMapping + ?Sized,
    D: SqlDialect + ?Sized,
{
    let plan = denote(schema)?;
    let mut renderer = Renderer::new(&plan, mapping, dialect);
    let mut out = SqlPlan::default();
    for check in &plan.checks {
        out.checks.push(SqlCheck {
            shape: check.shape,
            component: check.component.clone(),
            severity: check.severity.clone(),
            path: check.path.clone(),
            sql: dialect.render(&renderer.check(check.rows)?),
            parameters: check.parameters.clone(),
        });
    }
    Ok(out)
}
