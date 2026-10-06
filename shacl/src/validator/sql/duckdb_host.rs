//! An in-process DuckDB host for the SQL engine (feature `duckdb`, native only).
//!
//! rudof's library links no engine: this module exists so that the CLI can
//! offer `--mode sql` and the W3C suite can run every fixture through the SQL
//! engine. A browser host implements [`SqlExecutor`] over its own DuckDB.

use crate::ir::IRSchema;
use crate::validator::report::ValidationReport;
use crate::validator::sql::{DuckDb, RESULT_COLUMNS, Row, SqlExecutor, TRIPLE_TABLE_COLUMNS, TripleTable, compile_sql};
use duckdb::{Connection, appender_params_from_iter};
use rudof_rdf::NeighsRDF;
use std::fmt::Display;

/// Runs a plan's checks on a DuckDB connection.
pub struct DuckDbExecutor {
    connection: Connection,
}

impl DuckDbExecutor {
    pub fn new(connection: Connection) -> Self {
        Self { connection }
    }

    /// An in-memory database.
    pub fn in_memory() -> Result<Self, duckdb::Error> {
        Ok(Self::new(Connection::open_in_memory()?))
    }

    pub fn connection(&self) -> &Connection {
        &self.connection
    }

    /// Creates the triple table `table` and loads `store` into it.
    pub fn load_triples<S>(&self, table: &str, store: &S) -> Result<TripleTable, String>
    where
        S: NeighsRDF<Term = oxrdf::Term>,
    {
        let rows = TripleTable::rows(store).map_err(|e| e.to_string())?;
        let columns = TRIPLE_TABLE_COLUMNS
            .iter()
            .map(|c| format!("\"{c}\" VARCHAR NOT NULL"))
            .collect::<Vec<_>>()
            .join(", ");
        self.connection
            .execute_batch(&format!("CREATE TABLE \"{table}\" ({columns})"))
            .map_err(|e| e.to_string())?;
        let mut appender = self.connection.appender(table).map_err(|e| e.to_string())?;
        for row in &rows {
            appender
                .append_row(appender_params_from_iter(row.iter()))
                .map_err(|e| e.to_string())?;
        }
        appender.flush().map_err(|e| e.to_string())?;
        TripleTable::new(&format!("\"{table}\"")).map_err(|e| e.to_string())
    }
}

impl SqlExecutor for DuckDbExecutor {
    type Error = duckdb::Error;

    fn execute(&self, sql: &str) -> Result<(), Self::Error> {
        self.connection.execute_batch(sql)
    }

    fn rows(&self, sql: &str) -> Result<Vec<Row>, Self::Error> {
        let mut statement = self.connection.prepare(sql)?;
        let rows = statement.query_map([], |row| {
            (0..RESULT_COLUMNS.len())
                .map(|i| row.get::<_, Option<String>>(i))
                .collect::<Result<Row, _>>()
        })?;
        rows.collect()
    }
}

/// Validates `store` against `schema` with the SQL engine, on an in-memory
/// DuckDB holding the graph as a [`TripleTable`].
pub fn validate_with_duckdb<S>(store: &S, schema: &IRSchema) -> Result<ValidationReport, String>
where
    S: NeighsRDF<Term = oxrdf::Term>,
{
    fn text(e: impl Display) -> String {
        e.to_string()
    }
    let executor = DuckDbExecutor::in_memory().map_err(text)?;
    let mapping = executor.load_triples("triples", store)?;
    let plan = compile_sql(schema, &mapping, &DuckDb).map_err(text)?;
    let report = plan.validate(schema, &executor).map_err(text)?;
    let mut pm = schema.prefix_map().clone();
    if let Some(store_pm) = store.prefixmap() {
        pm.merge(store_pm);
    }
    Ok(report.with_prefixmap(pm))
}
