//! `rml::sql::triples` without an engine: the text it renders for a small mapping, how it resolves the
//! tables, and what it refuses. What the query *means* is held by `cases.rs` and `statement_r.rs` on DuckDB.

#![cfg(feature = "sql")]

mod common;

use common::identity;
use rml::Error;
use rml::rdf::parse_turtle;
use sqlparser::ast::{Ident, ObjectName};

const PREFIXES: &str = "@prefix rml: <http://w3id.org/rml/> .
@prefix ex: <https://example.org/> .
@prefix xsd: <http://www.w3.org/2001/XMLSchema#> .
";

fn render(body: &str, base: Option<&str>, tables: impl Fn(&str) -> ObjectName) -> Result<String, Error> {
    let mapping = parse_turtle(&format!("{PREFIXES}{body}"), Some("http://example.com/base/")).expect("parses");
    rml::sql::triples(&mapping, base, tables).map(|q| q.to_string())
}

#[test]
fn the_columns_are_the_relations_seven() {
    assert_eq!(
        rml::sql::COLUMNS,
        ["s_type", "s_value", "p", "o_type", "o_value", "o_datatype", "o_lang"]
    );
}

#[test]
fn a_mapping_with_no_triples_map_renders_no_rows_in_every_column() {
    let sql = render("", None, identity).unwrap();
    assert_eq!(
        sql,
        "SELECT CAST(NULL AS VARCHAR) AS \"s_type\", CAST(NULL AS VARCHAR) AS \"s_value\", CAST(NULL AS VARCHAR) AS \"p\", \
         CAST(NULL AS VARCHAR) AS \"o_type\", CAST(NULL AS VARCHAR) AS \"o_value\", CAST(NULL AS VARCHAR) AS \"o_datatype\", \
         CAST(NULL AS VARCHAR) AS \"o_lang\" WHERE false"
    );
}

/// Constants and a datatype override need no natural mapping, so the text is short enough to read.
#[test]
fn a_constant_and_a_typed_column_render_as_their_branches() {
    let sql = render(
        "<#T> rml:logicalSource [ rml:source [] ; rml:referenceFormulation rml:SQL2008Table ; rml:iterator \"t\" ] ;
           rml:subject ex:s ;
           rml:predicateObjectMap [ rml:predicate ex:kind ; rml:object ex:Kind ] ,
                                  [ rml:predicate ex:year ; rml:objectMap [ rml:constant \"2026\"^^xsd:gYear ] ] ,
                                  [ rml:predicate ex:label ; rml:objectMap [ rml:constant \"hola\"@es ] ] .",
        None,
        identity,
    )
    .unwrap();
    let branch = |p: &str, o: &str| {
        format!(
            "SELECT 'I' AS \"s_type\", 'https://example.org/s' AS \"s_value\", 'https://example.org/{p}' AS \"p\", {o} \
             FROM \"t\" AS \"t\""
        )
    };
    let year = "'L' AS \"o_type\", '2026' AS \"o_value\", 'http://www.w3.org/2001/XMLSchema#gYear' AS \"o_datatype\", '' AS \"o_lang\"";
    let kind = "'I' AS \"o_type\", 'https://example.org/Kind' AS \"o_value\", '' AS \"o_datatype\", '' AS \"o_lang\"";
    let label = "'L' AS \"o_type\", 'hola' AS \"o_value\", 'http://www.w3.org/1999/02/22-rdf-syntax-ns#langString' AS \"o_datatype\", 'es' AS \"o_lang\"";
    // The union is balanced, and the branches keep the mapping's (canonical) order.
    let mut parts: Vec<String> = sql.split(" UNION ALL ").map(str::to_owned).collect();
    parts.sort();
    let mut want = vec![branch("kind", kind), branch("label", label), branch("year", year)];
    want.sort();
    assert_eq!(parts, want);
}

#[test]
fn a_reference_is_a_column_of_the_row_and_a_null_makes_no_triple() {
    let sql = render(
        "<#T> rml:logicalSource [ rml:source [] ; rml:referenceFormulation rml:SQL2008Table ; rml:iterator \"t\" ] ;
           rml:subjectMap [ rml:reference \"id\" ] ;
           rml:predicateObjectMap [ rml:predicate ex:p ; rml:objectMap [ rml:reference \"\\\"odd name\\\"\" ; rml:datatype xsd:string ] ] .",
        None,
        identity,
    )
    .unwrap();
    assert!(sql.contains("\"t\".\"id\" IS NOT NULL"), "{sql}");
    assert!(
        sql.contains("\"t\".\"odd name\" IS NOT NULL"),
        "a delimited reference names the column inside the quotes: {sql}"
    );
    assert!(
        sql.contains("'http://www.w3.org/2001/XMLSchema#string' AS \"o_datatype\""),
        "{sql}"
    );
}

#[test]
fn the_base_iri_is_the_maps_then_the_environments_then_the_specifications() {
    let body = |base: &str| {
        format!(
            "<#T> {base} rml:logicalSource [ rml:source [] ; rml:referenceFormulation rml:SQL2008Table ; rml:iterator \"t\" ] ;
               rml:subjectMap [ rml:reference \"id\" ; rml:class ex:C ] ."
        )
    };
    // §4.1.1: the default base IRI "MUST be http://example.org/".
    let default = render(&body(""), None, identity).unwrap();
    assert!(default.contains("'http://example.org/'"), "{default}");
    let environment = render(&body(""), Some("http://example.com/"), identity).unwrap();
    assert!(
        environment.contains("'http://example.com/'") && !environment.contains("example.org/'"),
        "{environment}"
    );
    let own = render(
        &body("rml:baseIRI <http://own.example/> ;"),
        Some("http://example.com/"),
        identity,
    )
    .unwrap();
    assert!(
        own.contains("'http://own.example/'") && !own.contains("'http://example.com/'"),
        "{own}"
    );
    // A template that starts with a scheme is absolute whatever its data: no prefix.
    let template =
        "<#T> rml:logicalSource [ rml:source [] ; rml:referenceFormulation rml:SQL2008Table ; rml:iterator \"t\" ] ;
           rml:subjectMap [ rml:template \"https://x.org/{id}\" ; rml:class ex:C ] .";
    assert!(
        !render(template, None, identity)
            .unwrap()
            .contains("regexp_matches(\"t\"")
    );
}

#[test]
fn the_host_resolves_the_tables_a_table_and_a_query_name() {
    let schema = |name: &str| ObjectName::from(vec![Ident::with_quote('"', "job"), Ident::with_quote('"', name)]);
    let sql = render(
        "<#A> rml:logicalSource [ rml:source [] ; rml:referenceFormulation rml:SQL2008Table ; rml:iterator \"Person\" ] ;
              rml:subject ex:s ; rml:predicateObjectMap [ rml:predicate ex:p ; rml:object ex:o ] .
         <#B> rml:logicalSource [ rml:source [] ; rml:referenceFormulation rml:SQL2008Query ;
              rml:iterator \"WITH recent AS (SELECT * FROM Orders) SELECT p.id FROM Person AS p JOIN recent r ON r.p = p.id JOIN other.Shared s ON true, range(3) AS n\" ] ;
              rml:subject ex:s ; rml:predicateObjectMap [ rml:predicate ex:p ; rml:object ex:o ] .",
        None,
        schema,
    )
    .unwrap();
    assert!(sql.contains("FROM \"job\".\"Person\" AS \"t\""), "the table: {sql}");
    assert!(
        sql.contains("FROM \"job\".\"Orders\""),
        "a table the query's CTE reads: {sql}"
    );
    assert!(
        sql.contains("FROM \"job\".\"Person\" AS p"),
        "a table the query reads: {sql}"
    );
    assert!(
        sql.contains("JOIN recent r ON"),
        "a CTE the query declares is not a table: {sql}"
    );
    assert!(
        sql.contains("JOIN other.Shared"),
        "a name with a schema is the host's already: {sql}"
    );
    assert!(sql.contains("range(3)"), "a table function is not a table: {sql}");
}

#[test]
fn a_query_that_is_not_sql_is_refused_by_name() {
    for query in ["SELEC 1", "SELECT 1; SELECT 2", "CREATE TABLE t (a INT)"] {
        let r = render(
            &format!(
                "<#T> rml:logicalSource [ rml:source [] ; rml:referenceFormulation rml:SQL2008Query ; rml:iterator \"{query}\" ] ;
                   rml:subject ex:s ; rml:predicateObjectMap [ rml:predicate ex:p ; rml:object ex:o ] ."
            ),
            None,
            identity,
        );
        assert!(matches!(r, Err(Error::Query { .. })), "{query}: {r:?}");
    }
}

#[test]
fn a_view_is_created_over_the_tables_the_host_resolves_and_its_names_are_read_as_sql() {
    let mapping = parse_turtle(
        &format!(
            "{PREFIXES}<#T> rml:logicalSource [ rml:source [] ; rml:referenceFormulation rml:SQL2008Table ; rml:iterator \"Person\" ] ;
               rml:subject ex:s ; rml:predicateObjectMap [ rml:predicate ex:p ; rml:object ex:o ] ."
        ),
        None,
    )
    .unwrap();
    let job = |t: &str| Ok(format!("\"job.1\".\"{t}\""));
    let sql = rml::sql::create_view(&mapping, None, "\"job.1\".triples", job).unwrap();
    assert!(
        sql.starts_with("CREATE OR REPLACE VIEW \"job.1\".triples AS SELECT 'I' AS \"s_type\""),
        "{sql}"
    );
    assert!(sql.contains("FROM \"job.1\".\"Person\" AS \"t\""), "{sql}");

    // What the host gives that is not a SQL name is refused, whether it is the view's or a table's.
    for into in ["triples; DROP TABLE t", "a b", ""] {
        assert!(
            matches!(
                rml::sql::create_view(&mapping, None, into, job),
                Err(Error::Table { .. })
            ),
            "{into:?}"
        );
    }
    let r = rml::sql::create_view(&mapping, None, "triples", |_| Ok("Person) --".to_owned()));
    assert!(matches!(r, Err(Error::Table { .. })), "{r:?}");
    let r = rml::sql::create_view(&mapping, None, "triples", |t| Err(format!("no such table {t}")));
    assert!(
        matches!(&r, Err(Error::Table { name, reason }) if name == "Person" && reason.contains("no such table")),
        "{r:?}"
    );
}
