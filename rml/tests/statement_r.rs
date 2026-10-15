//! The round-trip property of the mapping over a graph, on DuckDB: layout independence (R1),
//! information preservation (R2) and monotonicity (R4), over the fixture of `cases/D-fossil-conformance`.
//!
//! **Statement R** is stated in the design that asks for this crate (fossil `docs/.../design/graphar.mdx`),
//! derived from Sequeda, Arenas and Miranker's definitions of information preservation and monotonicity
//! (*On directly mapping relational databases to RDF and OWL*, WWW 2012), and holds for the *admissible*
//! graphs: absolute IRI subjects unique across the types, columns whose value-to-literal map is injective,
//! edge-unique relations, no blank nodes, no data errors. The fixture is one; `recover.sql` checks the
//! conditions it can (A1, A5) before it checks what follows from them.
//!
//! - **R1**: the graph with its vertices reordered and renumbered, its integers wider, and its tables
//!   written in another order generates the same set of triples (`statement-r/permute.sql`).
//! - **R2**: from the triples alone the original graph's classes, columns and relations are recovered
//!   as sets (`statement-r/recover.sql`).
//! - **R4**: every logical source of the mapping is a selection, a projection or a join, so cutting the
//!   tables to some of their rows can only remove triples (`statement-r/subset.sql`).

#![cfg(all(feature = "sql", feature = "duckdb"))]

mod common;

use duckdb::Connection;
use rml::rdf::parse_turtle;
use sqlparser::ast::{Ident, ObjectName};
use std::fs;
use std::path::PathBuf;

fn tests() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests")
}

/// The statements of a script: split at the end of a line that ends in `;`, comment-only chunks dropped.
fn statements(file: &str) -> Vec<String> {
    fs::read_to_string(tests().join(file))
        .expect(file)
        .split(";\n")
        .map(str::trim)
        .filter(|s| {
            s.lines()
                .any(|l| !l.trim().is_empty() && !l.trim_start().starts_with("--"))
        })
        .map(str::to_owned)
        .collect()
}

fn count(connection: &Connection, sql: &str) -> i64 {
    connection
        .query_row(sql, [], |r| r.get(0))
        .unwrap_or_else(|e| panic!("{e}: {sql}"))
}

#[test]
fn statement_r_holds_on_the_fixture() {
    let case = tests().join("cases/D-fossil-conformance");
    let mapping = parse_turtle(
        &fs::read_to_string(case.join("mapping.ttl")).unwrap(),
        Some("https://example.org/"),
    )
    .unwrap();
    let connection = Connection::open_in_memory().unwrap();
    connection.execute_batch("CREATE SCHEMA g; USE g").unwrap();
    connection
        .execute_batch(&fs::read_to_string(case.join("resource.sql")).unwrap())
        .unwrap();
    connection.execute_batch("USE memory.main").unwrap();
    for script in ["statement-r/permute.sql", "statement-r/subset.sql"] {
        for statement in statements(script) {
            connection
                .execute_batch(&statement)
                .unwrap_or_else(|e| panic!("{e}: {statement}"));
        }
    }
    for schema in ["g", "perm", "sub"] {
        let query = rml::sql::triples(&mapping, Some("https://example.org/"), |t| {
            ObjectName::from(vec![Ident::with_quote('"', schema), Ident::with_quote('"', t)])
        })
        .unwrap();
        connection
            .execute_batch(&format!("CREATE VIEW {schema}_triples AS {query}"))
            .unwrap();
    }
    let differences = |a: &str, b: &str| {
        count(
            &connection,
            &format!("SELECT count(*) FROM ((SELECT * FROM {a}) EXCEPT (SELECT * FROM {b}))"),
        )
    };

    let full = count(&connection, "SELECT count(*) FROM (SELECT DISTINCT * FROM g_triples)");
    assert_eq!(full, 2193, "the triples the fixture has");

    // R1
    assert_eq!(
        differences("g_triples", "perm_triples") + differences("perm_triples", "g_triples"),
        0
    );
    let moved = count(
        &connection,
        r#"SELECT count(*) FROM g."Person" a JOIN perm."Person" b ON a.subject = b.subject WHERE a.dense_id <> b.dense_id"#,
    );
    assert!(moved > 0, "the permutation moved vertices");

    // R4
    let cut = count(&connection, "SELECT count(*) FROM (SELECT DISTINCT * FROM sub_triples)");
    assert_eq!(differences("sub_triples", "g_triples"), 0);
    assert!(0 < cut && cut < full, "a strict, non-empty cut: {cut} of {full}");

    // R2, and the conditions it needs
    for statement in statements("statement-r/recover.sql") {
        let (what, n): (String, i64) = connection
            .query_row(&statement, [], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap_or_else(|e| panic!("{e}: {statement}"));
        assert_eq!(n, 0, "{what}");
    }
}
