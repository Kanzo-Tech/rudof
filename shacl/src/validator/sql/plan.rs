//! The compiled plan, the host's executor, and the report built from rows.

use crate::algebra::Check;
use crate::ir::IRSchema;
use crate::validator::report::{ValidationReport, ValidationResult};
use crate::validator::sql::render::RESULT_COLUMNS;
use crate::validator::sql::term::decode;
use rudof_iri::IriS;
use rudof_rdf::term::Object;
use std::fmt::Display;

/// One row of a check, in [`RESULT_COLUMNS`] order: the focus term, the value
/// term (all `NULL` when the result has none) and the path override.
pub type Row = Vec<Option<String>>;

/// Runs a check's query. Hosts implement it over their engine; rudof links none.
///
/// The query arrives as text, rendered for the plan's dialect: a host needs no
/// SQL AST, and the plan's public surface does not tie rudof's semver to the
/// AST crate's.
pub trait SqlExecutor {
    type Error: Display;

    /// Every row of `sql`, each in [`RESULT_COLUMNS`] order, as text.
    fn rows(&self, sql: &str) -> Result<Vec<Row>, Self::Error>;
}

/// One `SELECT` of a plan: the failing rows of one constraint component of
/// one shape, in one context (a targeted shape, or a property shape reached
/// from one).
#[derive(Debug, Clone)]
pub struct SqlCheck {
    /// What every row of the query reports: shape, component, severity, path.
    pub check: Check,
    /// The query as its dialect renders it; its columns are [`RESULT_COLUMNS`].
    pub(crate) sql: String,
}

impl SqlCheck {
    /// The query, as the plan's dialect renders it.
    pub fn sql(&self) -> &str {
        &self.sql
    }
}

/// A row that cannot be read back as a result.
#[derive(Debug, thiserror::Error)]
#[error("row {row} of check {check}: {message}")]
pub struct SqlRowError {
    pub check: usize,
    pub row: usize,
    pub message: String,
}

/// Running a plan: the executor failed, or a row was malformed.
#[derive(Debug, thiserror::Error)]
pub enum SqlRunError<E: Display> {
    #[error("executing check {check}: {error}")]
    Executor { check: usize, error: E },
    #[error(transparent)]
    Row(#[from] SqlRowError),
}

/// What [`compile_sql`](crate::validator::sql::compile_sql) produces: a
/// `SELECT` per shape, component and context, whose rows are the results.
#[derive(Debug, Clone, Default)]
pub struct SqlPlan {
    pub checks: Vec<SqlCheck>,
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
    /// Runs every check through `executor`.
    pub fn execute<X: SqlExecutor>(&self, executor: &X) -> Result<Vec<Vec<Row>>, SqlRunError<X::Error>> {
        self.checks
            .iter()
            .enumerate()
            .map(|(check, c)| {
                executor
                    .rows(&c.sql)
                    .map_err(|error| SqlRunError::Executor { check, error })
            })
            .collect()
    }

    /// The validation report of the rows of every check (`rows_by_check[i]`
    /// are the rows of `checks[i]`), built by [`ValidationResult::of`] as the
    /// in-memory evaluator builds its own, so both reports read alike.
    pub fn report(&self, schema: &IRSchema, rows_by_check: &[Vec<Row>]) -> Result<ValidationReport, SqlRowError> {
        // One row set per check: a missing set is not an empty one, and
        // reading it as such would report conformance for checks never run.
        if rows_by_check.len() != self.checks.len() {
            return Err(SqlRowError {
                check: rows_by_check.len().min(self.checks.len()),
                row: 0,
                message: format!(
                    "{} row sets for {} checks: every check needs its rows, empty or not",
                    rows_by_check.len(),
                    self.checks.len()
                ),
            });
        }
        let mut results = Vec::new();
        for (index, (check, rows)) in self.checks.iter().zip(rows_by_check).enumerate() {
            let err = |row: usize, message: String| SqlRowError {
                check: index,
                row,
                message,
            };
            for (r, row) in rows.iter().enumerate() {
                if row.len() != RESULT_COLUMNS.len() {
                    return Err(err(r, format!("{} columns, not {}", row.len(), RESULT_COLUMNS.len())));
                }
                let focus = term(row, 0)
                    .map_err(|m| err(r, m))?
                    .ok_or_else(|| err(r, "no focus node".to_owned()))?;
                let value = term(row, 4).map_err(|m| err(r, m))?;
                let path = cell(row, 8).map(IriS::new_unchecked);
                results.push(ValidationResult::of(schema, &check.check, focus, value, path).map_err(|m| err(r, m))?);
            }
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
