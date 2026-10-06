#[cfg(not(target_family = "wasm"))]
use crate::common::{Manifest, TestSuiteError};
#[cfg(not(target_family = "wasm"))]
use shacl::error::IRError;
#[cfg(not(target_family = "wasm"))]
use shacl::validator::report::ValidationReport;
#[cfg(not(target_family = "wasm"))]
use rudof_rdf::backend::OxigraphInMemory;
#[cfg(not(target_family = "wasm"))]
use std::path::Path;

/// One W3C test per line, run through both interpretations of the plan: an
/// `eval` and a `sql` module of `#[test]`s over the same fixtures. The SQL run
/// loads each data graph into an in-memory DuckDB as a triple table. Each must
/// produce a report equal to the expected one (the equality ignores messages).
#[cfg(not(target_family = "wasm"))]
macro_rules! w3c_tests {
    ($dir:literal; $($name:ident => $file:literal),* $(,)?) => {
        mod eval {
            $(
                #[test]
                fn $name() -> Result<(), crate::common::TestSuiteError> {
                    crate::test(format!("{}{}.ttl", $dir, $file), crate::Interpretation::Eval)
                }
            )*
        }

        mod sql {
            $(
                #[test]
                fn $name() -> Result<(), crate::common::TestSuiteError> {
                    crate::test(format!("{}{}.ttl", $dir, $file), crate::Interpretation::Sql)
                }
            )*
        }
    };
}

#[cfg(not(target_family = "wasm"))]
mod common;
#[cfg(not(target_family = "wasm"))]
mod core;
#[cfg(not(target_family = "wasm"))]
mod shacl12;

/// How a test validates its data graph.
#[cfg(not(target_family = "wasm"))]
#[derive(Clone, Copy)]
enum Interpretation {
    Eval,
    Sql,
}

#[cfg(not(target_family = "wasm"))]
fn test(path: String, interpretation: Interpretation) -> Result<(), TestSuiteError> {
    let mut manifest = Manifest::new(Path::new(&path))?;
    for test in manifest.collect_tests()? {
        let shapes = test
            .shapes
            .try_into()
            .map_err(|e: IRError| TestSuiteError::TestShapesCompilation(e.to_string()))?;
        let data: &OxigraphInMemory = &test.data;
        let report: ValidationReport = match interpretation {
            Interpretation::Eval => shacl::validator::validate(&shapes, data).map_err(|e| e.to_string()),
            Interpretation::Sql => shacl::validator::sql::validate_with_duckdb(data, &shapes),
        }
        .map_err(TestSuiteError::Validation)?;
        if report != test.report {
            println!("Expected report:\n{:#?}", test.report.results());
            println!("Actual report:\n{:#?}", report.results());
            return Err(TestSuiteError::NotEquals);
        }
    }
    Ok(())
}
