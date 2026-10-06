//! The W3C SHACL test suites, run as the W3C publishes them: from their
//! manifests (`mf:include`, `mf:entries`), never from a list kept by hand.
//!
//! Each suite is embedded at compile time from the pinned `data-shapes`
//! submodule, so the same runner reads it natively and on wasm32, where there
//! is no file system. The in-memory interpretation runs on both; the SQL one
//! needs DuckDB and runs natively.
//!
//! A suite passes when every case does, in every interpretation.

use include_dir::{Dir, include_dir};
use oxrdf::{NamedNode, Term};
use rudof_rdf::backend::{OxigraphInMemory, ReaderMode};
use rudof_rdf::term::Object;
use rudof_rdf::{NeighsRDF, RDFFormat};
use shacl::ir::IRSchema;
use shacl::rdf::ShaclParser;
use shacl::validator::report::ValidationReport;
use std::collections::BTreeMap;

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

/// The base the suite's relative IRIs resolve against: `<base><suite>/<path>`.
const BASE: &str = "http://w3c-test.invalid/";

const MF: &str = "http://www.w3.org/2001/sw/DataAccess/tests/test-manifest#";
const SHT: &str = "http://www.w3.org/ns/shacl-test#";
const RDF: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";
const SH: &str = "http://www.w3.org/ns/shacl#";

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
        let expected =
            ValidationReport::parse(manifest, result.clone()).map_err(|e| format!("expected report: {e}"))?;
        let actual = interpretation.validate(&schema, &data)?;
        if actual != expected {
            return Err(format!(
                "expected {:#?}\nactual {:#?}",
                expected.results(),
                actual.results()
            ));
        }
        messages_hold(manifest, &result, &actual)
    }

    /// Runs the suite: every case holds.
    fn check(&self, interpretation: Interpretation) {
        let outcomes = self.run(interpretation);
        assert!(!outcomes.is_empty(), "{} has no cases", self.name);
        let failures: Vec<String> = outcomes
            .iter()
            .filter_map(|(id, outcome)| outcome.as_ref().err().map(|why| format!("FAILS {id}\n{why}")))
            .collect();
        assert!(
            failures.is_empty(),
            "{} ({}): {} of {} pass\n\n{}",
            self.name,
            interpretation.name(),
            outcomes.len() - failures.len(),
            outcomes.len(),
            failures.join("\n\n")
        );
    }
}

/// The messages the expected report states: as the W3C harness does, a
/// result's `sh:resultMessage` is compared only where the expected report
/// gives one, and then the actual result with its focus node and component
/// carries each of them, language tag included.
fn messages_hold(manifest: &OxigraphInMemory, report: &Term, actual: &ValidationReport) -> Result<(), String> {
    for result in objects(manifest, report, SH, "result") {
        let wanted = objects(manifest, &result, SH, "resultMessage");
        if wanted.is_empty() {
            continue;
        }
        let object = |local: &str| -> Result<Object, String> {
            Object::try_from(one(manifest, &result, SH, local)?).map_err(|e| e.to_string())
        };
        let (focus, component) = (object("focusNode")?, object("sourceConstraintComponent")?);
        let carries = |r: &&shacl::validator::report::ValidationResult| {
            wanted.iter().all(|w| match w {
                Term::Literal(l) => r.message().messages().iter().any(|(lang, text)| {
                    text == l.value() && lang.as_ref().map(ToString::to_string).as_deref() == l.language()
                }),
                _ => false,
            })
        };
        let held = actual
            .results()
            .iter()
            .filter(|r| r.focus_node() == &focus && r.constraint_component() == &component)
            .any(|r| carries(&r));
        if !held {
            return Err(format!(
                "no result for {focus} of {component} has the messages {}",
                wanted.iter().map(Term::to_string).collect::<Vec<_>>().join(", ")
            ));
        }
    }
    Ok(())
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
