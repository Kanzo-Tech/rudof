//! A graph read by subject, in one pass: a lookup by subject (and predicate) on the
//! store itself scans every quad, which is what made reading the annotations of
//! each property shape cost a pass over the graph apiece.

use std::collections::HashMap;

use rudof_lib::form::{NamedOrBlankNode, OxigraphInMemory, Quad, Term};

use crate::dto::LangString;

const RDFS_LABEL: &str = "http://www.w3.org/2000/01/rdf-schema#label";

pub(crate) struct SubjectIndex {
    pub(crate) by_subject: HashMap<NamedOrBlankNode, Vec<(String, Term)>>,
    /// The graph's `rdfs:label`s of each IRI subject.
    labels: Labels,
}

/// The `rdfs:label` literals of each IRI subject of a graph, by subject IRI: what
/// SHACL UI's Property Labels take from the data graph and the shapes graph for a
/// predicate.
#[derive(Default)]
pub(crate) struct Labels(HashMap<String, Vec<LangString>>);

impl Labels {
    pub(crate) fn from_quads(quads: impl Iterator<Item = Quad>) -> Self {
        let mut labels = Labels::default();
        for q in quads {
            labels.add(&q);
        }
        labels.sorted()
    }

    fn add(&mut self, q: &Quad) {
        if let (NamedOrBlankNode::NamedNode(s), true, Term::Literal(l)) =
            (&q.subject, q.predicate.as_str() == RDFS_LABEL, &q.object)
        {
            self.0.entry(s.as_str().to_string()).or_default().push(LangString {
                value: l.value().to_string(),
                language: l.language().unwrap_or("").to_string(),
            });
        }
    }

    /// The graph's order is not stable; the payload should be.
    fn sorted(mut self) -> Self {
        for ls in self.0.values_mut() {
            ls.sort_by(|a, b| (&a.language, &a.value).cmp(&(&b.language, &b.value)));
        }
        self
    }

    pub(crate) fn of(&self, iri: &str) -> Vec<LangString> {
        self.0.get(iri).cloned().unwrap_or_default()
    }
}

impl SubjectIndex {
    pub(crate) fn new(graph: &OxigraphInMemory) -> Self {
        let mut by_subject: HashMap<NamedOrBlankNode, Vec<(String, Term)>> = HashMap::new();
        let mut labels = Labels::default();
        for q in graph.quads() {
            labels.add(&q);
            by_subject
                .entry(q.subject)
                .or_default()
                .push((q.predicate.into_string(), q.object));
        }
        Self {
            by_subject,
            labels: labels.sorted(),
        }
    }

    pub(crate) fn labels(&self) -> &Labels {
        &self.labels
    }

    /// The `(predicate, object)` pairs of `subject`.
    pub(crate) fn about(&self, subject: &NamedOrBlankNode) -> &[(String, Term)] {
        self.by_subject.get(subject).map_or(&[], Vec::as_slice)
    }

    pub(crate) fn values<'a>(
        &'a self,
        subject: &NamedOrBlankNode,
        predicate: &'a str,
    ) -> impl Iterator<Item = &'a Term> + 'a {
        self.about(subject)
            .iter()
            .filter(move |(p, _)| p == predicate)
            .map(|(_, o)| o)
    }
}
