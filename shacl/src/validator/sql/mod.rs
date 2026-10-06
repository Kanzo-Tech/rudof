//! The SQL interpretation of the [algebra](crate::algebra): the plan of a
//! shapes graph rendered as a script, a temporary table per relation the
//! checks share and then one query, a `UNION ALL` of a `SELECT` per shape,
//! constraint component and context, run on the host's engine over one
//! `(s, p, o)` relation. The in-memory evaluator ([`crate::validator::eval`])
//! reads the same plan, and the W3C suite holds the two reports equal.
//!
//! ```text
//! validate(&IRSchema, triples: &str, &impl SqlEngine).await -> ValidationReport
//! ```
//!
//! - `triples` names the relation the data is read from, columns
//!   `s_k, s_v, p, o_k, o_v, o_d, o_l` ([`triples`](self) has them). A table
//!   or a view: what tables lie under it is the host's, so the engine knows no
//!   mapping language and no product vocabulary.
//! - [`SqlEngine`] is implemented by the host: rudof links no engine. A
//!   DuckDB one, [`DuckDbEngine`], exists behind the native-only `duckdb`
//!   feature, for the tests.
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
mod plan;
mod render;
mod term;
mod triples;

#[cfg(all(feature = "duckdb", not(target_family = "wasm")))]
pub use duckdb_host::{DuckDbEngine, DuckDbLoadError};
pub use plan::{Row, SqlEngine, SqlError};
pub use render::RESULT_COLUMNS;

use crate::algebra::{DenoteError, denote};
use crate::ir::IRSchema;
use crate::validator::report::ValidationReport;
use dialect::{Dialect, DuckDb};
use plan::SqlPlan;
use render::Renderer;
use sqlparser::ast::helpers::stmt_create_table::CreateTableBuilder;
use sqlparser::ast::{Ident, ObjectName, ObjectType, Statement};
use triples::Triples;

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
    /// The triples relation's name is not a SQL object name.
    #[error("the triples relation {0}")]
    Table(String),
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

/// Validates the data in the relation `triples` against `schema` on
/// `engine`: the script of the plan, run on one connection, and the report of
/// its rows, worded as the in-memory evaluator words its own.
///
/// A shape with targets yields the checks of its components and of its
/// property shapes; a shape without targets is reached only through another
/// (as a property shape, or by `sh:node`, `sh:and`, …). Deactivated shapes
/// yield nothing and conform everywhere.
pub async fn validate<E: SqlEngine>(
    schema: &IRSchema,
    triples: &str,
    engine: &E,
) -> Result<ValidationReport, SqlError<E::Error>> {
    compile(schema, &Triples::new(triples)?, &DuckDb)?.run(engine).await
}

fn compile<D: Dialect>(schema: &IRSchema, triples: &Triples, dialect: &D) -> Result<SqlPlan, SqlCompileError> {
    let plan = denote(schema)?;
    let (tables, query) = Renderer::new(&plan, triples, dialect).script(&plan.checks)?;
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
