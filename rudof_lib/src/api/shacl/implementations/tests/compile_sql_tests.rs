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
    assert!(plan.query().contains("\"triples\"") || plan.setup().iter().any(|s| s.contains("\"triples\"")));
}

#[test]
fn test_compile_sql_over_an_rml_mapping() {
    let rudof = rudof_with(SHAPES);
    let mapping = SqlMapping::Rml {
        mapping: r#"
            @prefix rml: <http://w3id.org/rml/> . @prefix ex: <http://example.org/> .
            <#Person> a rml:TriplesMap ;
                rml:logicalSource [ rml:source [ a rml:Source ] ;
                                    rml:referenceFormulation rml:SQL2008Table ; rml:iterator "person" ] ;
                rml:subjectMap [ rml:reference "id" ; rml:class ex:Person ] ;
                rml:predicateObjectMap [ rml:predicate ex:name ; rml:objectMap [ rml:reference "name" ] ] .
        "#
        .to_string(),
        schema: Some("warehouse".to_string()),
    };
    let plan = rudof.compile_sql(&mapping).execute().unwrap();
    assert_eq!(plan.checks.len(), 2);
    // The unqualified table name resolves against the schema.
    let script = [plan.setup().join(";\n"), plan.query().to_owned()].join(";\n");
    assert!(script.contains("\"warehouse\".\"person\""), "{script}");
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
fn test_sql_dialect_parse() {
    assert_eq!("duckdb".parse::<SqlDialectFormat>().unwrap(), SqlDialectFormat::DuckDb);
    assert!("postgres".parse::<SqlDialectFormat>().is_err());
}
