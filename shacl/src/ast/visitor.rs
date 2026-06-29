//! A generic visitor over [`ASTComponent`].
//!
//! The variant decomposition lives in exactly one place — [`ASTComponent::accept`] —
//! so every consumer (Display, constraint-IRI mapping, IR compilation, form
//! projection, …) overrides only the arms it cares about and shares a single
//! source of truth for the component shape. Generics, not `Box<dyn>`: each
//! consumer is a concrete `ComponentVisitor` impl monomorphised at the call
//! site.
//!
//! A visitor declares its `Output`/`Error` types and a `default_component`
//! fallback; the per-variant methods default to that fallback, so a projection
//! that cares about three components writes three methods and ignores the rest.

use crate::ast::ASTComponent;
use crate::types::{MessageMap, NodeKind, Value};
use prefixmap::{IriRef, PrefixMap};
use rudof_iri::IriS;
use rudof_rdf::term::Object;
use rudof_rdf::term::literal::{ConcreteLiteral, Lang};
use std::collections::HashSet;

/// Fold over a single SHACL component. Override the arms of interest; the rest
/// fall through to [`ComponentVisitor::default_component`].
pub trait ComponentVisitor {
    /// Value produced for each visited component.
    type Output;
    /// Error raised while visiting.
    type Error;

    /// Fallback used by every `visit_*` arm not overridden by the implementor.
    fn default_component(&mut self) -> Result<Self::Output, Self::Error>;

    // value type
    fn visit_class(&mut self, _class: &Object) -> Result<Self::Output, Self::Error> {
        self.default_component()
    }
    fn visit_datatype(&mut self, _datatype: &IriRef) -> Result<Self::Output, Self::Error> {
        self.default_component()
    }
    fn visit_node_kind(&mut self, _node_kind: &NodeKind) -> Result<Self::Output, Self::Error> {
        self.default_component()
    }

    // cardinality
    fn visit_min_count(&mut self, _count: isize) -> Result<Self::Output, Self::Error> {
        self.default_component()
    }
    fn visit_max_count(&mut self, _count: isize) -> Result<Self::Output, Self::Error> {
        self.default_component()
    }

    // value range
    fn visit_min_exclusive(&mut self, _lit: &ConcreteLiteral) -> Result<Self::Output, Self::Error> {
        self.default_component()
    }
    fn visit_max_exclusive(&mut self, _lit: &ConcreteLiteral) -> Result<Self::Output, Self::Error> {
        self.default_component()
    }
    fn visit_min_inclusive(&mut self, _lit: &ConcreteLiteral) -> Result<Self::Output, Self::Error> {
        self.default_component()
    }
    fn visit_max_inclusive(&mut self, _lit: &ConcreteLiteral) -> Result<Self::Output, Self::Error> {
        self.default_component()
    }

    // string
    fn visit_min_length(&mut self, _len: isize) -> Result<Self::Output, Self::Error> {
        self.default_component()
    }
    fn visit_max_length(&mut self, _len: isize) -> Result<Self::Output, Self::Error> {
        self.default_component()
    }
    fn visit_pattern(&mut self, _pattern: &str, _flags: Option<&str>) -> Result<Self::Output, Self::Error> {
        self.default_component()
    }
    fn visit_unique_lang(&mut self, _unique: bool) -> Result<Self::Output, Self::Error> {
        self.default_component()
    }
    fn visit_language_in(&mut self, _langs: &[Lang]) -> Result<Self::Output, Self::Error> {
        self.default_component()
    }

    // property pair
    fn visit_equals(&mut self, _iri: &IriRef) -> Result<Self::Output, Self::Error> {
        self.default_component()
    }
    fn visit_disjoint(&mut self, _iri: &IriRef) -> Result<Self::Output, Self::Error> {
        self.default_component()
    }
    fn visit_less_than(&mut self, _iri: &IriRef) -> Result<Self::Output, Self::Error> {
        self.default_component()
    }
    fn visit_less_than_or_equals(&mut self, _iri: &IriRef) -> Result<Self::Output, Self::Error> {
        self.default_component()
    }

    // logical (carry shape refs so the IR compiler can intern them)
    fn visit_or(&mut self, _shapes: &[Object]) -> Result<Self::Output, Self::Error> {
        self.default_component()
    }
    fn visit_and(&mut self, _shapes: &[Object]) -> Result<Self::Output, Self::Error> {
        self.default_component()
    }
    fn visit_not(&mut self, _shape: &Object) -> Result<Self::Output, Self::Error> {
        self.default_component()
    }
    fn visit_xone(&mut self, _shapes: &[Object]) -> Result<Self::Output, Self::Error> {
        self.default_component()
    }

    // shape-based
    fn visit_closed(&mut self, _is_closed: bool, _ignored: &HashSet<IriS>) -> Result<Self::Output, Self::Error> {
        self.default_component()
    }
    fn visit_node(&mut self, _shape: &Object) -> Result<Self::Output, Self::Error> {
        self.default_component()
    }
    fn visit_has_value(&mut self, _value: &Value) -> Result<Self::Output, Self::Error> {
        self.default_component()
    }
    fn visit_in(&mut self, _values: &[Value]) -> Result<Self::Output, Self::Error> {
        self.default_component()
    }
    fn visit_qualified_value_shape(
        &mut self,
        _shape: &Object,
        _q_min_count: Option<isize>,
        _q_max_count: Option<isize>,
        _disjoint: Option<bool>,
        _siblings: &[Object],
    ) -> Result<Self::Output, Self::Error> {
        self.default_component()
    }

    // status / sparql
    fn visit_deactivated(&mut self, _deactivated: bool) -> Result<Self::Output, Self::Error> {
        self.default_component()
    }
    fn visit_basic_sparql(
        &mut self,
        _select: &str,
        _message: Option<&MessageMap>,
        _deactivated: Option<bool>,
        _prefixes: Option<&PrefixMap>,
    ) -> Result<Self::Output, Self::Error> {
        self.default_component()
    }
}

impl ASTComponent {
    /// Dispatch this component to the matching `visit_*` method. This is the
    /// single, exhaustive source of truth for component-variant decomposition.
    pub fn accept<V: ComponentVisitor>(&self, visitor: &mut V) -> Result<V::Output, V::Error> {
        match self {
            ASTComponent::Class(class) => visitor.visit_class(class),
            ASTComponent::Datatype(dt) => visitor.visit_datatype(dt),
            ASTComponent::NodeKind(nk) => visitor.visit_node_kind(nk),
            ASTComponent::MinCount(n) => visitor.visit_min_count(*n),
            ASTComponent::MaxCount(n) => visitor.visit_max_count(*n),
            ASTComponent::MinExclusive(lit) => visitor.visit_min_exclusive(lit),
            ASTComponent::MaxExclusive(lit) => visitor.visit_max_exclusive(lit),
            ASTComponent::MinInclusive(lit) => visitor.visit_min_inclusive(lit),
            ASTComponent::MaxInclusive(lit) => visitor.visit_max_inclusive(lit),
            ASTComponent::MinLength(len) => visitor.visit_min_length(*len),
            ASTComponent::MaxLength(len) => visitor.visit_max_length(*len),
            ASTComponent::Pattern { pattern, flags } => visitor.visit_pattern(pattern, flags.as_deref()),
            ASTComponent::UniqueLang(b) => visitor.visit_unique_lang(*b),
            ASTComponent::LanguageIn(langs) => visitor.visit_language_in(langs),
            ASTComponent::Equals(iri) => visitor.visit_equals(iri),
            ASTComponent::Disjoint(iri) => visitor.visit_disjoint(iri),
            ASTComponent::LessThan(iri) => visitor.visit_less_than(iri),
            ASTComponent::LessThanOrEquals(iri) => visitor.visit_less_than_or_equals(iri),
            ASTComponent::Or(shapes) => visitor.visit_or(shapes),
            ASTComponent::And(shapes) => visitor.visit_and(shapes),
            ASTComponent::Not(shape) => visitor.visit_not(shape),
            ASTComponent::Xone(shapes) => visitor.visit_xone(shapes),
            ASTComponent::Closed {
                is_closed,
                ignored_properties,
            } => visitor.visit_closed(*is_closed, ignored_properties),
            ASTComponent::Node(shape) => visitor.visit_node(shape),
            ASTComponent::HasValue(value) => visitor.visit_has_value(value),
            ASTComponent::In(values) => visitor.visit_in(values),
            ASTComponent::QualifiedValueShape {
                shape,
                q_min_count,
                q_max_count,
                disjoint,
                siblings,
            } => visitor.visit_qualified_value_shape(shape, *q_min_count, *q_max_count, *disjoint, siblings),
            ASTComponent::Deactivated(b) => visitor.visit_deactivated(*b),
            ASTComponent::BasicSparql {
                select,
                message,
                deactivated,
                prefixes,
            } => visitor.visit_basic_sparql(select, message.as_ref(), *deactivated, prefixes.as_ref()),
        }
    }
}
