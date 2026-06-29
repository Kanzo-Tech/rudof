use rudof_rdf::rdf_core::Rdf;
use std::collections::HashSet;
use std::collections::hash_set::IntoIter;
use std::fmt::{Display, Formatter};

/// Contains the set of focus nodes.
///
/// The set is **owned** (no `Arc`): focus nodes are built once by the engine
/// (`target_*`/`path`/value-node computation) and threaded by `&`. The previous
/// `Arc<HashSet>` only existed to make `clone()` cheap; we keep an explicit
/// `Clone` for the few save/restore sites but the container itself is plain.
#[derive(Debug)]
pub struct FocusNodes<RDF: Rdf> {
    set: HashSet<RDF::Term>,
}

impl<RDF: Rdf> FocusNodes<RDF> {
    pub fn new(set: HashSet<RDF::Term>) -> Self {
        Self { set }
    }

    /// Creates a [`FocusNodes`] containing exactly one node.
    pub fn single(node: RDF::Term) -> Self {
        let mut set = HashSet::with_capacity(1);
        set.insert(node);
        Self { set }
    }

    pub fn is_empty(&self) -> bool {
        self.set.is_empty()
    }

    pub fn len(&self) -> usize {
        self.set.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = &RDF::Term> {
        self.set.iter()
    }
}

impl<RDF: Rdf> Clone for FocusNodes<RDF> {
    fn clone(&self) -> Self {
        Self {
            set: self.set.clone(),
        }
    }
}

impl<RDF: Rdf> Default for FocusNodes<RDF> {
    fn default() -> Self {
        Self { set: HashSet::new() }
    }
}

impl<RDF: Rdf> FromIterator<RDF::Term> for FocusNodes<RDF> {
    fn from_iter<T: IntoIterator<Item = RDF::Term>>(iter: T) -> Self {
        Self {
            set: HashSet::from_iter(iter),
        }
    }
}

impl<RDF: Rdf> IntoIterator for FocusNodes<RDF> {
    type Item = RDF::Term;
    type IntoIter = IntoIter<RDF::Term>;

    fn into_iter(self) -> Self::IntoIter {
        self.set.into_iter()
    }
}

impl<RDF: Rdf> Display for FocusNodes<RDF> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "FocusNodes[{}]",
            self.set
                .iter()
                .map(|node| node.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}
