//! Compiled-state SHACL components.
//!
//! Only the components that gain real compiled state over their AST form keep a
//! dedicated struct here: `Pattern` (compiled `RDFRegex`), `Closed` (resolved
//! ignored-property set), `BasicSparql` (parsed query), `QualifiedValueShape`,
//! and the logical/shape-based ones that carry interned [`ShapeLabelIdx`]
//! (`And`, `Or`, `Not`, `Xone`, `Node`). Every other component is a trivial
//! newtype over a scalar/term and lives INLINE in [`crate::ir::IRComponent`]
//! (e.g. `IRComponent::MinCount(isize)`), so it needs no struct here.

mod and;
mod basic_sparql;
mod closed;
mod node;
mod not;
mod or;
mod pattern;
mod qualified_value_shape;
mod xone;

pub use and::And;
pub use basic_sparql::BasicSparql;
pub use closed::Closed;
pub use node::Node;
pub use not::Not;
pub use or::Or;
pub use pattern::Pattern;
pub use qualified_value_shape::QualifiedValueShape;
pub use xone::Xone;
