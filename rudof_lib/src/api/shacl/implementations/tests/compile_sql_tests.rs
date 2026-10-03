use crate::{
    Rudof, RudofConfig,
    api::shacl::implementations::compile_sql::compile_sql,
    api::shacl::implementations::load_shacl_schema::load_shacl_schema,
    formats::{InputSpec, ShaclFormat, SqlDialectFormat, SqlMapping},
};

const SHAPES: &str = r#"
    @prefix ex: <http://example.org/> .
    @prefix sh: <http://www.w3.org/ns/shacl#> .
    @prefix xsd: <http://www.w3.org/2001/XMLSchema#> .

    ex:PersonShape a sh:NodeShape ;
        sh:targetClass ex:Person ;
        sh:property [ sh:path ex:name ; sh:datatype xsd:string ; sh:minCount 1 ] .
"#;

fn rudof_with(shapes: &str) -> Rudof {
    let mut rudof = Rudof::new(RudofConfig::default());
    load_shacl_schema(
        &mut rudof,
        Some(&InputSpec::str(shapes)),
        Some(&ShaclFormat::Turtle),
        None,
        None,
    )
    .unwrap();
    rudof
}

#[test]
fn test_compile_sql_over_a_triple_table() {
    let rudof = rudof_with(SHAPES);
    let mapping = SqlMapping::TripleTable {
        table: "triples".to_string(),
    };
    let plan = compile_sql(&rudof, &mapping, Some(&SqlDialectFormat::DuckDb)).unwrap();
    // sh:datatype and sh:minCount of the one property shape.
    assert_eq!(plan.checks.len(), 2);
    assert!(plan.checks.iter().all(|c| c.sql().contains("\"triples\"")));
}

#[test]
fn test_compile_sql_over_tables() {
    let rudof = rudof_with(SHAPES);
    let mapping = SqlMapping::from_json(
        r#"{
          "classes": [ { "class": "http://example.org/Person", "table": "person", "subject": { "column": "id" } } ],
          "properties": [ { "predicate": "http://example.org/name", "table": "person",
                            "subject": { "column": "id" },
                            "object": { "column": "name", "termType": "Literal" } } ]
        }"#,
    )
    .unwrap();
    let plan = rudof.compile_sql(&mapping).execute().unwrap();
    assert_eq!(plan.checks.len(), 2);
    assert!(plan.checks.iter().all(|c| c.sql().contains("\"person\"")));
}

#[test]
fn test_compile_sql_refuses_recursive_shapes() {
    let rudof = rudof_with(
        r#"
        @prefix ex: <http://example.org/> .
        @prefix sh: <http://www.w3.org/ns/shacl#> .
        ex:S a sh:NodeShape ; sh:targetClass ex:Person ;
            sh:property [ sh:path ex:knows ; sh:node ex:S ] .
        "#,
    );
    let mapping = SqlMapping::TripleTable {
        table: "triples".to_string(),
    };
    assert!(compile_sql(&rudof, &mapping, None).is_err());
}

#[test]
fn test_compile_sql_needs_shapes() {
    let rudof = Rudof::new(RudofConfig::default());
    let mapping = SqlMapping::TripleTable {
        table: "triples".to_string(),
    };
    assert!(compile_sql(&rudof, &mapping, None).is_err());
}

#[test]
fn test_sql_dialect_and_mode_parse() {
    assert_eq!("duckdb".parse::<SqlDialectFormat>().unwrap(), SqlDialectFormat::DuckDb);
    assert!("postgres".parse::<SqlDialectFormat>().is_err());
    assert_eq!(
        "sql".parse::<crate::formats::ShaclValidationMode>().unwrap(),
        crate::formats::ShaclValidationMode::Sql
    );
}

#[test]
fn test_validate_shacl_in_sql_mode() {
    use crate::api::data::implementations::load_data;
    use crate::api::shacl::implementations::validate_shacl::validate_shacl;
    use crate::formats::{DataFormat, ShaclValidationMode};

    let mut rudof = rudof_with(SHAPES);
    let data = InputSpec::str(
        r#"
        @prefix ex: <http://example.org/> .
        ex:alice a ex:Person ; ex:name "Alice" .
        ex:bob a ex:Person .
        "#,
    );
    load_data(
        &mut rudof,
        Some(&[data]),
        Some(&DataFormat::Turtle),
        None,
        None,
        None,
        None,
        None,
    )
    .unwrap();
    let outcome = validate_shacl(&mut rudof, Some(&ShaclValidationMode::Sql));
    if cfg!(feature = "duckdb") {
        outcome.unwrap();
        let report = rudof.shacl_validation_results.as_ref().unwrap();
        // ex:bob has no ex:name.
        assert_eq!(report.results().len(), 1);
    } else {
        // Without an engine linked, the mode says so instead of validating.
        assert!(outcome.is_err());
    }
}
