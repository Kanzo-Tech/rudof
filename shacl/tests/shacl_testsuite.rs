#[cfg(not(target_family = "wasm"))]
use crate::common::{Manifest, TestSuiteError};
#[cfg(not(target_family = "wasm"))]
use shacl::error::IRError;
#[cfg(not(target_family = "wasm"))]
use shacl::validator::ShaclValidationMode;
#[cfg(not(target_family = "wasm"))]
use shacl::validator::processor::{DataValidation, ShaclProcessor};
#[cfg(not(target_family = "wasm"))]
use std::path::Path;

/// One W3C test per line, run through every engine: a `native` and a `sql`
/// module of `#[test]`s over the same fixtures. The SQL run loads each data
/// graph into an in-memory DuckDB as a triple table, compiles the shapes with
/// that mapping, and must produce a report equal to the expected one (the
/// equality ignores messages), as the native run must.
#[cfg(not(target_family = "wasm"))]
macro_rules! w3c_tests {
    ($dir:literal; $($name:ident => $file:literal),* $(,)?) => {
        mod native {
            $(
                #[test]
                fn $name() -> Result<(), crate::common::TestSuiteError> {
                    crate::test(format!("{}{}.ttl", $dir, $file), shacl::validator::ShaclValidationMode::Native)
                }
            )*
        }

        mod sql {
            $(
                #[test]
                fn $name() -> Result<(), crate::common::TestSuiteError> {
                    crate::test(format!("{}{}.ttl", $dir, $file), shacl::validator::ShaclValidationMode::Sql)
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
#[cfg(not(target_family = "wasm"))]
mod sparql;

#[cfg(not(target_family = "wasm"))]
fn test(path: String, mode: ShaclValidationMode) -> Result<(), TestSuiteError> {
    let mut manifest = Manifest::new(Path::new(&path))?;
    let tests = manifest.collect_tests()?;

    for test in tests {
        let mut validator: DataValidation = test.data.into();
        let test_shapes = test
            .shapes
            .try_into()
            .map_err(|e: IRError| TestSuiteError::TestShapesCompilation(e.to_string()))?;

        let report = validator
            .validate(&test_shapes, &mode)
            .map_err(|e| TestSuiteError::Validation(e.to_string()))?;

        if report != test.report {
            println!("❌ Test failed");
            println!("Expected report:\n{:#?}", test.report.results());
            println!("Actual report:\n{:#?}", report.results());
            return Err(TestSuiteError::NotEquals);
        }
    }

    Ok(())
}
