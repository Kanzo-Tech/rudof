use crate::types::{MessageMap, Severity};
use rudof_iri::IriS;
use rudof_rdf::term::Object;
use serde::{Deserialize, Serialize};

/// What the reifiers of a constraint's triples say about that one constraint
/// (SHACL 1.2 Core §2.1.3–§2.1.5): its severity, its messages, and whether it
/// is deactivated. Each overrides what the shape says for all its constraints.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Annotation {
    pub severity: Option<Severity>,
    pub message: Option<MessageMap>,
    pub deactivated: bool,
}

/// The annotations of a shape's triples, by the predicate and the object of
/// the triple they reify.
pub type Annotations = Vec<(IriS, Object, Annotation)>;

impl Annotation {
    /// The annotation of a constraint stated by the triples of `parameters`:
    /// what any of their reifiers says. SHACL allows at most one value of each
    /// property across them, so the first one found is the one.
    pub fn of(annotations: &Annotations, parameters: &[IriS]) -> Annotation {
        let mut out = Annotation::default();
        for (_, _, a) in annotations.iter().filter(|(p, _, _)| parameters.contains(p)) {
            out.severity = out.severity.or_else(|| a.severity.clone());
            out.message = out.message.or_else(|| a.message.clone());
            out.deactivated |= a.deactivated;
        }
        out
    }

    /// Whether a reifier deactivates the triple `(shape, predicate, object)`:
    /// how a single `sh:property` of a shape is switched off.
    pub fn deactivates(annotations: &Annotations, predicate: &IriS, object: &Object) -> bool {
        annotations
            .iter()
            .any(|(p, o, a)| a.deactivated && p == predicate && o == object)
    }
}
