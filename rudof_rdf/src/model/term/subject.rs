use crate::term::TermKind;
use std::fmt::{Debug, Display};
use std::hash::Hash;

/// Represents the subject position of an RDF triple.
///
/// In RDF, a subject can be an IRI, a blank node, or (in RDF-star) a triple.
/// This trait defines the common behavior for all types that can appear as
/// subjects in RDF statements.
pub trait Subject: Debug + Display + PartialEq + Clone + Eq + Hash {
    /// Returns the kind of RDF term this subject represents.
    ///
    /// This method allows distinguishing between IRIs, blank nodes, and quoted triples at runtime.
    fn kind(&self) -> TermKind;

    /// Returns `true` if this subject is an IRI.
    fn is_iri(&self) -> bool {
        self.kind() == TermKind::Iri
    }

    /// Returns `true` if this subject is a blank node.
    fn is_blank_node(&self) -> bool {
        self.kind() == TermKind::BlankNode
    }

    /// Returns `true` if this subject is a quoted triple (RDF-star).
    fn is_triple(&self) -> bool {
        self.kind() == TermKind::Triple
    }
}
