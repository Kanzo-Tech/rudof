use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum NodeKind {
    Iri,
    Lit,
    BNode,
    BNodeOrIri,
    BNodeOrLit,
    IriOrLit,
    /// SHACL 1.2: an RDF 1.2 triple term.
    TripleTerm,
}

impl Display for NodeKind {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            NodeKind::Iri => write!(f, "Iri"),
            NodeKind::Lit => write!(f, "Literal"),
            NodeKind::BNode => write!(f, "BlankNode"),
            NodeKind::BNodeOrIri => write!(f, "BlankNodeOrIri"),
            NodeKind::BNodeOrLit => write!(f, "BlankNodeOrLiteral"),
            NodeKind::IriOrLit => write!(f, "IriOrLiteral"),
            NodeKind::TripleTerm => write!(f, "TripleTerm"),
        }
    }
}
