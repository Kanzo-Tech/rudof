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
//!   described by an R2RML-like DTO ([`TablesSpec`]).
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
mod component;
mod context;
mod coverage;
mod dialect;
#[cfg(all(feature = "duckdb", not(target_family = "wasm")))]
mod duckdb_host;
mod mapping;
mod path;
mod plan;
mod shape;
mod target;
mod term;

pub use context::RESULT_COLUMNS;
pub use coverage::{COVERAGE, Coverage};
pub use dialect::{CastTarget, DuckDb, SqlDialect};
#[cfg(all(feature = "duckdb", not(target_family = "wasm")))]
pub use duckdb_host::{DuckDbExecutor, validate_with_duckdb};
pub use mapping::{
    ClassMap, PREDICATE_COLUMN, PredicateRel, PropertyMap, Relation, RelationalMapping, SqlMapping, SubClassOf,
    TRIPLE_TABLE_COLUMNS, Tables, TablesSpec, TermMap, TermType, TripleTable,
};
pub use plan::{Row, SqlCheck, SqlExecutor, SqlPlan, SqlRowError, SqlRunError};
pub use term::{EncodedTerm, decode, encode};

use crate::ir::{IRComponent, IRSchema, IRShape};
use crate::types::Target;
use context::Ctx;
use plan::Parameters;
use shape::ShapeCompiler;
use target::TargetCompiler;

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
    #[error("invalid data: {0}")]
    Data(String),
    #[error("internal error of the SQL compiler: {0}")]
    Internal(String),
}

/// The SHACL 1.2 and SHACL-SPARQL features of `shape` the engine refuses.
fn refuse(shape: &IRShape) -> Result<(), SqlCompileError> {
    if shape.targets().iter().any(|t| matches!(t, Target::Where(_))) {
        return Err(SqlCompileError::Unsupported(format!(
            "sh:targetWhere (SHACL 1.2) on {}",
            shape.id()
        )));
    }
    if shape.reifier_info().is_some() {
        return Err(SqlCompileError::Unsupported(format!(
            "sh:reifierShape (SHACL 1.2) on {}",
            shape.id()
        )));
    }
    if shape
        .components()
        .iter()
        .any(|c| matches!(c, IRComponent::BasicSparql(_)))
    {
        return Err(SqlCompileError::Unsupported(format!(
            "sh:sparql (SHACL-SPARQL is outside SHACL Core) on {}",
            shape.id()
        )));
    }
    Ok(())
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
    let graph = schema.dependency_graph();
    if graph.has_cycles() {
        return Err(SqlCompileError::RecursiveShapes(format!("{graph}")));
    }
    for (_, shape) in schema.iter() {
        refuse(shape)?;
    }

    let mut plan = SqlPlan::default();
    for level in schema.shapes_with_targets_by_level() {
        for idx in level {
            let shape = schema
                .get_shape_from_idx(&idx)
                .ok_or_else(|| SqlCompileError::Internal(format!("shape {idx} is not in the schema")))?;
            if shape.deactivated() {
                continue;
            }
            let mut ctx = Ctx::new(schema, mapping, dialect);
            let focus = TargetCompiler::focus(&mut ctx, shape)?;
            let mut pending = Vec::new();
            ShapeCompiler::emit(&mut ctx, idx, &focus, false, &mut pending)?;
            for p in pending {
                let owner = schema
                    .get_shape_from_idx(&p.shape)
                    .ok_or_else(|| SqlCompileError::Internal(format!("shape {} is not in the schema", p.shape)))?;
                plan.checks.push(SqlCheck {
                    shape: p.shape,
                    component: p.rows.component.clone(),
                    severity: owner.severity().clone(),
                    path: owner.path().cloned(),
                    query: ctx.finish(&p.rows.rows)?,
                    parameters: match p.rows.parameters {
                        Some(own) => Parameters::Own(own),
                        None => Parameters::Component(p.component),
                    },
                });
            }
        }
    }
    Ok(plan)
}
