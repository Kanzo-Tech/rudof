//! The SQL interpretation of the [algebra](crate::algebra): the plan of a
//! shapes graph rendered as a script, a temporary table per relation the
//! checks share and then one query, a `UNION ALL` of a `SELECT` per shape,
//! constraint component and context, that a host runs on its own engine over
//! its own tables. The in-memory evaluator ([`crate::validator::eval`]) reads
//! the same plan, and the W3C suite holds the two reports equal.
//!
//! ```text
//! compile(&IRSchema, &SqlMapping, SqlDialect) -> SqlPlan
//! SqlPlan::validate(&impl SqlExecutor) -> ValidationReport   (a synchronous host)
//! SqlPlan::report(rows) -> ValidationReport                  (a host that runs the script itself)
//! ```
//!
//! - [`SqlMapping`] says where the RDF terms live: one `(s, p, o)` table for
//!   arbitrary RDF, or ordinary tables described by an RML mapping.
//! - [`SqlDialect`] names the engine the SQL is written for.
//! - The plan is text: the compiler builds `sqlparser` ASTs and renders them
//!   for the dialect, so a host needs no SQL AST.
//! - [`SqlExecutor`] is implemented by the host: rudof links no engine. A
//!   DuckDB one, [`DuckDbExecutor`], exists behind the native-only `duckdb`
//!   feature.
//!
//! Everything SHACL Core defines compiles (`coverage.rs` holds the list
//! against the Recommendation), and `sh:targetWhere`.
//! What the algebra does not denote is **refused**, never skipped: recursive
//! shapes (their semantics is undefined) and SHACL-SPARQL.
//!
//! The module builds for wasm: it depends on no engine, thread or I/O.

mod ast;
#[cfg(test)]
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

pub use dialect::SqlDialect;
#[cfg(all(feature = "duckdb", not(target_family = "wasm")))]
pub use duckdb_host::{DuckDbExecutor, DuckDbLoadError};
pub use mapping::SqlMapping;
pub use plan::{Row, SqlExecutor, SqlPlan, SqlRowError, SqlRunError};
pub use render::RESULT_COLUMNS;

use crate::algebra::{DenoteError, denote};
use crate::ir::IRSchema;
use dialect::{Dialect, DuckDb};
use mapping::RelationalMapping;
use render::Renderer;
use sqlparser::ast::helpers::stmt_create_table::CreateTableBuilder;
use sqlparser::ast::{Ident, ObjectName, ObjectType, Statement};
use tables::Tables;
use triple_table::TripleTable;

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

/// Compiles `schema` into the SQL script that finds its validation results
/// in the tables `mapping` describes, written for `dialect`.
///
/// A shape with targets yields the checks of its components and of its
/// property shapes; a shape without targets is reached only through another
/// (as a property shape, or by `sh:node`, `sh:and`, …). Deactivated shapes
/// yield nothing and conform everywhere.
pub fn compile(schema: &IRSchema, mapping: &SqlMapping, dialect: SqlDialect) -> Result<SqlPlan, SqlCompileError> {
    match dialect {
        SqlDialect::DuckDb => match mapping {
            SqlMapping::TripleTable { table } => script(schema, &TripleTable::new(table)?, &DuckDb),
            SqlMapping::Rml {
                mapping,
                schema: db_schema,
            } => script(
                schema,
                &Tables::from_rml(mapping, db_schema.as_deref(), DuckDb)?,
                &DuckDb,
            ),
        },
    }
}

fn script<M, D>(schema: &IRSchema, mapping: &M, dialect: &D) -> Result<SqlPlan, SqlCompileError>
where
    M: RelationalMapping,
    D: Dialect,
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
        schema: schema.clone(),
    })
}
