use crate::ast::ASTComponent;
use crate::ast::node_shape::ASTNodeShape;
use crate::ast::property_shape::ASTPropertyShape;
use crate::types::Annotations;
use rudof_iri::IriS;
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};

// `Clone`/`PartialEq` stay hand-written (below) to keep the boxed-variant
// semantics; serde derives coexist with them.
#[derive(Debug, Serialize, Deserialize)]
pub enum ASTShape {
    NodeShape(Box<ASTNodeShape>),
    PropertyShape(Box<ASTPropertyShape>),
}

impl ASTShape {
    /// Creates a node shape
    pub fn node_shape(ns: ASTNodeShape) -> Self {
        Self::NodeShape(Box::new(ns))
    }

    /// Creates a property shape
    pub fn property_shape(ps: ASTPropertyShape) -> Self {
        Self::PropertyShape(Box::new(ps))
    }

    pub fn components(&self) -> &Vec<ASTComponent> {
        match self {
            Self::NodeShape(ns) => ns.components(),
            Self::PropertyShape(ps) => ps.components(),
        }
    }

    fn with_components(self, components: Vec<ASTComponent>) -> Self {
        match self {
            Self::NodeShape(ns) => Self::node_shape(ns.with_components(components)),
            Self::PropertyShape(ps) => Self::property_shape(ps.with_components(components)),
        }
    }

    /// Whether the shape is `sh:closed sh:ByTypes`.
    pub fn closed_by_types(&self) -> bool {
        self.components()
            .iter()
            .any(|c| matches!(c, ASTComponent::Closed { by_types: Some(_), .. }))
    }

    /// The shape with `properties` as what `sh:closed sh:ByTypes` permits for
    /// each type.
    pub fn with_properties_by_type(self, properties: &[(IriS, Vec<IriS>)]) -> Self {
        let components = self
            .components()
            .iter()
            .cloned()
            .map(|c| match c {
                ASTComponent::Closed {
                    is_closed,
                    ignored_properties,
                    by_types: Some(_),
                } => ASTComponent::Closed {
                    is_closed,
                    ignored_properties,
                    by_types: Some(properties.to_vec()),
                },
                other => other,
            })
            .collect();
        self.with_components(components)
    }

    pub fn with_annotations(self, annotations: Annotations) -> Self {
        match self {
            Self::NodeShape(ns) => Self::node_shape(ns.with_annotations(annotations)),
            Self::PropertyShape(ps) => Self::property_shape(ps.with_annotations(annotations)),
        }
    }
}

impl Display for ASTShape {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ASTShape::NodeShape(ns) => write!(f, "{ns}"),
            ASTShape::PropertyShape(ps) => write!(f, "{ps}"),
        }
    }
}

impl Clone for ASTShape {
    fn clone(&self) -> Self {
        match self {
            ASTShape::NodeShape(ns) => Self::NodeShape((*ns).clone()),
            ASTShape::PropertyShape(ps) => Self::PropertyShape((*ps).clone()),
        }
    }
}

impl PartialEq for ASTShape {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::NodeShape(l), Self::NodeShape(r)) => l == r,
            (Self::PropertyShape(l), Self::PropertyShape(r)) => l == r,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::ast::{ASTNodeShape, ASTShape};
    use rudof_iri::iri;
    use rudof_rdf::term::Object;

    #[test]
    fn test_clone() {
        let ns = ASTNodeShape::new(Object::Iri(iri!("http://example.org/id")));
        let s1 = ASTShape::node_shape(ns);
        let s2 = s1.clone();
        assert_eq!(s1, s2)
    }
}
