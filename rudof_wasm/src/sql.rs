//! The SQL engine across the ABI: the plan of the loaded shapes as SQL text
//! plus the metadata of each check, and the report read back from the rows
//! the host's engine returned. Compilation and the report are the façade's
//! (`FormEngine::compile_sql` / `report_from_rows`); here they are only
//! marshalled.

use rudof_lib::form::{FormEngine, SqlPlan, RESULT_COLUMNS};

use crate::dto::{SqlCheckDto, SqlPlanDto};
use crate::shapes::path_key;
use crate::validate::{object_to_term, path_to_term, severity_iri};

/// The plan as the ABI DTO.
pub fn plan_dto(engine: &FormEngine, plan: &SqlPlan, dialect: &str) -> SqlPlanDto {
    SqlPlanDto {
        dialect: dialect.to_lowercase(),
        columns: RESULT_COLUMNS.iter().map(|c| (*c).to_string()).collect(),
        checks: plan
            .checks
            .iter()
            .map(|check| SqlCheckDto {
                sql: check.sql(),
                source_shape: engine.sql_shape_id(&check.shape).map(object_to_term),
                source_constraint_component: check.component.as_str().to_string(),
                severity: severity_iri(&check.severity),
                path: check.path.as_ref().and_then(path_to_term),
                path_key: check.path.as_ref().map(path_key),
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use rudof_lib::form::{FormEngine, RDFFormat, SqlRow};
    use wasm_bindgen_test::wasm_bindgen_test;

    use super::plan_dto;

    const SHAPES: &str = r#"@prefix sh: <http://www.w3.org/ns/shacl#> . @prefix : <http://example.org/> .
:S a sh:NodeShape ; sh:targetClass :C ; sh:property [ sh:path :p ; sh:minCount 1 ] ."#;

    const TABLES: &str = r#"{
      "classes": [ { "class": "http://example.org/C", "table": "c", "subject": { "column": "id" } } ],
      "properties": [ { "predicate": "http://example.org/p", "table": "c",
                        "subject": { "column": "id" }, "object": { "column": "p", "termType": "Literal" } } ]
    }"#;

    fn row(focus: &str) -> SqlRow {
        let mut row: SqlRow = vec![
            Some("I".into()),
            Some(focus.into()),
            Some(String::new()),
            Some(String::new()),
        ];
        row.extend([None, None, None, None, None]);
        row
    }

    #[wasm_bindgen_test]
    fn the_plan_is_sql_text_with_the_metadata_of_each_check() {
        let mut engine = FormEngine::new();
        engine.load_shapes(SHAPES, &RDFFormat::Turtle, None).unwrap();
        let plan = engine.compile_sql(TABLES, "duckdb").unwrap().clone();
        let dto = plan_dto(&engine, &plan, "DuckDB");
        assert_eq!(dto.dialect, "duckdb");
        assert_eq!(dto.columns.len(), 9);
        assert_eq!(dto.checks.len(), 1);
        let check = &dto.checks[0];
        assert!(check.sql.contains("\"c\""), "{}", check.sql);
        assert_eq!(
            check.source_constraint_component,
            "http://www.w3.org/ns/shacl#MinCountConstraintComponent"
        );
        assert_eq!(
            check.path.as_ref().map(|p| p.value.as_str()),
            Some("http://example.org/p")
        );
    }

    #[wasm_bindgen_test]
    fn rows_come_back_as_the_native_report() {
        let mut engine = FormEngine::new();
        engine.load_shapes(SHAPES, &RDFFormat::Turtle, None).unwrap();
        engine.compile_sql(TABLES, "duckdb").unwrap();
        let outcome = engine.report_from_rows(&[vec![row("http://example.org/n")]]).unwrap();
        assert!(!outcome.conforms);
        assert_eq!(outcome.results.len(), 1);
        assert!(!outcome.results[0].message().messages().is_empty());
        // One row set per check, or the rows cannot be attributed.
        assert!(engine.report_from_rows(&[]).is_err());
    }

    #[wasm_bindgen_test]
    fn an_unknown_dialect_is_refused() {
        let mut engine = FormEngine::new();
        engine.load_shapes(SHAPES, &RDFFormat::Turtle, None).unwrap();
        assert!(engine.compile_sql(TABLES, "oracle").is_err());
    }
}
