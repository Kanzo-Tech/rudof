//! The W3C SHACL test suites, run as the W3C publishes them: from their
//! manifests (`mf:include`, `mf:entries`), never from a list kept by hand.
//!
//! Each suite is embedded at compile time from the pinned `data-shapes`
//! submodule, so the same runner reads it natively and on wasm32, where there
//! is no file system. The in-memory interpretation runs on both; the SQL one
//! needs DuckDB and runs natively.
//!
//! A suite passes when every case does, in every interpretation, except the
//! ones `LEFT_OUT` names; a listed case that passes is red too, so the list
//! only ever shrinks.
//!
//! The suites are also the corpus of a differential test: each case's shapes
//! validate graphs drawn at random from the terms of its data, and the two
//! interpretations must give the same report, messages included. A difference
//! is shrunk to the smallest graph that shows it.

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

/// Cases of features left out on purpose, by id (`<suite>/<path>`), each with
/// why.
const LEFT_OUT: &[(&str, &str)] = &[(
    "shacl12/targets/shape-001",
    "sh:shape (SHACL 1.2 §3.1.3.7) waits until the draft's section settles",
)];

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
        self.each_case(|id, manifest, entry| {
            outcomes.insert(id, self.case(manifest, entry, interpretation));
        });
        outcomes
    }

    /// Calls `f` with the id, manifest and entry of every case of every
    /// manifest reachable from the root one.
    fn each_case(&self, mut f: impl FnMut(String, &mut OxigraphInMemory, &Term)) {
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
                    f(id, &mut graph, &entry);
                }
            }
        }
    }

    /// The data graph and the compiled shapes of the entry `entry`.
    fn inputs(&self, manifest: &OxigraphInMemory, entry: &Term) -> Result<(OxigraphInMemory, IRSchema), String> {
        let action = one(manifest, entry, MF, "action")?;
        let graph_of = |name: &str| -> Result<OxigraphInMemory, String> {
            self.graph(&unbracket(one(manifest, &action, SHT, name)?.to_string()))
        };
        let data = graph_of("dataGraph")?;
        let shapes = ShaclParser::new(graph_of("shapesGraph")?)
            .parse()
            .map_err(|e| format!("shapes: {e}"))?;
        let schema = IRSchema::try_from(&shapes).map_err(|e| format!("shapes: {e}"))?;
        Ok((data, schema))
    }

    /// Whether the `sht:Validate` entry `entry` gives its expected report.
    fn case(
        &self,
        manifest: &mut OxigraphInMemory,
        entry: &Term,
        interpretation: Interpretation,
    ) -> Result<(), String> {
        let (data, schema) = self.inputs(manifest, entry)?;
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

    /// Runs the suite: every case holds but the ones `LEFT_OUT` names, which
    /// fail.
    fn check(&self, interpretation: Interpretation) {
        let outcomes = self.run(interpretation);
        assert!(!outcomes.is_empty(), "{} has no cases", self.name);
        let left_out = |id: &str| LEFT_OUT.iter().any(|(case, _)| *case == id);
        let mut failures: Vec<String> = outcomes
            .iter()
            .filter_map(|(id, outcome)| match (outcome, left_out(id)) {
                (Err(why), false) => Some(format!("FAILS {id}\n{why}")),
                (Ok(()), true) => Some(format!("PASSES {id}, listed in LEFT_OUT")),
                _ => None,
            })
            .collect();
        failures.extend(
            LEFT_OUT
                .iter()
                .filter(|(id, _)| id.starts_with(&format!("{}/", self.name)) && !outcomes.contains_key(*id))
                .map(|(id, _)| format!("UNKNOWN {id}, listed in LEFT_OUT but not a case")),
        );
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

/// The two interpretations agree on graphs drawn from each case's terms.
#[cfg(not(target_family = "wasm"))]
mod differential {
    use super::*;
    use oxrdf::{NamedOrBlankNode, Triple};
    use proptest::collection::vec;
    use proptest::sample::subsequence;
    use proptest::test_runner::{Config, RngAlgorithm, TestCaseError, TestError, TestRng, TestRunner};

    /// Graphs drawn per case.
    const GRAPHS: u32 = 16;

    /// At most this many triples added to a draw from the case's own.
    const ADDED: usize = 6;

    /// The terms a case's graphs are drawn from: its data's triples, and the
    /// subjects, predicates and objects they hold.
    struct Terms {
        triples: Vec<Triple>,
        subjects: Vec<NamedOrBlankNode>,
        predicates: Vec<NamedNode>,
        objects: Vec<Term>,
    }

    impl Terms {
        fn of(data: &OxigraphInMemory) -> Terms {
            let triples: Vec<Triple> = data.triples().expect("an in-memory graph lists its triples").collect();
            let mut subjects: Vec<NamedOrBlankNode> = triples.iter().map(|t| t.subject.clone()).collect();
            let mut predicates: Vec<NamedNode> = triples.iter().map(|t| t.predicate.clone()).collect();
            let mut objects: Vec<Term> = triples.iter().map(|t| t.object.clone()).collect();
            // A node that is only ever an object is drawn as a subject too.
            subjects.extend(objects.iter().filter_map(|o| match o {
                Term::NamedNode(n) => Some(n.clone().into()),
                Term::BlankNode(b) => Some(b.clone().into()),
                _ => None,
            }));
            objects.extend(subjects.iter().cloned().map(Term::from));
            for pool in [&mut subjects] {
                pool.sort_by_key(ToString::to_string);
                pool.dedup();
            }
            predicates.sort();
            predicates.dedup();
            objects.sort_by_key(ToString::to_string);
            objects.dedup();
            Terms {
                triples,
                subjects,
                predicates,
                objects,
            }
        }

        /// The graph of `kept` and of the triples `added` indexes.
        fn graph(&self, kept: &[Triple], added: &[(usize, usize, usize)]) -> OxigraphInMemory {
            let added = added.iter().map(|&(s, p, o)| {
                Triple::new(
                    self.subjects[s].clone(),
                    self.predicates[p].clone(),
                    self.objects[o].clone(),
                )
            });
            let text: String = kept.iter().cloned().chain(added).map(|t| format!("{t} .\n")).collect();
            OxigraphInMemory::from_str(&text, &RDFFormat::NTriples, None, &ReaderMode::Strict)
                .unwrap_or_else(|e| panic!("a drawn graph reads back: {e}\n{text}"))
        }
    }

    /// A report as the sorted spellings of its results, messages included in
    /// language order (a message map is a hash map, in no order of its own).
    fn spelled(report: Result<ValidationReport, String>) -> Result<Vec<String>, String> {
        let mut results: Vec<String> = report?
            .results()
            .iter()
            .map(|r| {
                let mut messages: Vec<String> = r
                    .message()
                    .messages()
                    .iter()
                    .map(|(lang, text)| format!("{text:?}@{lang:?}"))
                    .collect();
                messages.sort();
                format!(
                    "{} {} {:?} {:?} {:?} {:?} {:?} {messages:?}",
                    r.focus_node(),
                    r.constraint_component(),
                    r.severity(),
                    r.path(),
                    r.value(),
                    r.source(),
                    r.details(),
                )
            })
            .collect();
        results.sort();
        Ok(results)
    }

    /// The smallest graph drawn for `id` on which the interpretations differ.
    fn agree(id: &str, data: &OxigraphInMemory, schema: &IRSchema) -> Result<(), String> {
        let terms = Terms::of(data);
        if terms.predicates.is_empty() {
            return Ok(());
        }
        let added = vec(
            (
                0..terms.subjects.len(),
                0..terms.predicates.len(),
                0..terms.objects.len(),
            ),
            0..=ADDED,
        );
        let strategy = (subsequence(terms.triples.clone(), 0..=terms.triples.len()), added);
        let mut runner = TestRunner::new_with_rng(
            Config {
                cases: GRAPHS,
                failure_persistence: None,
                ..Config::default()
            },
            TestRng::deterministic_rng(RngAlgorithm::ChaCha),
        );
        runner
            .run(&strategy, |(kept, added)| {
                let graph = terms.graph(&kept, &added);
                let eval = spelled(Interpretation::Eval.validate(schema, &graph));
                let sql = spelled(Interpretation::Sql.validate(schema, &graph));
                if eval == sql {
                    Ok(())
                } else {
                    Err(TestCaseError::fail(format!(
                        "eval {eval:#?}\nsql {sql:#?}\ngraph\n{}",
                        kept.iter()
                            .map(|t| format!("{t} ."))
                            .chain(added.iter().map(|&(s, p, o)| format!(
                                "{} {} {} .",
                                terms.subjects[s], terms.predicates[p], terms.objects[o]
                            )))
                            .collect::<Vec<_>>()
                            .join("\n")
                    )))
                }
            })
            .map_err(|e| match e {
                TestError::Fail(why, _) => format!("DIFFERS {id}\n{why}"),
                TestError::Abort(why) => format!("ABORTED {id}\n{why}"),
            })
    }

    fn check(suite: &Suite) {
        let mut differences = Vec::new();
        suite.each_case(|id, manifest, entry| {
            let (data, schema) = suite.inputs(manifest, entry).unwrap_or_else(|e| panic!("{id}: {e}"));
            if let Err(why) = agree(&id, &data, &schema) {
                differences.push(why);
            }
        });
        assert!(
            differences.is_empty(),
            "{}: {} cases differ\n\n{}",
            suite.name,
            differences.len(),
            differences.join("\n\n")
        );
    }

    #[test]
    fn shacl_1_0_core_in_memory_is_sql() {
        check(&SHACL_1_0);
    }

    #[test]
    fn shacl_1_2_core_in_memory_is_sql() {
        check(&SHACL_1_2);
    }
}
