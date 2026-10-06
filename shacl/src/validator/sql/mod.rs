//! The SQL interpretation of the [algebra](crate::algebra): the plan of a
//! shapes graph rendered as a script, a temporary table per relation the
//! checks share and then one query, a `UNION ALL` of a `SELECT` per shape,
//! constraint component and context, that a host runs on its own engine over
//! its own tables. The in-memory evaluator ([`crate::validator::eval`]) reads
//! the same plan, and the W3C suite holds the two reports equal.
//!
//! ```text
//! compile_sql(&IRSchema, &impl RelationalMapping, &impl SqlDialect) -> SqlPlan
//! SqlPlan::execute(&impl SqlExecutor) -> rows       (the host's engine: setup, query, teardown)
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
//! Everything SHACL Core defines compiles ([`COVERAGE`]), and `sh:targetWhere`.
//! What the algebra does not denote is **refused**, never skipped: recursive
//! shapes (their semantics is undefined) and SHACL-SPARQL.
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
pub use plan::{Row, SqlExecutor, SqlPlan, SqlRowError, SqlRunError};
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
use sqlparser::ast::helpers::stmt_create_table::CreateTableBuilder;
use sqlparser::ast::{Ident, ObjectName, ObjectType, Statement};

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

/// Compiles `schema` into the SQL statement that finds its validation results in
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
    let (tables, query) = Renderer::new(&plan, mapping, dialect).script(&plan.checks)?;
    let table = |name: &str| ObjectName::from(vec![Ident::with_quote('"', name)]);
    Ok(SqlPlan {
        setup: tables
            .iter()
            .map(|(name, body)| {
                let create = CreateTableBuilder::new(table(name))
                    .temporary(true)
                    .query(Some(Box::new(body.clone())))
                    .build();
                dialect.render(&Statement::CreateTable(create))
            })
            .collect(),
        query: dialect.render(&Statement::Query(Box::new(query))),
        teardown: tables
            .iter()
            .rev()
            .map(|(name, _)| {
                dialect.render(&Statement::Drop {
                    object_type: ObjectType::Table,
                    if_exists: true,
                    names: vec![table(name)],
                    cascade: false,
                    restrict: false,
                    purge: false,
                    temporary: false,
                    table: None,
                })
            })
            .collect(),
        checks: plan.checks,
    })
}
