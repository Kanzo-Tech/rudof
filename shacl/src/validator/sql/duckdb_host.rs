//! An in-process DuckDB host for the SQL engine (feature `duckdb`, native only).
//!
//! rudof's library links no engine: this module exists so that the W3C suite
//! and the differential tests can run every fixture through the SQL engine. A
//! browser host implements [`SqlExecutor`] over its own DuckDB.

use crate::validator::sql::term::encode;
use crate::validator::sql::{RESULT_COLUMNS, Row, SqlCompileError, SqlExecutor, SqlMapping};
use duckdb::{Connection, appender_params_from_iter};
use rudof_iri::IriS;
use rudof_rdf::NeighsRDF;
use rudof_rdf::term::Triple;

/// Runs a plan's statements on a DuckDB connection.
pub struct DuckDbExecutor {
    connection: Connection,
}

/// The columns of a triple table, in order (see [`SqlMapping::TripleTable`]).
const COLUMNS: [&str; 7] = ["s_k", "s_v", "p", "o_k", "o_v", "o_d", "o_l"];

/// The rows of `store` in [`COLUMNS`] order.
fn rows<S>(store: &S) -> Result<Vec<[String; 7]>, SqlCompileError>
where
    S: NeighsRDF<Term = oxrdf::Term>,
{
    let triples = store
        .triples()
        .map_err(|e| SqlCompileError::Data(format!("reading the data graph: {e}")))?;
    let mut rows = Vec::new();
    for triple in triples {
        let (subject, predicate, object) = triple.into_components();
        let [s_k, s_v, _, _] = encode(&S::subject_as_term(&subject))?;
        let predicate: IriS = predicate.into();
        let [o_k, o_v, o_d, o_l] = encode(&object)?;
        rows.push([s_k, s_v, predicate.as_str().to_owned(), o_k, o_v, o_d, o_l]);
    }
    Ok(rows)
}

/// Why a graph does not load into a triple table.
#[derive(Debug, thiserror::Error)]
pub enum DuckDbLoadError {
    #[error(transparent)]
    Engine(#[from] duckdb::Error),
    #[error(transparent)]
    Data(#[from] SqlCompileError),
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
    pub fn load_triples<S>(&self, table: &str, store: &S) -> Result<SqlMapping, DuckDbLoadError>
    where
        S: NeighsRDF<Term = oxrdf::Term>,
    {
        let rows = rows(store)?;
        let columns = COLUMNS
            .iter()
            .map(|c| format!("\"{c}\" VARCHAR NOT NULL"))
            .collect::<Vec<_>>()
            .join(", ");
        self.connection
            .execute_batch(&format!("CREATE TABLE \"{table}\" ({columns})"))?;
        let mut appender = self.connection.appender(table)?;
        for row in &rows {
            appender.append_row(appender_params_from_iter(row.iter()))?;
        }
        appender.flush()?;
        Ok(SqlMapping::TripleTable {
            table: format!("\"{table}\""),
        })
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
