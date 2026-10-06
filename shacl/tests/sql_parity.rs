//! The SQL engine reads the same report from one dataset whether it sits in a
//! triple table or in ordinary tables described by an R2RML mapping — and
//! that report is the in-memory evaluator's.
#![cfg(not(target_family = "wasm"))]

use rudof_rdf::RDFFormat;
use rudof_rdf::backend::{OxigraphInMemory, ReaderMode};
use shacl::ir::IRSchema;
use shacl::rdf::ShaclParser;
use shacl::validator::report::ValidationReport;
use shacl::validator::sql::{DuckDbExecutor, SqlCompileError, SqlDialect, SqlMapping, compile};

mod common;
use common::validate_with_duckdb;

const PREFIXES: &str = r#"
@prefix sh:  <http://www.w3.org/ns/shacl#> .
@prefix ex:  <http://example.org/> .
@prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .
@prefix xsd: <http://www.w3.org/2001/XMLSchema#> .
"#;

fn graph(ttl: &str) -> OxigraphInMemory {
    OxigraphInMemory::from_str(&format!("{PREFIXES}{ttl}"), &RDFFormat::Turtle, None, &ReaderMode::Lax)
        .expect("turtle parses")
}

fn schema(shapes: &str) -> IRSchema {
    let ast = ShaclParser::new(graph(shapes)).parse().expect("shapes parse");
    IRSchema::try_from(&ast).expect("schema compiles")
}

/// The data, as RDF.
const DATA: &str = r#"
ex:alice a ex:Person ; ex:name "Alice" ; ex:age 30 ; ex:knows ex:bob, ex:carol, ex:dave, ex:erin .
ex:bob a ex:Person ; ex:name "Bob" ; ex:age 17 ; ex:knows ex:alice, ex:erin .
ex:carol a ex:Student ; ex:name "carol" ; ex:age 21 ; ex:knows ex:dave .
ex:dave a ex:Person ; ex:age 40 ; ex:email "dave@example.org" .
ex:erin a ex:Person .
ex:Student rdfs:subClassOf ex:Person .
"#;

/// The same data, as the tables a host would hold: vertices with surrogate
/// keys, and an edge table `knows(src, dst)` that refers to them by key.
const TABLES: &str = r#"
CREATE TABLE node (id INTEGER, iri VARCHAR);
INSERT INTO node VALUES
    (1, 'http://example.org/alice'), (2, 'http://example.org/bob'), (3, 'http://example.org/carol'),
    (4, 'http://example.org/dave'), (5, 'http://example.org/erin');
CREATE TABLE person (id INTEGER, iri VARCHAR, name VARCHAR, age INTEGER, email VARCHAR);
INSERT INTO person VALUES
    (1, 'http://example.org/alice', 'Alice', 30, NULL),
    (2, 'http://example.org/bob', 'Bob', 17, NULL),
    (4, 'http://example.org/dave', NULL, 40, 'dave@example.org'),
    (5, 'http://example.org/erin', NULL, NULL, NULL);
CREATE TABLE student (id INTEGER, iri VARCHAR, name VARCHAR, age INTEGER);
INSERT INTO student VALUES (3, 'http://example.org/carol', 'carol', 21);
CREATE TABLE knows (src INTEGER, dst INTEGER);
INSERT INTO knows VALUES (1, 2), (1, 3), (1, 4), (1, 5), (2, 1), (2, 5), (3, 4);
"#;

/// Where those tables' terms are, in R2RML: classes and literal columns, an
/// edge table whose source key is resolved by an R2RML view joining the
/// vertex table, and whose target is a referencing object map joined on the
/// target key. The subclass triple comes from a triples map whose terms are
/// constants, over a one-row view.
const MAPPING: &str = r#"
@prefix rr:   <http://www.w3.org/ns/r2rml#> .
@prefix ex:   <http://example.org/> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .

<#Node> a rr:TriplesMap ; rr:logicalTable [ rr:tableName "node" ] ;
    rr:subjectMap [ rr:column "iri" ] .

<#Person> a rr:TriplesMap ; rr:logicalTable [ rr:tableName "person" ] ;
    rr:subjectMap [ rr:column "iri" ; rr:class ex:Person ] ;
    rr:predicateObjectMap [ rr:predicate ex:name ; rr:objectMap [ rr:column "name" ] ] ;
    rr:predicateObjectMap [ rr:predicate ex:age ; rr:objectMap [ rr:column "age" ] ] ;
    rr:predicateObjectMap [ rr:predicate ex:email ; rr:objectMap [ rr:column "email" ] ] .

<#Student> a rr:TriplesMap ; rr:logicalTable [ rr:tableName "student" ] ;
    rr:subjectMap [ rr:column "iri" ; rr:class ex:Student ] ;
    rr:predicateObjectMap [ rr:predicate ex:name ; rr:objectMap [ rr:column "name" ] ] ;
    rr:predicateObjectMap [ rr:predicate ex:age ; rr:objectMap [ rr:column "age" ] ] .

<#Knows> a rr:TriplesMap ;
    rr:logicalTable [ rr:sqlQuery """SELECT k.dst, n.iri AS src_iri FROM knows AS k JOIN node AS n ON k.src = n.id""" ] ;
    rr:subjectMap [ rr:column "src_iri" ] ;
    rr:predicateObjectMap [
        rr:predicate ex:knows ;
        rr:objectMap [ rr:parentTriplesMap <#Node> ; rr:joinCondition [ rr:child "dst" ; rr:parent "id" ] ]
    ] .

<#Hierarchy> a rr:TriplesMap ; rr:logicalTable [ rr:sqlQuery "SELECT 1 AS one" ] ;
    rr:subject ex:Student ;
    rr:predicateObjectMap [ rr:predicate rdfs:subClassOf ; rr:object ex:Person ] .
"#;

/// Shapes over classes, literal columns and edges: targets with subclasses,
/// cardinality, ranges, patterns, a path over the edge table, a closure, a
/// property pair, and the shape-based components.
const SHAPES: &str = r#"
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
ex:PersonShape a sh:NodeShape ;
    sh:targetClass ex:Person ;
    sh:property [ sh:path ex:name ; sh:minCount 1 ; sh:datatype xsd:string ; sh:pattern "^[A-Z]" ] ;
    sh:property [ sh:path ex:age ; sh:maxCount 1 ; sh:minInclusive 18 ] ;
    sh:property [ sh:path ex:knows ; sh:class ex:Person ; sh:node ex:Named ] ;
    sh:property [ sh:path ( ex:knows ex:knows ) ; sh:disjoint ex:knows ] ;
    sh:property [ sh:path [ sh:oneOrMorePath ex:knows ] ; sh:minCount 2 ] ;
    sh:property [ sh:path [ sh:inversePath ex:knows ] ; sh:maxCount 1 ;
                  sh:qualifiedValueShape ex:Named ; sh:qualifiedMinCount 1 ] .
ex:Named a sh:NodeShape ;
    sh:property [ sh:path ex:name ; sh:minCount 1 ] .
ex:AdultShape a sh:NodeShape ;
    sh:targetSubjectsOf ex:age ;
    sh:or ( [ sh:path ex:age ; sh:minExclusive 20 ] [ sh:path ex:knows ; sh:minCount 2 ] ) ;
    sh:closed true ;
    sh:ignoredProperties ( rdf:type ex:name ex:age ) ;
    sh:property [ sh:path ex:knows ] .
"#;

fn in_memory(data: &OxigraphInMemory, schema: &IRSchema) -> ValidationReport {
    shacl::validator::validate(schema, data).expect("in memory validates")
}

/// Validates through the R2RML mapping, with the tables in `db_schema` (the
/// mapping's table names unqualified) when one is given.
fn through_tables(schema: &IRSchema, db_schema: Option<&str>) -> ValidationReport {
    let executor = DuckDbExecutor::in_memory().expect("duckdb opens");
    let ddl = match db_schema {
        Some(name) => format!(
            "CREATE SCHEMA {name};\n{}",
            TABLES
                .replace("TABLE ", &format!("TABLE {name}."))
                .replace("INTO ", &format!("INTO {name}."))
        ),
        None => TABLES.to_owned(),
    };
    executor.connection().execute_batch(&ddl).expect("tables load");
    let mapping = r2rml(db_schema);
    let plan = compile(schema, &mapping, SqlDialect::DuckDb).expect("shapes compile");
    plan.validate(&executor).expect("plan runs")
}

fn r2rml(db_schema: Option<&str>) -> SqlMapping {
    SqlMapping::R2rml {
        mapping: MAPPING.to_owned(),
        schema: db_schema.map(str::to_owned),
        base_iri: None,
    }
}

#[test]
fn tables_and_triple_table_yield_the_evaluators_report() {
    let data = graph(&format!(
        "@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .\n{DATA}"
    ));
    let schema = schema(SHAPES);

    let in_memory = in_memory(&data, &schema);
    let triples = validate_with_duckdb(&data, &schema).expect("triple table validates");
    let tables = through_tables(&schema, None);
    let in_a_schema = through_tables(&schema, Some("warehouse"));

    assert!(
        in_memory.results().len() > 15,
        "the dataset violates many components: {in_memory}"
    );
    assert_eq!(
        triples, in_memory,
        "triple table vs in memory\n{triples}\n---\n{in_memory}"
    );
    assert_eq!(tables, in_memory, "tables vs in memory\n{tables}\n---\n{in_memory}");
    assert_eq!(in_a_schema, in_memory, "tables in a schema vs in memory");
}

#[test]
fn messages_are_the_evaluators() {
    let data = graph(&format!(
        "@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .\n{DATA}"
    ));
    let schema = schema(SHAPES);
    let keyed = |report: &ValidationReport| {
        let mut out: Vec<_> = report
            .results()
            .iter()
            .map(|r| {
                (
                    format!("{} {} {:?}", r.focus_node(), r.constraint_component(), r.value()),
                    r.message().clone(),
                )
            })
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    };
    let in_memory = keyed(&in_memory(&data, &schema));
    let sql = keyed(&validate_with_duckdb(&data, &schema).expect("validates"));
    assert_eq!(sql.len(), in_memory.len());
    for ((key, sql), (_, in_memory)) in sql.iter().zip(&in_memory) {
        assert_eq!(sql, in_memory, "the message of {key}");
    }
}

#[test]
fn recursive_shapes_are_refused() {
    let schema = schema(
        r#"
ex:S a sh:NodeShape ; sh:targetClass ex:Person ;
    sh:property [ sh:path ex:knows ; sh:node ex:S ] .
"#,
    );
    let refused = compile(&schema, &r2rml(None), SqlDialect::DuckDb).expect_err("recursion is refused");
    assert!(matches!(refused, SqlCompileError::RecursiveShapes(_)), "{refused}");
}

#[test]
fn shacl_sparql_is_refused_not_skipped() {
    let schema = schema(
        r#"
ex:S a sh:NodeShape ; sh:targetClass ex:Person ;
    sh:sparql [ sh:select "SELECT $this WHERE { $this ?p ?o }" ] .
"#,
    );
    let refused = compile(&schema, &r2rml(None), SqlDialect::DuckDb).expect_err("sh:sparql is refused");
    assert!(matches!(refused, SqlCompileError::Unsupported(_)), "{refused}");
}

#[test]
fn target_where_selects_the_same_focus_nodes_through_sql() {
    let data = graph(
        r#"
ex:a a ex:Person .
ex:b a ex:Person ; ex:name "B" .
ex:c ex:name "C" .
"#,
    );
    let schema = schema(
        r#"
ex:S a sh:NodeShape ; sh:targetWhere ex:W ; sh:property [ sh:path ex:name ; sh:minCount 1 ] .
ex:W a sh:NodeShape ; sh:class ex:Person .
"#,
    );
    let sql = validate_with_duckdb(&data, &schema).expect("sql validates");
    assert_eq!(sql, in_memory(&data, &schema));
    assert_eq!(sql.results().len(), 1);
}
