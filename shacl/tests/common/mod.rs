//! Validation through the SQL engine on an in-memory DuckDB, the graph loaded
//! into a triple table: how the tests run a fixture through SQL.

use rudof_rdf::backend::OxigraphInMemory;
use shacl::ir::IRSchema;
use shacl::validator::report::ValidationReport;
use shacl::validator::sql::{DuckDbExecutor, SqlDialect, compile};

pub fn validate_with_duckdb(data: &OxigraphInMemory, schema: &IRSchema) -> Result<ValidationReport, String> {
    let executor = DuckDbExecutor::in_memory().map_err(|e| e.to_string())?;
    let mapping = executor.load_triples("triples", data).map_err(|e| e.to_string())?;
    let plan = compile(schema, &mapping, SqlDialect::DuckDb).map_err(|e| e.to_string())?;
    plan.validate(&executor).map_err(|e| e.to_string())
}
