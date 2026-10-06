//! The SQL engine across the ABI: the plan of the loaded shapes as a SQL
//! script plus the metadata of each check, and the report read back from the rows
//! the host's engine returned. Compilation and the report are the façade's
//! (`FormEngine::compile_sql` / `report_from_rows`); here they are only
//! marshalled.

use rudof_lib::form::{SqlDialect, SqlPlan, RESULT_COLUMNS};

use crate::dto::{SqlCheckDto, SqlPlanDto};
use crate::shapes::path_key;
use crate::validate::{object_to_term, path_to_term, severity_iri};

/// The plan as the ABI DTO.
pub fn plan_dto(plan: &SqlPlan, dialect: SqlDialect) -> SqlPlanDto {
    SqlPlanDto {
        dialect: dialect.to_string(),
        setup: plan.setup().to_vec(),
        query: plan.query().to_owned(),
        teardown: plan.teardown().to_vec(),
        columns: RESULT_COLUMNS.iter().map(|c| (*c).to_string()).collect(),
        checks: plan
            .checks
            .iter()
            .map(|check| SqlCheckDto {
                source_shape: plan
                    .schema()
                    .get_shape_from_idx(&check.shape)
                    .map(|shape| object_to_term(shape.id())),
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
    use rudof_lib::form::{FormEngine, RDFFormat, SqlDialect, SqlMapping, SqlRow};
    use wasm_bindgen_test::wasm_bindgen_test;

    use super::plan_dto;

    const SHAPES: &str = r#"@prefix sh: <http://www.w3.org/ns/shacl#> . @prefix : <http://example.org/> .
:S a sh:NodeShape ; sh:targetClass :C ; sh:property [ sh:path :p ; sh:minCount 1 ] ."#;

    const TABLES: &str = r#"
        @prefix rr: <http://www.w3.org/ns/r2rml#> . @prefix : <http://example.org/> .
        <#C> rr:logicalTable [ rr:tableName "c" ] ;
            rr:subjectMap [ rr:column "id" ; rr:class :C ] ;
            rr:predicateObjectMap [ rr:predicate :p ; rr:objectMap [ rr:column "p" ] ] ."#;

    fn tables(schema: Option<&str>) -> SqlMapping {
        SqlMapping::R2rml {
            mapping: TABLES.to_owned(),
            schema: schema.map(str::to_owned),
            base_iri: None,
        }
    }

    fn row(check: &str, focus: &str) -> SqlRow {
        let mut row: SqlRow = vec![
            Some(check.into()),
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
        let plan = engine
            .compile_sql(&tables(Some("warehouse")), SqlDialect::DuckDb)
            .unwrap();
        let dto = plan_dto(plan, SqlDialect::DuckDb);
        assert_eq!(dto.dialect, "duckdb");
        assert_eq!(dto.columns.len(), 10);
        assert!(dto.query.contains("\"warehouse\".\"c\""), "{}", dto.query);
        assert_eq!(dto.setup.len(), dto.teardown.len());
        assert_eq!(dto.checks.len(), 1);
        let check = &dto.checks[0];
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
    fn rows_come_back_as_the_in_memory_report() {
        let mut engine = FormEngine::new();
        engine.load_shapes(SHAPES, &RDFFormat::Turtle, None).unwrap();
        engine
            .compile_sql(&tables(Some("warehouse")), SqlDialect::DuckDb)
            .unwrap();
        let outcome = engine.report_from_rows(&[row("0", "http://example.org/n")]).unwrap();
        assert!(!outcome.conforms);
        assert_eq!(outcome.results.len(), 1);
        assert!(!outcome.results[0].message().messages().is_empty());
        assert!(engine.report_from_rows(&[]).unwrap().conforms);
        // A row naming no check of the plan cannot be attributed.
        assert!(engine.report_from_rows(&[row("1", "http://example.org/n")]).is_err());
    }

    #[wasm_bindgen_test]
    fn an_unknown_dialect_is_refused() {
        assert!("oracle".parse::<SqlDialect>().is_err());
        assert_eq!("DuckDB".parse::<SqlDialect>().unwrap(), SqlDialect::DuckDb);
    }
}
