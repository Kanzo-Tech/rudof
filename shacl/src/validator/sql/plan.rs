//! The compiled script, the host's engine, and the report built from rows.

use crate::algebra::{Check, Unchecked};
use crate::ir::IRSchema;
use crate::validator::report::{ValidationReport, ValidationResult};
use crate::validator::sql::SqlCompileError;
use crate::validator::sql::render::RESULT_COLUMNS;
use crate::validator::sql::term::decode;
use rudof_iri::IriS;
use rudof_rdf::term::Object;
use std::fmt::Display;

/// One row of the script's query, in [`RESULT_COLUMNS`] order: the index of
/// its check, the focus term, the value term (all `NULL` when the result has
/// none) and the path override.
pub type Row = Vec<Option<String>>;

/// The host's SQL engine; rudof links none.
///
/// Asynchronous, because the engine a browser holds (DuckDB-WASM) answers
/// across the event loop; a synchronous one answers at once. Statements
/// arrive as text, so a host needs no SQL AST. All of one validation's
/// statements run on one connection, since its tables are temporary, and a
/// connection runs one validation at a time, since their names are fixed.
#[allow(async_fn_in_trait)]
pub trait SqlEngine {
    type Error: Display;

    /// Runs `sql`, a statement without rows.
    async fn execute(&self, sql: &str) -> Result<(), Self::Error>;

    /// Every row of `sql`, each in [`RESULT_COLUMNS`] order, as text.
    async fn rows(&self, sql: &str) -> Result<Vec<Row>, Self::Error>;
}

/// Why a validation through SQL failed.
#[derive(Debug, thiserror::Error)]
pub enum SqlError<E: Display> {
    #[error(transparent)]
    Compile(#[from] SqlCompileError),
    #[error("the engine: {0}")]
    Engine(E),
    #[error("row {row}: {message}")]
    Row { row: usize, message: String },
}

/// The script of one plan over one triples relation: the setup, a `CREATE
/// TEMPORARY TABLE` per relation the plan shares, inputs first; then its
/// statement, the query whose rows are the results (they name their check by
/// index) or the `CREATE TABLE` of a fragment; then the teardown, which drops
/// those tables.
#[derive(Debug, Clone)]
pub(crate) struct SqlPlan {
    pub(crate) checks: Vec<Check>,
    pub(crate) unchecked: Vec<Unchecked>,
    pub(crate) schema: IRSchema,
    pub(crate) setup: Vec<String>,
    pub(crate) query: String,
    pub(crate) teardown: Vec<String>,
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
    /// The validation report of the query's rows, each built by
    /// [`ValidationResult::of`] for the check it names, as the in-memory
    /// evaluator builds its own, so both reports read alike.
    fn report<E: Display>(&self, rows: &[Row]) -> Result<ValidationReport, SqlError<E>> {
        let schema = &self.schema;
        let mut results = Vec::with_capacity(rows.len());
        for (r, row) in rows.iter().enumerate() {
            let err = |message: String| SqlError::Row { row: r, message };
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
            .with_unchecked(self.unchecked.clone())
            .with_prefixmap(schema.prefix_map().clone()))
    }

    /// Runs the script on `engine` and builds the report of its rows.
    pub(crate) async fn run<X: SqlEngine>(&self, engine: &X) -> Result<ValidationReport, SqlError<X::Error>> {
        let rows = self.scripted(engine, engine.rows(&self.query)).await?;
        self.report(&rows)
    }

    /// Runs the script on `engine`, its statement one without rows; the
    /// shapes the plan does not cover.
    pub(crate) async fn create<X: SqlEngine>(&self, engine: &X) -> Result<Vec<Unchecked>, SqlError<X::Error>> {
        self.scripted(engine, engine.execute(&self.query)).await?;
        Ok(self.unchecked.clone())
    }

    /// The setup, then `main`, then the teardown, which runs too when the
    /// setup or `main` fails; the first failure is the one returned.
    async fn scripted<X: SqlEngine, T>(
        &self,
        engine: &X,
        main: impl Future<Output = Result<T, X::Error>>,
    ) -> Result<T, SqlError<X::Error>> {
        let out = async {
            for statement in &self.setup {
                engine.execute(statement).await?;
            }
            main.await
        }
        .await;
        let mut dropped = Ok(());
        for statement in &self.teardown {
            if let Err(e) = engine.execute(statement).await {
                dropped = dropped.and(Err(e));
            }
        }
        let out = out.map_err(SqlError::Engine)?;
        dropped.map_err(SqlError::Engine)?;
        Ok(out)
    }
}
