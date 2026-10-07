//! An in-process DuckDB engine for the SQL interpretation (feature `duckdb`, native only).
//!
//! rudof's library links no engine: this module exists so that the W3C suite
//! and the differential tests can run every fixture through the SQL engine. A
//! browser host implements [`SqlEngine`] over its own DuckDB.

use crate::validator::sql::term::encode;
use crate::validator::sql::triples::COLUMNS;
use crate::validator::sql::{RESULT_COLUMNS, Row, SqlCompileError, SqlEngine};
use duckdb::{Connection, appender_params_from_iter};
use rudof_iri::IriS;
use rudof_rdf::NeighsRDF;
use rudof_rdf::term::Triple;

/// A DuckDB connection as a [`SqlEngine`].
pub struct DuckDbEngine {
    connection: Connection,
}

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
        let [s_type, s_value, _, _] = encode(&S::subject_as_term(&subject))?;
        let predicate: IriS = predicate.into();
        let [o_type, o_value, o_datatype, o_lang] = encode(&object)?;
        rows.push([
            s_type,
            s_value,
            predicate.as_str().to_owned(),
            o_type,
            o_value,
            o_datatype,
            o_lang,
        ]);
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

impl DuckDbEngine {
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

    /// Creates the triples table `table` (unquoted, in the default schema)
    /// and loads `store` into it.
    pub fn load_triples<S>(&self, table: &str, store: &S) -> Result<(), DuckDbLoadError>
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
        Ok(())
    }
}

impl SqlEngine for DuckDbEngine {
    type Error = duckdb::Error;

    async fn execute(&self, sql: &str) -> Result<(), Self::Error> {
        self.connection.execute_batch(sql)
    }

    async fn rows(&self, sql: &str) -> Result<Vec<Row>, Self::Error> {
        let mut statement = self.connection.prepare(sql)?;
        let rows = statement.query_map([], |row| {
            (0..RESULT_COLUMNS.len())
                .map(|i| row.get::<_, Option<String>>(i))
                .collect::<Result<Row, _>>()
        })?;
        rows.collect()
    }
}
