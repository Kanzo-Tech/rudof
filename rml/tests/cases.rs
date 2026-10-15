//! The cases of `tests/cases/`: a mapping, the tables it reads, and the triples it generates.
//!
//! `RMLTC*` are the KG-Construct RML-Core test cases (<https://w3id.org/rml/core/test-cases/>,
//! the draft of 31 March 2026) of the same id, re-expressed over SQL tables: see `tests/cases/README.md`.
//! `X-*` are this crate's own, each stating the clause it holds the query to.
//!
//! Without an engine every case is held to what needs none: it parses, the query is valid DuckDB
//! SQL that parses back to itself, and it has exactly the columns of the relation. With the feature
//! `duckdb` the query is run over the case's tables and its rows are the case's triples.

#![cfg(feature = "sql")]

mod common;

use common::{expected, identity};
use rml::rdf::parse_turtle;
use sqlparser::dialect::DuckDbDialect;
use sqlparser::parser::Parser;
use std::fs;
use std::path::PathBuf;

struct Case {
    name: String,
    mapping: String,
    resource: String,
    output: String,
    base: Option<String>,
}

fn cases() -> Vec<Case> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/cases");
    let mut out = Vec::new();
    for entry in fs::read_dir(&root).expect("tests/cases") {
        let dir = entry.expect("entry").path();
        if !dir.is_dir() {
            continue;
        }
        let read = |f: &str| fs::read_to_string(dir.join(f));
        out.push(Case {
            name: dir.file_name().unwrap().to_string_lossy().into_owned(),
            mapping: read("mapping.ttl").expect("mapping.ttl"),
            resource: read("resource.sql").expect("resource.sql"),
            output: read("output.nt").expect("output.nt"),
            base: read("base").ok().map(|b| b.trim().to_owned()),
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    assert!(out.len() >= 20, "the cases are there");
    out
}

fn sql(case: &Case) -> String {
    let mapping =
        parse_turtle(&case.mapping, Some("http://example.com/base/")).unwrap_or_else(|e| panic!("{}: {e}", case.name));
    rml::sql::triples(&mapping, case.base.as_deref(), identity)
        .unwrap_or_else(|e| panic!("{}: {e}", case.name))
        .to_string()
}

#[test]
fn every_case_renders_as_sql_that_parses_back_to_itself() {
    for case in cases() {
        let text = sql(&case);
        let statements =
            Parser::parse_sql(&DuckDbDialect {}, &text).unwrap_or_else(|e| panic!("{}: {e}\n{text}", case.name));
        assert_eq!(statements.len(), 1, "{}", case.name);
        assert_eq!(statements[0].to_string(), text, "{}", case.name);
    }
}

#[test]
fn every_case_has_the_columns_of_the_relation_in_every_branch() {
    for case in cases() {
        let text = sql(&case);
        for column in rml::sql::COLUMNS {
            assert!(text.contains(&format!("AS \"{column}\"")), "{}: {column}", case.name);
        }
    }
}

#[test]
fn every_expected_output_is_readable() {
    for case in cases() {
        assert!(!case.resource.is_empty(), "{}", case.name);
        let _ = expected(&case.output);
    }
}

#[cfg(feature = "duckdb")]
#[test]
fn every_case_generates_its_triples_on_duckdb() {
    use duckdb::Connection;
    for case in cases() {
        let connection = Connection::open_in_memory().expect("duckdb");
        connection
            .execute_batch(&case.resource)
            .unwrap_or_else(|e| panic!("{}: {e}", case.name));
        connection
            .execute_batch(&format!("CREATE VIEW triples AS {}", sql(&case)))
            .unwrap_or_else(|e| panic!("{}: {e}", case.name));
        let mut statement = connection.prepare("SELECT * FROM triples").expect("select");
        let rows = statement
            .query_map([], |row| {
                (0..7).map(|i| row.get::<_, String>(i)).collect::<Result<Vec<_>, _>>()
            })
            .expect("query")
            .collect::<Result<Vec<_>, _>>()
            .unwrap_or_else(|e| panic!("{}: {e}", case.name));
        let got: std::collections::BTreeSet<String> = rows
            .iter()
            .map(|r| common::key([&r[0], &r[1], &r[2], &r[3], &r[4], &r[5], &r[6]]))
            .collect();
        let want = expected(&case.output);
        assert_eq!(got, want, "{}", case.name);
    }
}
