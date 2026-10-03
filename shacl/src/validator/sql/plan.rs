//! The compiled plan, the host's executor, and the report built from rows.

use crate::ir::{IRSchema, ShapeLabelIdx};
use crate::types::Severity;
use crate::validator::constraints::{component_parameters, result_message};
use crate::validator::report::{ValidationReport, ValidationResult};
use crate::validator::sql::context::RESULT_COLUMNS;
use crate::validator::sql::term::decode;
use rudof_iri::IriS;
use rudof_rdf::SHACLPath;
use rudof_rdf::term::Object;
use sqlparser::ast::Query;
use std::fmt::Display;

/// One row of a check, in [`RESULT_COLUMNS`] order: the focus term, the value
/// term (all `NULL` when the result has none) and the path override.
pub type Row = Vec<Option<String>>;

/// Runs a check's query. Hosts implement it over their engine; rudof links none.
pub trait SqlExecutor {
    type Error: Display;

    /// Every row of `query`, each in [`RESULT_COLUMNS`] order, as text.
    fn rows(&self, query: &Query) -> Result<Vec<Row>, Self::Error>;
}

/// One `SELECT` of a plan: the failing rows of one constraint component of
/// one shape, in one context (a targeted shape, or a property shape reached
/// from one).
#[derive(Debug, Clone)]
pub struct SqlCheck {
    /// The shape that declares the component: the results' `sh:sourceShape`.
    pub shape: ShapeLabelIdx,
    /// The results' `sh:sourceConstraintComponent`.
    pub component: IriS,
    /// The results' `sh:resultSeverity`.
    pub severity: Severity,
    /// The results' `sh:resultPath`, unless a row overrides it (`sh:closed`).
    pub path: Option<SHACLPath>,
    /// The query, columns [`RESULT_COLUMNS`].
    pub query: Query,
    /// The index of the component in the shape (for its message parameters),
    /// or the parameters it names itself.
    pub(crate) parameters: Parameters,
}

#[derive(Debug, Clone)]
pub(crate) enum Parameters {
    Component(usize),
    Own(Vec<(&'static str, String)>),
}

impl SqlCheck {
    /// The text of the query, as the default dialect renders it.
    pub fn sql(&self) -> String {
        self.query.to_string()
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
                    .rows(&c.query)
                    .map_err(|error| SqlRunError::Executor { check, error })
            })
            .collect()
    }

    /// The validation report of the rows of every check (`rows_by_check[i]`
    /// are the rows of `checks[i]`). Messages come from the native engine's
    /// own wording ([`result_message`]), so both engines' reports read alike.
    pub fn report(&self, schema: &IRSchema, rows_by_check: &[Vec<Row>]) -> Result<ValidationReport, SqlRowError> {
        let mut results = Vec::new();
        for (index, (check, rows)) in self.checks.iter().zip(rows_by_check).enumerate() {
            let err = |row: usize, message: String| SqlRowError {
                check: index,
                row,
                message,
            };
            let shape = schema
                .get_shape_from_idx(&check.shape)
                .ok_or_else(|| err(0, format!("shape {} is not in the schema", check.shape)))?;
            let parameters = match &check.parameters {
                Parameters::Component(i) => shape
                    .components()
                    .get(*i)
                    .map(|c| component_parameters(c, schema))
                    .unwrap_or_default(),
                Parameters::Own(own) => own.clone(),
            };
            for (r, row) in rows.iter().enumerate() {
                if row.len() != RESULT_COLUMNS.len() {
                    return Err(err(r, format!("{} columns, not {}", row.len(), RESULT_COLUMNS.len())));
                }
                let focus = term(row, 0)
                    .map_err(|m| err(r, m))?
                    .ok_or_else(|| err(r, "no focus node".to_owned()))?;
                let value = term(row, 4).map_err(|m| err(r, m))?;
                let path = match cell(row, 8) {
                    Some(p) => Some(SHACLPath::iri(IriS::new_unchecked(p))),
                    None => check.path.clone(),
                };
                let message = result_message(schema, shape, &check.component, &parameters, value.as_ref());
                results.push(
                    ValidationResult::new(focus, Object::Iri(check.component.clone()), check.severity.clone())
                        .with_source(Some(shape.id().clone()))
                        .with_message(message)
                        .with_path(path)
                        .with_value(value),
                );
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
