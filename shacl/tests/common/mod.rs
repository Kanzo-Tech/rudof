//! Validation through the SQL engine on an in-memory DuckDB, the graph loaded
//! into a triples table: how the tests run a fixture through SQL.

use rudof_rdf::backend::OxigraphInMemory;
use shacl::ir::IRSchema;
use shacl::validator::report::ValidationReport;
use shacl::validator::sql::{DuckDbEngine, validate};

pub fn validate_with_duckdb(data: &OxigraphInMemory, schema: &IRSchema) -> Result<ValidationReport, String> {
    let engine = DuckDbEngine::in_memory().map_err(|e| e.to_string())?;
    engine.load_triples("triples", data).map_err(|e| e.to_string())?;
    futures::executor::block_on(validate(schema, "triples", None, &engine)).map_err(|e| e.to_string())
}
