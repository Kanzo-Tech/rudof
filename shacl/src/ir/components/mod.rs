//! Compiled-state SHACL components.
//!
//! Only the components that gain real compiled state over their AST form keep a
//! dedicated struct here: `Pattern` (compiled `RDFRegex`), `Closed` (resolved
//! permitted properties), `BasicSparql` (parsed query), `If` and
//! `QualifiedValueShape`. Every other component lives INLINE in
//! [`crate::ir::IRComponent`] (e.g. `IRComponent::MinCount(isize)`,
//! `IRComponent::And(Vec<ShapeLabelIdx>)`).

mod basic_sparql;
mod closed;
mod if_;
mod pattern;
mod qualified_value_shape;

pub use basic_sparql::BasicSparql;
pub use closed::Closed;
pub use if_::If;
pub use pattern::Pattern;
pub use qualified_value_shape::QualifiedValueShape;
