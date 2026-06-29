//! A generic visitor over [`IRComponent`].
//!
//! Mirrors [`crate::ast::visitor`] on the IR side: the variant decomposition
//! lives in exactly one place — [`IRComponent::accept`] — so every consumer
//! (dependency-graph edges, registration, Display, …) overrides only the arms
//! it cares about and shares a single source of truth for the compiled-component
//! shape. Generics, not `Box<dyn>`: each consumer is a concrete
//! `IRComponentVisitor` impl monomorphised at the call site.
//!
//! Arm payloads are passed **inlined** (the scalar/value the constraint carries)
//! for the components whose IR newtype is a trivial wrapper, and as the
//! *compiled* struct (`Pattern`, `Closed`, `BasicSparql`, `QualifiedValueShape`)
//! where the IR gains real state. This pre-stages the Stage-1 newtype-collapse:
//! when the trivial newtypes become inline `IRComponent` payloads, only
//! [`IRComponent::accept`] changes — every visitor keeps working untouched.

use crate::ir::ShapeLabelIdx;
use crate::ir::components::{BasicSparql, Closed, Pattern, QualifiedValueShape};
use crate::types::NodeKind;
use rudof_iri::IriS;
use rudof_rdf::rdf_core::term::Object;
use rudof_rdf::rdf_core::term::literal::{ConcreteLiteral, Lang};

/// Fold over a single compiled SHACL component. Override the arms of interest;
/// the rest fall through to [`IRComponentVisitor::default_component`].
pub trait IRComponentVisitor {
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
    fn visit_datatype(&mut self, _datatype: &IriS) -> Result<Self::Output, Self::Error> {
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
    fn visit_pattern(&mut self, _pattern: &Pattern) -> Result<Self::Output, Self::Error> {
        self.default_component()
    }
    fn visit_unique_lang(&mut self, _unique: bool) -> Result<Self::Output, Self::Error> {
        self.default_component()
    }
    fn visit_language_in(&mut self, _langs: &[Lang]) -> Result<Self::Output, Self::Error> {
        self.default_component()
    }

    // property pair
    fn visit_equals(&mut self, _iri: &IriS) -> Result<Self::Output, Self::Error> {
        self.default_component()
    }
    fn visit_disjoint(&mut self, _iri: &IriS) -> Result<Self::Output, Self::Error> {
        self.default_component()
    }
    fn visit_less_than(&mut self, _iri: &IriS) -> Result<Self::Output, Self::Error> {
        self.default_component()
    }
    fn visit_less_than_or_equals(&mut self, _iri: &IriS) -> Result<Self::Output, Self::Error> {
        self.default_component()
    }

    // logical (carry interned shape indices so consumers can walk the DG)
    fn visit_or(&mut self, _shapes: &[ShapeLabelIdx]) -> Result<Self::Output, Self::Error> {
        self.default_component()
    }
    fn visit_and(&mut self, _shapes: &[ShapeLabelIdx]) -> Result<Self::Output, Self::Error> {
        self.default_component()
    }
    fn visit_not(&mut self, _shape: ShapeLabelIdx) -> Result<Self::Output, Self::Error> {
        self.default_component()
    }
    fn visit_xone(&mut self, _shapes: &[ShapeLabelIdx]) -> Result<Self::Output, Self::Error> {
        self.default_component()
    }

    // shape-based
    fn visit_node(&mut self, _shape: ShapeLabelIdx) -> Result<Self::Output, Self::Error> {
        self.default_component()
    }
    fn visit_qualified_value_shape(&mut self, _qvs: &QualifiedValueShape) -> Result<Self::Output, Self::Error> {
        self.default_component()
    }
    fn visit_closed(&mut self, _closed: &Closed) -> Result<Self::Output, Self::Error> {
        self.default_component()
    }
    fn visit_has_value(&mut self, _value: &Object) -> Result<Self::Output, Self::Error> {
        self.default_component()
    }
    fn visit_in(&mut self, _values: &[Object]) -> Result<Self::Output, Self::Error> {
        self.default_component()
    }

    // status / sparql
    fn visit_deactivated(&mut self, _deactivated: bool) -> Result<Self::Output, Self::Error> {
        self.default_component()
    }
    fn visit_basic_sparql(&mut self, _sparql: &BasicSparql) -> Result<Self::Output, Self::Error> {
        self.default_component()
    }
}

use crate::ir::IRComponent;

impl IRComponent {
    /// Dispatch this compiled component to the matching `visit_*` method. This
    /// is the single, exhaustive source of truth for IR-component-variant
    /// decomposition (the mirror of [`crate::ast::ASTComponent::accept`]).
    pub fn accept<V: IRComponentVisitor>(&self, visitor: &mut V) -> Result<V::Output, V::Error> {
        match self {
            IRComponent::Class(c) => visitor.visit_class(c),
            IRComponent::Datatype(d) => visitor.visit_datatype(d),
            IRComponent::NodeKind(nk) => visitor.visit_node_kind(nk),
            IRComponent::MinCount(mc) => visitor.visit_min_count(*mc),
            IRComponent::MaxCount(mc) => visitor.visit_max_count(*mc),
            IRComponent::MinExclusive(me) => visitor.visit_min_exclusive(me),
            IRComponent::MaxExclusive(me) => visitor.visit_max_exclusive(me),
            IRComponent::MinInclusive(mi) => visitor.visit_min_inclusive(mi),
            IRComponent::MaxInclusive(mi) => visitor.visit_max_inclusive(mi),
            IRComponent::MinLength(ml) => visitor.visit_min_length(*ml),
            IRComponent::MaxLength(ml) => visitor.visit_max_length(*ml),
            IRComponent::Pattern(p) => visitor.visit_pattern(p),
            IRComponent::UniqueLang(ul) => visitor.visit_unique_lang(*ul),
            IRComponent::LanguageIn(langs) => visitor.visit_language_in(langs),
            IRComponent::Equals(e) => visitor.visit_equals(e),
            IRComponent::Disjoint(d) => visitor.visit_disjoint(d),
            IRComponent::LessThan(lt) => visitor.visit_less_than(lt),
            IRComponent::LessThanOrEquals(lte) => visitor.visit_less_than_or_equals(lte),
            IRComponent::Or(or) => visitor.visit_or(or.shapes()),
            IRComponent::And(and) => visitor.visit_and(and.shapes()),
            IRComponent::Not(not) => visitor.visit_not(*not.shape()),
            IRComponent::Xone(xone) => visitor.visit_xone(xone.shapes()),
            IRComponent::Node(node) => visitor.visit_node(*node.shape()),
            IRComponent::QualifiedValueShape(qvs) => visitor.visit_qualified_value_shape(qvs),
            IRComponent::Closed(closed) => visitor.visit_closed(closed),
            IRComponent::HasValue(hv) => visitor.visit_has_value(hv),
            IRComponent::In(values) => visitor.visit_in(values),
            IRComponent::Deactivated(d) => visitor.visit_deactivated(*d),
            IRComponent::BasicSparql(s) => visitor.visit_basic_sparql(s),
        }
    }
}
