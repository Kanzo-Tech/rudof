//! The W3C SHACL test suites, run as the W3C publishes them: from their
//! manifests (`mf:include`, `mf:entries`), never from a list kept by hand.
//!
//! Each suite is embedded at compile time from the pinned `data-shapes`
//! submodule, so the same runner reads it natively and on wasm32, where there
//! is no file system. The in-memory interpretation runs on both; the SQL one
//! needs DuckDB and runs natively.
//!
//! A suite passes when the cases that fail are exactly the ones
//! `w3c-expected-failures.txt` lists: a new failure is red, and so is a listed
//! case that has started to pass, so the list only ever shrinks.

use include_dir::{Dir, include_dir};
use oxrdf::{NamedNode, Term};
use rudof_rdf::backend::{OxigraphInMemory, ReaderMode};
use rudof_rdf::{NeighsRDF, RDFFormat};
use shacl::ir::IRSchema;
use shacl::rdf::ShaclParser;
use shacl::validator::report::ValidationReport;
use std::collections::{BTreeMap, BTreeSet};

#[cfg(target_family = "wasm")]
use wasm_bindgen_test::wasm_bindgen_test as test;

static SHACL_1_0: Suite = Suite {
    name: "shacl10",
    dir: &include_dir!("$CARGO_MANIFEST_DIR/tests/data-shapes/data-shapes-test-suite/tests/core"),
};

static SHACL_1_2: Suite = Suite {
    name: "shacl12",
    dir: &include_dir!("$CARGO_MANIFEST_DIR/tests/data-shapes/shacl12-test-suite/tests/core"),
};

/// One case id per line, `#` comments; an id followed by an interpretation
/// (`eval`, `sql`) fails only there.
const EXPECTED_FAILURES: &str = include_str!("w3c-expected-failures.txt");

/// The base the suite's relative IRIs resolve against: `<base><suite>/<path>`.
const BASE: &str = "http://w3c-test.invalid/";

const MF: &str = "http://www.w3.org/2001/sw/DataAccess/tests/test-manifest#";
const SHT: &str = "http://www.w3.org/ns/shacl-test#";
const RDF: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";

struct Suite {
    name: &'static str,
    dir: &'static Dir<'static>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Interpretation {
    Eval,
    #[cfg(not(target_family = "wasm"))]
    Sql,
}

impl Interpretation {
    fn name(self) -> &'static str {
        match self {
            Interpretation::Eval => "eval",
            #[cfg(not(target_family = "wasm"))]
            Interpretation::Sql => "sql",
        }
    }

    fn validate(self, schema: &IRSchema, data: &OxigraphInMemory) -> Result<ValidationReport, String> {
        match self {
            Interpretation::Eval => shacl::validator::validate(schema, data).map_err(|e| e.to_string()),
            #[cfg(not(target_family = "wasm"))]
            Interpretation::Sql => shacl::validator::sql::validate_with_duckdb(data, schema),
        }
    }
}

impl Suite {
    fn iri(&self, path: &str) -> String {
        format!("{BASE}{}/{path}", self.name)
    }

    /// The graph of the suite file `iri` names, its relative IRIs resolved
    /// against it.
    fn graph(&self, iri: &str) -> Result<OxigraphInMemory, String> {
        let path = iri
            .split('#')
            .next()
            .and_then(|i| i.strip_prefix(&self.iri("")))
            .ok_or_else(|| format!("{iri} is not a file of the suite"))?;
        let text = self
            .dir
            .get_file(path)
            .and_then(|f| f.contents_utf8())
            .ok_or_else(|| format!("{path} is not in the suite"))?;
        OxigraphInMemory::from_str(text, &RDFFormat::Turtle, Some(&self.iri(path)), &ReaderMode::Strict)
            .map_err(|e| format!("{path}: {e}"))
    }

    /// The id of the case `iri` names: its file's path, without `.ttl`.
    fn id(&self, iri: &str) -> String {
        let path = iri.strip_prefix(&self.iri("")).unwrap_or(iri);
        format!("{}/{}", self.name, path.trim_end_matches(".ttl"))
    }

    /// Every case of every manifest reachable from the root one: its id and
    /// whether it holds.
    fn run(&self, interpretation: Interpretation) -> BTreeMap<String, Result<(), String>> {
        let mut outcomes = BTreeMap::new();
        let mut manifests = vec![self.iri("manifest.ttl")];
        while let Some(manifest) = manifests.pop() {
            let mut graph = self
                .graph(&manifest)
                .unwrap_or_else(|e| panic!("manifest {manifest}: {e}"));
            let node = Term::NamedNode(NamedNode::new_unchecked(&manifest));
            manifests.extend(
                objects(&graph, &node, MF, "include")
                    .iter()
                    .map(Term::to_string)
                    .map(unbracket),
            );
            for list in objects(&graph, &node, MF, "entries") {
                for entry in members(&graph, list) {
                    let id = self.id(&unbracket(entry.to_string()));
                    outcomes.insert(id, self.case(&mut graph, &entry, interpretation));
                }
            }
        }
        outcomes
    }

    /// Whether the `sht:Validate` entry `entry` gives its expected report.
    fn case(
        &self,
        manifest: &mut OxigraphInMemory,
        entry: &Term,
        interpretation: Interpretation,
    ) -> Result<(), String> {
        let action = one(manifest, entry, MF, "action")?;
        let graph_of = |name: &str| -> Result<OxigraphInMemory, String> {
            self.graph(&unbracket(one(manifest, &action, SHT, name)?.to_string()))
        };
        let data = graph_of("dataGraph")?;
        let shapes = ShaclParser::new(graph_of("shapesGraph")?)
            .parse()
            .map_err(|e| format!("shapes: {e}"))?;
        let schema = IRSchema::try_from(&shapes).map_err(|e| format!("shapes: {e}"))?;
        let result = one(manifest, entry, MF, "result")?;
        let expected = ValidationReport::parse(manifest, result).map_err(|e| format!("expected report: {e}"))?;
        let actual = interpretation.validate(&schema, &data)?;
        if actual == expected {
            Ok(())
        } else {
            Err(format!(
                "expected {:#?}\nactual {:#?}",
                expected.results(),
                actual.results()
            ))
        }
    }

    /// Runs the suite and holds its failures to the expected ones.
    fn check(&self, interpretation: Interpretation) {
        let expected: BTreeSet<String> = EXPECTED_FAILURES
            .lines()
            .map(|l| {
                l.split('#')
                    .next()
                    .unwrap_or_default()
                    .split_whitespace()
                    .collect::<Vec<_>>()
            })
            .filter(|w| w.first().is_some_and(|id| id.starts_with(self.name)))
            .filter(|w| w.get(1).is_none_or(|i| *i == interpretation.name()))
            .map(|w| w[0].to_owned())
            .collect();
        let outcomes = self.run(interpretation);
        assert!(!outcomes.is_empty(), "{} has no cases", self.name);
        let mut wrong = Vec::new();
        for (id, outcome) in &outcomes {
            match (outcome, expected.contains(id)) {
                (Err(why), false) => wrong.push(format!("FAILS  {id}\n{why}")),
                (Ok(()), true) => wrong.push(format!("PASSES {id}, listed as an expected failure")),
                _ => {},
            }
        }
        wrong.extend(
            expected
                .iter()
                .filter(|id| !outcomes.contains_key(*id))
                .map(|id| format!("UNKNOWN {id}, listed but not a case")),
        );
        let passed = outcomes.values().filter(|o| o.is_ok()).count();
        assert!(
            wrong.is_empty(),
            "{} ({}): {passed} of {} pass\n\n{}",
            self.name,
            interpretation.name(),
            outcomes.len(),
            wrong.join("\n\n")
        );
    }
}

fn iri(namespace: &str, local: &str) -> NamedNode {
    NamedNode::new_unchecked(format!("{namespace}{local}"))
}

fn objects(graph: &OxigraphInMemory, subject: &Term, namespace: &str, local: &str) -> Vec<Term> {
    graph
        .objects_for(subject, &iri(namespace, local))
        .map(|set| set.into_iter().collect())
        .unwrap_or_default()
}

fn one(graph: &OxigraphInMemory, subject: &Term, namespace: &str, local: &str) -> Result<Term, String> {
    objects(graph, subject, namespace, local)
        .into_iter()
        .next()
        .ok_or_else(|| format!("{subject} has no {local}"))
}

/// The members of the RDF list `head`, in order.
fn members(graph: &OxigraphInMemory, mut head: Term) -> Vec<Term> {
    let mut out = Vec::new();
    while let Ok(first) = one(graph, &head, RDF, "first") {
        out.push(first);
        match one(graph, &head, RDF, "rest") {
            Ok(rest) => head = rest,
            Err(_) => break,
        }
    }
    out
}

/// `<iri>` as `iri`.
fn unbracket(term: String) -> String {
    term.trim_start_matches('<').trim_end_matches('>').to_owned()
}

#[test]
fn shacl_1_0_core_in_memory() {
    SHACL_1_0.check(Interpretation::Eval);
}

#[test]
fn shacl_1_2_core_in_memory() {
    SHACL_1_2.check(Interpretation::Eval);
}

#[cfg(not(target_family = "wasm"))]
#[test]
fn shacl_1_0_core_sql() {
    SHACL_1_0.check(Interpretation::Sql);
}

#[cfg(not(target_family = "wasm"))]
#[test]
fn shacl_1_2_core_sql() {
    SHACL_1_2.check(Interpretation::Sql);
}
