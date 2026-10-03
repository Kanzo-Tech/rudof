//! Conformance to the SHACL Recommendation beyond the W3C test suite, held by
//! both engines: each case runs through the native engine and through the SQL
//! engine (on DuckDB, over a triple table) and must give the result the spec
//! prescribes.
#![cfg(not(target_family = "wasm"))]

use rudof_rdf::backend::{OxigraphInMemory, ReaderMode};
use rudof_rdf::{BuildRDF, NeighsRDF, RDFFormat};
use shacl::ir::IRSchema;
use shacl::rdf::ShaclParser;
use shacl::validator::processor::validate_with_subset;
use shacl::validator::report::ValidationReport;
use shacl::validator::sql::validate_with_duckdb;

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

/// `(focus, component local name, path, value)` of every result, sorted.
fn summary(report: &ValidationReport) -> Vec<(String, String, String, String)> {
    let mut out: Vec<_> = report
        .results()
        .iter()
        .map(|r| {
            let component = r.constraint_component().to_string();
            let local = component
                .rsplit('#')
                .next()
                .unwrap_or_default()
                .trim_end_matches('>')
                .to_owned();
            (
                r.focus_node().to_string(),
                local,
                r.path().map(ToString::to_string).unwrap_or_default(),
                r.value()
                    .map(|v| oxrdf::Term::from(v.clone()).to_string())
                    .unwrap_or_default(),
            )
        })
        .collect();
    out.sort();
    out
}

/// Validates `data` against `shapes` with both engines; asserts both agree
/// and returns their (common) results.
fn both(shapes: &str, data: &str) -> Vec<(String, String, String, String)> {
    let ast = ShaclParser::new(graph(shapes)).parse().expect("shapes parse");
    let schema = IRSchema::try_from(&ast).expect("schema compiles");
    let data = graph(data);
    let native = summary(
        &validate_with_subset(&data, &schema, OxigraphInMemory::empty())
            .expect("native")
            .0,
    );
    let sql = summary(&validate_with_duckdb(&data, &schema).expect("sql"));
    assert_eq!(sql, native, "the SQL engine and the native engine disagree");
    native
}

/// The values (as displayed) that a component reported.
fn values(results: &[(String, String, String, String)], component: &str) -> Vec<String> {
    let mut v: Vec<_> = results
        .iter()
        .filter(|r| r.1 == component)
        .map(|r| r.3.clone())
        .collect();
    v.sort();
    v
}

/// [`shown`] of every term, sorted.
fn shown_all(ntriples: &[&str]) -> Vec<String> {
    let mut v: Vec<String> = ntriples.iter().map(|s| shown(s)).collect();
    v.sort();
    v
}

/// SHACL §4.4.1-2: `sh:minLength` / `sh:maxLength` bound `STRLEN(str(?value))`,
/// which counts characters, not the bytes of their UTF-8 encoding.
#[test]
fn lengths_count_characters_not_bytes() {
    let results = both(
        r#"
ex:S a sh:NodeShape ; sh:minLength 3 ; sh:maxLength 3 ;
    sh:targetNode "日本語", "😀😀😀", "ab", "éèêë", "abc" .
"#,
        "ex:x ex:p ex:y .",
    );
    // "日本語" is 9 bytes and "😀😀😀" 12: three characters each, so both conform.
    assert_eq!(values(&results, "MinLengthConstraintComponent"), shown_all(&["\"ab\""]));
    assert_eq!(
        values(&results, "MaxLengthConstraintComponent"),
        shown_all(&["\"éèêë\""])
    );
}

/// SHACL §4.3 defines the range components by SPARQL's `<` / `<=`, which on two
/// numerics is `op:numeric-less-than` after type promotion (SPARQL 1.1 §17.3):
/// any XSD numeric type against any other, by value. An ill-formed literal is
/// not comparable, so the comparison is not `true` and the value violates.
#[test]
fn ranges_compare_numerics_by_value_across_types() {
    let results = both(
        r#"
ex:S a sh:NodeShape ; sh:minInclusive 18 ; sh:maxExclusive 20.5 ;
    sh:targetNode "20"^^xsd:long, "17"^^xsd:long, "18"^^xsd:int, "17.5"^^xsd:decimal,
        "1.8e1"^^xsd:double, "17"^^xsd:short, "19"^^xsd:unsignedByte, "abc"^^xsd:long,
        "21"^^xsd:int, "20.5"^^xsd:float, "20.4"^^xsd:decimal .
"#,
        "ex:x ex:p ex:y .",
    );
    assert_eq!(
        values(&results, "MinInclusiveConstraintComponent"),
        shown_all(&[
            "\"17\"^^<http://www.w3.org/2001/XMLSchema#long>",
            "\"17.5\"^^<http://www.w3.org/2001/XMLSchema#decimal>",
            "\"17\"^^<http://www.w3.org/2001/XMLSchema#short>",
            "\"abc\"^^<http://www.w3.org/2001/XMLSchema#long>",
        ])
    );
    assert_eq!(
        values(&results, "MaxExclusiveConstraintComponent"),
        shown_all(&[
            "\"abc\"^^<http://www.w3.org/2001/XMLSchema#long>",
            "\"21\"^^<http://www.w3.org/2001/XMLSchema#int>",
            "\"20.5\"^^<http://www.w3.org/2001/XMLSchema#float>",
        ])
    );
}

/// XSD 1.1 Part 2 §3.3.7.4 orders a timezoned and an untimezoned dateTime only
/// when that is determinate: the untimezoned one, at any timezone from -14:00
/// to +14:00, falls on one side. Exactly 14 hours apart, or closer, the pair is
/// incomparable, the SPARQL comparison is not `true`, and the value violates.
#[test]
fn date_times_follow_the_xsd_partial_order() {
    let results = both(
        r#"
ex:Naive a sh:NodeShape ; sh:minInclusive "2002-10-10T12:00:00"^^xsd:dateTime ;
    sh:targetNode "2002-10-11T02:00:01Z"^^xsd:dateTime, "2002-10-11T02:00:00Z"^^xsd:dateTime,
        "2002-10-10T12:00:00Z"^^xsd:dateTime, "2002-10-09T21:59:59Z"^^xsd:dateTime,
        "2002-10-10T12:00:01"^^xsd:dateTime, "2002-10-10T11:59:59"^^xsd:dateTime .
ex:Zoned a sh:NodeShape ; sh:maxInclusive "2002-10-10T12:00:00Z"^^xsd:dateTime ;
    sh:targetNode "2002-10-09T21:59:59"^^xsd:dateTime, "2002-10-09T22:00:00"^^xsd:dateTime,
        "2002-10-10T13:00:00+02:00"^^xsd:dateTime, "2002-10-10T13:00:00-02:00"^^xsd:dateTime .
"#,
        "ex:x ex:p ex:y .",
    );
    let dt = |s: &str| shown(&format!("\"{s}\"^^<http://www.w3.org/2001/XMLSchema#dateTime>"));
    // 02:00:01Z is more than 14h after the naive bound: determinately later.
    // 02:00:00Z (exactly 14h) and 12:00:00Z are indeterminate; 21:59:59Z the
    // day before is determinately earlier. Two naive values compare directly.
    assert_eq!(
        values(&results, "MinInclusiveConstraintComponent"),
        [
            dt("2002-10-11T02:00:00Z"),
            dt("2002-10-10T12:00:00Z"),
            dt("2002-10-09T21:59:59Z"),
            dt("2002-10-10T11:59:59"),
        ]
        .to_vec()
        .sorted()
    );
    // Against a timezoned bound: 21:59:59 the day before is determinately
    // earlier, 22:00:00 (exactly 14h) is not; two timezoned values compare as
    // instants (11:00Z conforms, 15:00Z does not).
    assert_eq!(
        values(&results, "MaxInclusiveConstraintComponent"),
        [dt("2002-10-09T22:00:00"), dt("2002-10-10T13:00:00-02:00")]
            .to_vec()
            .sorted()
    );
}

/// SHACL §4.8.1: `sh:closed` constrains the *value nodes*. On a property shape
/// those are the nodes its path reaches; the focus node's own properties are
/// none of its business.
#[test]
fn closed_on_a_property_shape_checks_the_value_nodes() {
    let results = both(
        r#"
ex:S a sh:NodeShape ; sh:targetNode ex:a ;
    sh:property [ sh:path ex:knows ; sh:closed true ; sh:ignoredProperties ( rdf:type ) ;
                  sh:property [ sh:path ex:name ] ] .
"#,
        r#"
ex:a ex:knows ex:b, ex:c ; ex:extra 1 .
ex:b a ex:Person ; ex:name "B" .
ex:c ex:name "C" ; ex:age 3 ; ex:mail "c@example.org" .
"#,
    );
    let closed: Vec<_> = results
        .iter()
        .filter(|r| r.1 == "ClosedConstraintComponent")
        .map(|r| (r.0.clone(), r.2.clone(), r.3.clone()))
        .collect();
    let a = "http://example.org/a".to_owned();
    let mut expected = vec![
        (
            a.clone(),
            "http://example.org/age".to_owned(),
            shown("\"3\"^^<http://www.w3.org/2001/XMLSchema#integer>"),
        ),
        (a, "http://example.org/mail".to_owned(), shown("\"c@example.org\"")),
    ];
    expected.sort();
    let mut closed = closed;
    closed.sort();
    // ex:a's own ex:extra is not reported: it is the focus, not a value node.
    assert_eq!(closed, expected);
}

/// A term in the N-Triples form `summary` displays values in (prefixes allowed).
fn shown(ntriples: &str) -> String {
    let g = graph(&format!("ex:s ex:p {ntriples} ."));
    let triple = g.triples().expect("triples").next().expect("one triple");
    triple.object.to_string()
}

trait Sorted {
    fn sorted(self) -> Self;
}

impl Sorted for Vec<String> {
    fn sorted(mut self) -> Self {
        self.sort();
        self
    }
}
