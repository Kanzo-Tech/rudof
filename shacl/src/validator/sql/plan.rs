//! The compiled plan, the host's executor, and the report built from rows.

use crate::algebra::Check;
use crate::ir::IRSchema;
use crate::validator::report::{ValidationReport, ValidationResult};
use crate::validator::sql::render::RESULT_COLUMNS;
use crate::validator::sql::term::decode;
use rudof_iri::IriS;
use rudof_rdf::term::Object;
use std::fmt::Display;

/// One row of the plan's statement, in [`RESULT_COLUMNS`] order: the index of
/// its check, the focus term, the value term (all `NULL` when the result has
/// none) and the path override.
pub type Row = Vec<Option<String>>;

/// Runs the plan's statement. Hosts implement it over their engine; rudof
/// links none.
///
/// The statement arrives as text, rendered for the plan's dialect: a host
/// needs no SQL AST, and the plan's public surface does not tie rudof's semver
/// to the AST crate's.
pub trait SqlExecutor {
    type Error: Display;

    /// Every row of `sql`, each in [`RESULT_COLUMNS`] order, as text.
    fn rows(&self, sql: &str) -> Result<Vec<Row>, Self::Error>;
}

/// A row that cannot be read back as a result.
#[derive(Debug, thiserror::Error)]
#[error("row {row}: {message}")]
pub struct SqlRowError {
    pub row: usize,
    pub message: String,
}

/// Running a plan: the executor failed, or a row was malformed.
#[derive(Debug, thiserror::Error)]
pub enum SqlRunError<E: Display> {
    #[error("executing the plan: {0}")]
    Executor(E),
    #[error(transparent)]
    Row(#[from] SqlRowError),
}

/// What [`compile_sql`](crate::validator::sql::compile_sql) produces: one
/// statement whose rows are the results, and the checks they belong to (a
/// shape's constraint component in one context), by index.
#[derive(Debug, Clone)]
pub struct SqlPlan {
    /// What every row of a check reports: shape, component, severity, path.
    pub checks: Vec<Check>,
    /// The statement as its dialect renders it; its columns are
    /// [`RESULT_COLUMNS`].
    pub(crate) sql: String,
}

fn cell(row: &Row, i: usize) -> Option<&str> {
    row.get(i).and_then(|c| c.as_deref())
}

fn term(row: &Row, offset: usize) -> Result<Option<Object>, String> {
    match cell(row, offset) {
        None => Ok(None),
        Some(kind) => decode(
            kind,
            cell(row, offset + 1).unwrap_or_default(),
            cell(row, offset + 2).unwrap_or_default(),
            cell(row, offset + 3).unwrap_or_default(),
        )
        .map(Some),
    }
}

impl SqlPlan {
    /// The statement, as the plan's dialect renders it.
    pub fn sql(&self) -> &str {
        &self.sql
    }

    /// Runs the statement through `executor`.
    pub fn execute<X: SqlExecutor>(&self, executor: &X) -> Result<Vec<Row>, SqlRunError<X::Error>> {
        executor.rows(&self.sql).map_err(SqlRunError::Executor)
    }

    /// The validation report of the statement's rows, each built by
    /// [`ValidationResult::of`] for the check it names, as the in-memory
    /// evaluator builds its own, so both reports read alike.
    pub fn report(&self, schema: &IRSchema, rows: &[Row]) -> Result<ValidationReport, SqlRowError> {
        let mut results = Vec::with_capacity(rows.len());
        for (r, row) in rows.iter().enumerate() {
            let err = |message: String| SqlRowError { row: r, message };
            if row.len() != RESULT_COLUMNS.len() {
                return Err(err(format!("{} columns, not {}", row.len(), RESULT_COLUMNS.len())));
            }
            let check = cell(row, 0)
                .and_then(|c| c.parse::<usize>().ok())
                .and_then(|c| self.checks.get(c))
                .ok_or_else(|| err(format!("no check of the plan is {:?}", cell(row, 0))))?;
            let focus = term(row, 1)
                .map_err(err)?
                .ok_or_else(|| err("no focus node".to_owned()))?;
            let value = term(row, 5).map_err(err)?;
            let path = cell(row, 9).map(IriS::new_unchecked);
            results.push(ValidationResult::of(schema, check, focus, value, path).map_err(err)?);
        }
        Ok(ValidationReport::new()
            .with_results(results)
            .with_prefixmap(schema.prefix_map().clone()))
    }

    /// Runs the plan and builds its report.
    pub fn validate<X: SqlExecutor>(
        &self,
        schema: &IRSchema,
        executor: &X,
    ) -> Result<ValidationReport, SqlRunError<X::Error>> {
        let rows = self.execute(executor)?;
        Ok(self.report(schema, &rows)?)
    }
}
