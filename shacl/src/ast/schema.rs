use crate::ast::shape::ASTShape;
use prefixmap::PrefixMap;
use rudof_iri::IriS;
use rudof_rdf::term::Object;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::collections::btree_map::IntoIter;
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ASTSchema {
    // imports: Vec<IriS>
    // entailments: Vec<IriS>
    /// The shapes, keyed by their term. A set: a shapes graph has no order, so
    /// iteration is by the term's `Ord`, the same on every run.
    shapes: BTreeMap<Object, ASTShape>,
    prefixmap: PrefixMap,
    base: Option<IriS>,
}

impl ASTSchema {
    pub fn new() -> Self {
        ASTSchema {
            shapes: BTreeMap::new(),
            prefixmap: PrefixMap::new(),
            base: None,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.shapes.is_empty()
    }

    pub fn with_prefixmap(mut self, prefixmap: PrefixMap) -> Self {
        self.prefixmap = prefixmap;
        self
    }

    pub fn with_shapes(mut self, shapes: BTreeMap<Object, ASTShape>) -> Self {
        self.shapes = shapes;
        self
    }

    pub fn prefixmap(&self) -> &PrefixMap {
        &self.prefixmap
    }

    pub fn base(&self) -> Option<&IriS> {
        self.base.as_ref()
    }

    /// The shapes in the order of their terms (`Object`'s `Ord`), not the graph's.
    pub fn iter(&self) -> impl Iterator<Item = (&Object, &ASTShape)> {
        self.shapes.iter()
    }

    pub fn get_shape(&self, sref: &Object) -> Option<&ASTShape> {
        self.shapes.get(sref)
    }
}

impl IntoIterator for ASTSchema {
    type Item = (Object, ASTShape);
    type IntoIter = IntoIter<Object, ASTShape>;

    fn into_iter(self) -> Self::IntoIter {
        self.shapes.into_iter()
    }
}

impl Display for ASTSchema {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        for (id, shape) in self.shapes.iter() {
            writeln!(f, "{id} -> {shape}")?;
        }
        Ok(())
    }
}
