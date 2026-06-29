//! Fluent builders for the SHACL AST.
//!
//! The AST types only offer per-field `with_*` setters; these builders make
//! construction (and tests) readable and centralise the node-vs-property and
//! unique-id invariants. Setters are infallible; `build()` is fallible and never
//! panics.

use crate::ast::{ASTComponent, ASTNodeShape, ASTPropertyShape, ASTSchema, ASTShape};
use crate::types::{Presentation, Target, Value};
use prefixmap::{IriRef, PrefixMap};
use rudof_iri::IriS;
use rudof_rdf::rdf_core::SHACLPath;
use rudof_rdf::rdf_core::term::Object;
use rudof_rdf::rdf_core::term::literal::NumericLiteral;
use std::collections::HashMap;
use thiserror::Error;

/// Errors raised when finalising a builder.
#[derive(Debug, Error)]
pub enum BuildError {
    #[error("duplicate shape id in schema: {0}")]
    DuplicateShapeId(Box<Object>),
}

/// Builds an [`ASTPropertyShape`]. The path is required up front (a property
/// shape without a path is not representable).
pub struct PropertyShapeBuilder {
    id: Object,
    path: SHACLPath,
    components: Vec<ASTComponent>,
    targets: Vec<Target>,
    property_shapes: Vec<Object>,
    order: Option<NumericLiteral>,
    default_value: Option<Value>,
    presentation: Presentation,
}

impl PropertyShapeBuilder {
    pub fn new(id: impl Into<Object>, path: SHACLPath) -> Self {
        Self {
            id: id.into(),
            path,
            components: Vec::new(),
            targets: Vec::new(),
            property_shapes: Vec::new(),
            order: None,
            default_value: None,
            presentation: Presentation::default(),
        }
    }

    /// Convenience for the common predicate-path case.
    pub fn predicate(id: impl Into<Object>, pred: IriS) -> Self {
        Self::new(id, SHACLPath::iri(pred))
    }

    pub fn component(mut self, c: ASTComponent) -> Self {
        self.components.push(c);
        self
    }

    pub fn datatype(self, dt: IriRef) -> Self {
        self.component(ASTComponent::Datatype(dt))
    }

    pub fn class(self, class: impl Into<Object>) -> Self {
        self.component(ASTComponent::Class(class.into()))
    }

    pub fn min_count(self, n: isize) -> Self {
        self.component(ASTComponent::MinCount(n))
    }

    pub fn max_count(self, n: isize) -> Self {
        self.component(ASTComponent::MaxCount(n))
    }

    pub fn node_kind(self, nk: crate::types::NodeKind) -> Self {
        self.component(ASTComponent::NodeKind(nk))
    }

    pub fn target(mut self, target: Target) -> Self {
        self.targets.push(target);
        self
    }

    pub fn property_shape(mut self, ref_id: impl Into<Object>) -> Self {
        self.property_shapes.push(ref_id.into());
        self
    }

    pub fn order(mut self, order: NumericLiteral) -> Self {
        self.order = Some(order);
        self
    }

    pub fn default_value(mut self, value: Value) -> Self {
        self.default_value = Some(value);
        self
    }

    pub fn editor(mut self, editor: IriS) -> Self {
        self.presentation = std::mem::take(&mut self.presentation).with_editor(Some(editor));
        self
    }

    pub fn viewer(mut self, viewer: IriS) -> Self {
        self.presentation = std::mem::take(&mut self.presentation).with_viewer(Some(viewer));
        self
    }

    pub fn build(self) -> Result<ASTPropertyShape, BuildError> {
        Ok(ASTPropertyShape::new(self.id, self.path)
            .with_components(self.components)
            .with_targets(self.targets)
            .with_property_shapes(self.property_shapes)
            .with_order(self.order)
            .with_default_value(self.default_value)
            .with_presentation(self.presentation))
    }
}

/// Builds an [`ASTNodeShape`]. Node shapes carry no path/order/defaultValue
/// (those are property-shape concepts), so the builder simply doesn't offer them.
pub struct NodeShapeBuilder {
    id: Object,
    components: Vec<ASTComponent>,
    targets: Vec<Target>,
    property_shapes: Vec<Object>,
    presentation: Presentation,
}

impl NodeShapeBuilder {
    pub fn new(id: impl Into<Object>) -> Self {
        Self {
            id: id.into(),
            components: Vec::new(),
            targets: Vec::new(),
            property_shapes: Vec::new(),
            presentation: Presentation::default(),
        }
    }

    pub fn component(mut self, c: ASTComponent) -> Self {
        self.components.push(c);
        self
    }

    pub fn class(self, class: impl Into<Object>) -> Self {
        self.component(ASTComponent::Class(class.into()))
    }

    pub fn target(mut self, target: Target) -> Self {
        self.targets.push(target);
        self
    }

    pub fn target_class(self, class: impl Into<Object>) -> Self {
        self.target(Target::Class(class.into()))
    }

    pub fn property_shape(mut self, ref_id: impl Into<Object>) -> Self {
        self.property_shapes.push(ref_id.into());
        self
    }

    pub fn editor(mut self, editor: IriS) -> Self {
        self.presentation = std::mem::take(&mut self.presentation).with_editor(Some(editor));
        self
    }

    pub fn build(self) -> Result<ASTNodeShape, BuildError> {
        Ok(ASTNodeShape::new(self.id)
            .with_components(self.components)
            .with_targets(self.targets)
            .with_property_shapes(self.property_shapes)
            .with_presentation(self.presentation))
    }
}

/// Builds an [`ASTSchema`], rejecting duplicate shape ids.
#[derive(Default)]
pub struct SchemaBuilder {
    shapes: Vec<ASTShape>,
    prefixmap: PrefixMap,
}

impl SchemaBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn prefixmap(mut self, prefixmap: PrefixMap) -> Self {
        self.prefixmap = prefixmap;
        self
    }

    pub fn shape(mut self, shape: impl Into<ASTShape>) -> Self {
        self.shapes.push(shape.into());
        self
    }

    pub fn node_shape(self, ns: ASTNodeShape) -> Self {
        self.shape(ASTShape::node_shape(ns))
    }

    pub fn property_shape(self, ps: ASTPropertyShape) -> Self {
        self.shape(ASTShape::property_shape(ps))
    }

    pub fn build(self) -> Result<ASTSchema, BuildError> {
        let mut shapes: HashMap<Object, ASTShape> = HashMap::with_capacity(self.shapes.len());
        for shape in self.shapes {
            let id = match &shape {
                ASTShape::NodeShape(ns) => ns.id().clone(),
                ASTShape::PropertyShape(ps) => ps.id().clone(),
            };
            if shapes.insert(id.clone(), shape).is_some() {
                return Err(BuildError::DuplicateShapeId(Box::new(id)));
            }
        }
        Ok(ASTSchema::new().with_prefixmap(self.prefixmap).with_shapes(shapes))
    }
}

impl From<ASTNodeShape> for ASTShape {
    fn from(ns: ASTNodeShape) -> Self {
        ASTShape::node_shape(ns)
    }
}

impl From<ASTPropertyShape> for ASTShape {
    fn from(ps: ASTPropertyShape) -> Self {
        ASTShape::property_shape(ps)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rudof_iri::iri;

    #[test]
    fn builds_schema_with_invariants() {
        let ps = PropertyShapeBuilder::predicate(Object::iri(iri!("http://ex/namePS")), iri!("http://ex/name"))
            .datatype(IriRef::iri(iri!("http://www.w3.org/2001/XMLSchema#string")))
            .min_count(1)
            .build()
            .unwrap();

        let ns = NodeShapeBuilder::new(Object::iri(iri!("http://ex/Person")))
            .target_class(Object::iri(iri!("http://ex/Person")))
            .property_shape(Object::iri(iri!("http://ex/namePS")))
            .build()
            .unwrap();

        let schema = SchemaBuilder::new().node_shape(ns).property_shape(ps).build().unwrap();
        assert_eq!(schema.iter().count(), 2);
    }

    #[test]
    fn rejects_duplicate_ids() {
        let a = NodeShapeBuilder::new(Object::iri(iri!("http://ex/X"))).build().unwrap();
        let b = NodeShapeBuilder::new(Object::iri(iri!("http://ex/X"))).build().unwrap();
        let err = SchemaBuilder::new().node_shape(a).node_shape(b).build();
        assert!(matches!(err, Err(BuildError::DuplicateShapeId(_))));
    }
}
