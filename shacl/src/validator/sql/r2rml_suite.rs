//! The W3C R2RML test cases (RDB2RDF Working Group), run through the reader
//! as an R2RML processor: each case's database is loaded into DuckDB, its
//! mapping read into [`Tables`], and the triples the mapping yields, read
//! back term by term, must be the case's expected graph (as RDF graphs, blank
//! nodes and all). A case that expects no output must fail: reading the
//! mapping, running its SQL, or reading a term back (a data error, R2RML
//! §11).
//!
//! A case whose mapping uses what the reader refuses by name (graph maps,
//! computed predicates) is *inapplicable*, and one whose database DuckDB
//! cannot create is *cannot tell*: neither is skipped silently. A case that
//! fails is listed in [`KNOWN`] with its reason; a listed case that passes,
//! or a failure not listed, is red. The test also pins the counts, so a
//! change in any of them is read before it is accepted. With `R2RML_EARL` set to a path,
//! the outcomes are written there as an EARL 1.0 report, the format of the
//! R2RML implementation report.
//!
//! The cases are `tests/r2rml`, from KG-Construct's
//! `r2rml-test-cases-support` (its README says which commit).

use crate::validator::sql::SqlCompileError;
use crate::validator::sql::dialect::{Dialect, DuckDb};
use crate::validator::sql::mapping::RelationalMapping;
use crate::validator::sql::tables::Tables;
use crate::validator::sql::term::decode;
use duckdb::Connection;
use oxrdf::graph::CanonicalizationAlgorithm;
use oxrdf::{Graph, NamedNode, NamedOrBlankNode, Term, Triple};
use rudof_rdf::backend::{OxigraphInMemory, ReaderMode};
use rudof_rdf::term::Triple as _;
use rudof_rdf::{NeighsRDF, RDFFormat};
use sqlparser::ast::Statement;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The base IRI of the suite's processor runs: what relative IRIs resolve to.
const BASE_IRI: &str = "http://example.com/base/";
const MANIFEST_BASE: &str = "http://www.w3.org/2001/sw/rdb2rdf/test-cases/";
const TEST: &str = "http://purl.org/NET/rdb2rdf-test#";
const DCTERMS_IDENTIFIER: &str = "http://purl.org/dc/terms/identifier";

/// The cases that fail, and why.
const KNOWN: &[(&str, &str)] = &[
    (
        "R2RMLTC0002f",
        "DuckDB matches identifiers without regard to case, delimited or not, so the regular identifier \
         Name finds the delimited column \"Name\" that SQL 2008 would not",
    ),
    (
        "R2RMLTC0015b",
        "\"english\" is a well-formed BCP 47 tag (a 5-8 letter primary subtag); only the IANA registry, \
         which the reader does not hold, says no such language exists",
    ),
    (
        "R2RMLTC0018a",
        "DuckDB's CHAR(n) is VARCHAR: values are not padded with blanks to n characters",
    ),
];

enum Outcome {
    Passed,
    Failed(String),
    Inapplicable(String),
    /// The case's database does not load into DuckDB, so nothing was tried.
    CantTell(String),
}

struct Case {
    id: String,
    script: PathBuf,
    mapping: PathBuf,
    output: Option<PathBuf>,
}

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/r2rml")
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn cases() -> Vec<Case> {
    let manifest = OxigraphInMemory::from_str(
        &read(&dir().join("manifest.ttl")),
        &RDFFormat::Turtle,
        Some(MANIFEST_BASE),
        &ReaderMode::Strict,
    )
    .expect("the manifest parses");
    let mut arcs: BTreeMap<String, BTreeMap<String, Term>> = BTreeMap::new();
    for triple in manifest.triples().expect("the manifest reads") {
        let (s, p, o) = triple.into_components();
        arcs.entry(s.to_string()).or_default().insert(p.as_str().to_owned(), o);
    }
    let text = |node: &BTreeMap<String, Term>, p: &str| match node.get(p) {
        Some(Term::Literal(l)) => Some(l.value().to_owned()),
        _ => None,
    };
    let mut cases: Vec<Case> = arcs
        .values()
        .filter(|node| node.contains_key(&format!("{TEST}mappingDocument")))
        .map(|node| {
            let id = text(node, DCTERMS_IDENTIFIER).expect("a case has an identifier");
            let database = node
                .get(&format!("{TEST}database"))
                .and_then(|db| arcs.get(&db.to_string()))
                .expect("a case names its database");
            let file = |name: String| dir().join(&id).join(name);
            Case {
                script: dir()
                    .join("databases")
                    .join(text(database, &format!("{TEST}sqlScriptFile")).expect("a script")),
                mapping: file(text(node, &format!("{TEST}mappingDocument")).expect("a mapping")),
                output: text(node, &format!("{TEST}output")).map(file),
                id,
            }
        })
        .collect();
    cases.sort_by(|a, b| a.id.cmp(&b.id));
    cases
}

/// The triples `tables` yields on `connection`, each term read back.
fn triples(connection: &Connection, tables: &Tables<DuckDb>) -> Result<Graph, String> {
    let sql = DuckDb.render(&Statement::Query(Box::new(tables.triples())));
    let mut statement = connection.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = statement
        .query_map([], |row| {
            (0..9).map(|i| row.get::<_, String>(i)).collect::<Result<Vec<_>, _>>()
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let mut graph = Graph::new();
    for row in rows {
        let term = |at: usize| decode(&row[at], &row[at + 1], &row[at + 2], &row[at + 3]).map(Term::from);
        let subject = match term(0)? {
            Term::NamedNode(n) => NamedOrBlankNode::from(n),
            Term::BlankNode(b) => NamedOrBlankNode::from(b),
            other => return Err(format!("the subject {other} is not an IRI or a blank node")),
        };
        let predicate = NamedNode::new(&row[4]).map_err(|e| format!("the predicate <{}>: {e}", row[4]))?;
        graph.insert(&Triple::new(subject, predicate, term(5)?));
    }
    Ok(graph)
}

fn expected(path: &Path) -> Graph {
    let store = OxigraphInMemory::from_str(&read(path), &RDFFormat::NQuads, None, &ReaderMode::Strict)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut graph = Graph::new();
    for triple in store.triples().expect("the expected graph reads") {
        let (s, p, o) = triple.into_components();
        graph.insert(&Triple::new(s, p, o));
    }
    graph
}

fn run(case: &Case) -> Outcome {
    let connection = Connection::open_in_memory().expect("duckdb opens");
    if let Err(e) = connection.execute_batch(&read(&case.script)) {
        return Outcome::CantTell(format!("the database does not load into DuckDB: {e}"));
    }
    let produced = match Tables::from_r2rml(&read(&case.mapping), None, Some(BASE_IRI), DuckDb) {
        Err(SqlCompileError::UnsupportedR2rml(why)) => return Outcome::Inapplicable(why),
        Err(e) => Err(e.to_string()),
        Ok(tables) => triples(&connection, &tables),
    };
    match (produced, &case.output) {
        (Err(_), None) => Outcome::Passed,
        (Ok(_), None) => Outcome::Failed("an error was expected, and the mapping yields triples".to_owned()),
        (Err(e), Some(_)) => Outcome::Failed(e),
        (Ok(mut produced), Some(output)) => {
            let mut expected = expected(output);
            produced.canonicalize(CanonicalizationAlgorithm::Unstable);
            expected.canonicalize(CanonicalizationAlgorithm::Unstable);
            if produced == expected {
                Outcome::Passed
            } else {
                Outcome::Failed(format!("produced\n{produced}\nexpected\n{expected}"))
            }
        },
    }
}

/// The outcomes as an EARL 1.0 report.
fn earl(outcomes: &[(String, Outcome)]) -> String {
    let mut out = String::from(
        "@prefix earl: <http://www.w3.org/ns/earl#> .\n\
         @prefix dc: <http://purl.org/dc/terms/> .\n\
         <https://github.com/Kanzo-Tech/rudof> a earl:TestSubject, earl:Software ;\n    \
         dc:title \"rudof SHACL SQL engine, R2RML reader\" .\n\n",
    );
    for (id, outcome) in outcomes {
        let (result, info) = match outcome {
            Outcome::Passed => ("earl:passed", String::new()),
            Outcome::Failed(why) => ("earl:failed", why.clone()),
            Outcome::Inapplicable(why) => ("earl:inapplicable", why.clone()),
            Outcome::CantTell(why) => ("earl:cantTell", why.clone()),
        };
        let info = info.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n");
        out.push_str(&format!(
            "[] a earl:Assertion ;\n    earl:subject <https://github.com/Kanzo-Tech/rudof> ;\n    \
             earl:test <{MANIFEST_BASE}{id}> ;\n    earl:mode earl:automatic ;\n    \
             earl:result [ a earl:TestResult ; earl:outcome {result} ; earl:info \"{info}\" ] .\n\n"
        ));
    }
    out
}

#[test]
fn the_r2rml_test_cases() {
    let outcomes: Vec<(String, Outcome)> = cases().iter().map(|case| (case.id.clone(), run(case))).collect();
    assert_eq!(outcomes.len(), 62, "the suite has 62 cases");
    if let Ok(path) = std::env::var("R2RML_EARL") {
        std::fs::write(&path, earl(&outcomes)).unwrap_or_else(|e| panic!("{path}: {e}"));
    }
    let mut wrong = Vec::new();
    for (id, outcome) in &outcomes {
        let known = KNOWN.iter().find(|(k, _)| k == id);
        match (outcome, known) {
            (Outcome::Failed(why), None) => wrong.push(format!("{id} fails: {why}")),
            (Outcome::Passed | Outcome::Inapplicable(_) | Outcome::CantTell(_), Some((_, why))) => {
                wrong.push(format!("{id} is listed as failing ({why}) and does not fail"))
            },
            _ => {},
        }
    }
    let count = |f: fn(&Outcome) -> bool| outcomes.iter().filter(|(_, o)| f(o)).count();
    let counts = (
        count(|o| matches!(o, Outcome::Passed)),
        count(|o| matches!(o, Outcome::Failed(_))),
        count(|o| matches!(o, Outcome::Inapplicable(_))),
        count(|o| matches!(o, Outcome::CantTell(_))),
    );
    let summary: Vec<String> = outcomes
        .iter()
        .map(|(id, outcome)| match outcome {
            Outcome::Passed => format!("{id} passed"),
            Outcome::Failed(_) => format!("{id} FAILED"),
            Outcome::Inapplicable(why) => format!("{id} inapplicable: {why}"),
            Outcome::CantTell(why) => format!("{id} cannot tell: {why}"),
        })
        .collect();
    assert!(wrong.is_empty(), "{}\n\n{}", wrong.join("\n\n"), summary.join("\n"));
    assert_eq!(
        counts,
        (46, 3, 8, 5),
        "(passed, failed, inapplicable, cannot tell)\n{}",
        summary.join("\n")
    );
}
