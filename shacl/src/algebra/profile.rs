//! The shapes the engine does not check, and why.
//!
//! What [`denote`](super::denote) gives a meaning to is stated once, as a
//! SHACL 1.2 Profiling profile (`profile.ttl`): a `prof:Profile` of SHACL whose
//! `rdfs:member`s are the constraint components and target declarations a
//! shape may use, in the form of the Working Group's own example profile. A shapes graph is never refused as
//! a whole. A shape is **unchecked** when
//!
//! - it uses a constraint component or a target declaration the profile does
//!   not list (`sh:SPARQLConstraintComponent`, …);
//! - it lies on a cycle of the dependency graph: SHACL leaves the semantics of
//!   recursive shapes undefined, and recursion is no vocabulary, so this half
//!   is structural;
//! - a component of it (`sh:node`, `sh:and`, …), its `sh:targetWhere` or its
//!   reifier shape refers to a shape whose conformance cannot be decided: one
//!   that is unchecked, or has an unchecked property shape.
//!
//! Every other shape is checked, and a report lists the unchecked ones beside
//! its results.

use crate::algebra::DenoteError;
use crate::ir::{IRComponent, IRSchema, IRShape, ShapeLabelIdx};
use crate::types::Target;
use rudof_iri::IriS;
use rudof_rdf::backend::{OxigraphInMemory, ReaderMode};
use rudof_rdf::term::Object;
use rudof_rdf::term::Triple as _;
use rudof_rdf::vocab::{RdfsVocab, ShaclVocab};
use rudof_rdf::{NeighsRDF, RDFFormat, Rdf};
use std::collections::{HashMap, HashSet};
use std::fmt::{Display, Formatter};
use std::sync::OnceLock;

/// The profile, as written.
pub const PROFILE: &str = include_str!("profile.ttl");

/// A shape the engine does not check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unchecked {
    /// The shape: the node that declares it in the shapes graph.
    pub shape: Object,
    pub reason: Reason,
}

/// Why a shape is not checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reason {
    /// It uses a SHACL element outside the profile.
    Outside(IriS),
    /// It refers to itself, through other shapes or not.
    Recursive,
    /// It refers to this shape, whose conformance is not decided.
    Depends(Object),
}

impl Display for Reason {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Reason::Outside(iri) => write!(f, "{iri} is outside the engine's profile"),
            Reason::Recursive => write!(f, "recursive shapes have no SHACL semantics"),
            Reason::Depends(shape) => write!(f, "it depends on {shape}, which is not checked"),
        }
    }
}

/// The profile's members.
pub fn members() -> &'static HashSet<IriS> {
    static MEMBERS: OnceLock<HashSet<IriS>> = OnceLock::new();
    MEMBERS.get_or_init(|| {
        let graph = OxigraphInMemory::from_str(PROFILE, &RDFFormat::Turtle, None, &ReaderMode::Strict)
            .expect("the profile parses");
        graph
            .triples_with_predicate(&RdfsVocab::rdfs_member().into())
            .expect("the profile is read")
            .filter_map(|t| match OxigraphInMemory::term_as_object(&t.into_components().2) {
                Ok(Object::Iri(iri)) => Some(iri),
                _ => None,
            })
            .collect()
    })
}

/// The unchecked shapes of `schema`, by index.
pub(crate) fn unchecked(schema: &IRSchema) -> Result<HashMap<ShapeLabelIdx, Unchecked>, DenoteError> {
    let mut out: HashMap<ShapeLabelIdx, Unchecked> = HashMap::new();
    let shapes: HashMap<ShapeLabelIdx, &IRShape> = schema
        .iter()
        .filter_map(|(id, shape)| schema.get_idx(id).map(|idx| (*idx, shape)))
        .collect();
    let mark = |out: &mut HashMap<ShapeLabelIdx, Unchecked>, idx: ShapeLabelIdx, reason: Reason| {
        if let Some(shape) = shapes.get(&idx) {
            out.entry(idx).or_insert_with(|| Unchecked {
                shape: shape.id().clone(),
                reason,
            });
        }
    };

    for (idx, shape) in &shapes {
        if let Some(outside) = vocabulary(shape).into_iter().find(|e| !members().contains(e)) {
            mark(&mut out, *idx, Reason::Outside(outside));
        }
    }
    for idx in schema.dependency_graph().recursive() {
        mark(&mut out, idx, Reason::Recursive);
    }

    // A reference to an undecided shape leaves the referrer undecided. The
    // dependency graph is acyclic off the shapes already marked, so this
    // reaches a fixpoint.
    loop {
        let undecided = undecided(&shapes, &out);
        let mut more = Vec::new();
        for (idx, shape) in &shapes {
            if out.contains_key(idx) {
                continue;
            }
            if let Some(r) = references(schema, shape).into_iter().find(|r| undecided.contains(r)) {
                more.push((*idx, Reason::Depends(shapes[&r].id().clone())));
            }
        }
        if more.is_empty() {
            return Ok(out);
        }
        for (idx, reason) in more {
            mark(&mut out, idx, reason);
        }
    }
}

/// The shapes whose conformance is not decided: the unchecked ones, and those
/// with an undecided property shape.
fn undecided(
    shapes: &HashMap<ShapeLabelIdx, &IRShape>,
    unchecked: &HashMap<ShapeLabelIdx, Unchecked>,
) -> HashSet<ShapeLabelIdx> {
    let mut out: HashSet<ShapeLabelIdx> = unchecked.keys().copied().collect();
    loop {
        let more: Vec<ShapeLabelIdx> = shapes
            .iter()
            .filter(|(idx, shape)| !out.contains(idx) && shape.property_shapes().iter().any(|p| out.contains(p)))
            .map(|(idx, _)| *idx)
            .collect();
        if more.is_empty() {
            return out;
        }
        out.extend(more);
    }
}

/// The elements of the profile's vocabulary `shape` uses: its constraint
/// components, named as a report names them, and its target declarations.
fn vocabulary(shape: &IRShape) -> Vec<IriS> {
    let mut out = Vec::new();
    for component in shape.components() {
        match component {
            // A declaration of the shape, not a constraint.
            IRComponent::Deactivated(_) => {},
            IRComponent::QualifiedValueShape(qvs) => {
                if qvs.qualified_min_count().is_some() {
                    out.push(ShaclVocab::sh_qualified_min_count_constraint_component());
                }
                if qvs.qualified_max_count().is_some() {
                    out.push(ShaclVocab::sh_qualified_max_count_constraint_component());
                }
            },
            other => out.push(IriS::from(other)),
        }
    }
    if shape.reifier_info().is_some() {
        out.push(ShaclVocab::sh_reifier_shape_constraint_component());
    }
    for target in shape.targets() {
        out.push(match target {
            Target::Node(_) | Target::WrongNode(_) => ShaclVocab::sh_target_node(),
            Target::Class(_) | Target::WrongClass(_) => ShaclVocab::sh_target_class(),
            Target::SubjectsOf(_) | Target::WrongSubjectsOf(_) => ShaclVocab::sh_target_subjects_of(),
            Target::ObjectsOf(_) | Target::WrongObjectsOf(_) => ShaclVocab::sh_target_objects_of(),
            // A class that is a shape: no SHACL element is written for it.
            Target::ImplicitClass(_) | Target::WrongImplicitClass(_) => continue,
            Target::Where(_) => ShaclVocab::sh_target_where(),
        });
    }
    out
}

/// The shapes whose conformance `shape`'s results depend on.
fn references(schema: &IRSchema, shape: &IRShape) -> Vec<ShapeLabelIdx> {
    let mut out: Vec<ShapeLabelIdx> = Vec::new();
    for component in shape.components() {
        out.extend(component.shapes().into_iter().map(|(idx, _)| idx));
        if let IRComponent::QualifiedValueShape(qvs) = component {
            out.extend(qvs.siblings().iter().copied());
        }
    }
    for target in shape.targets() {
        if let Target::Where(node) = target {
            out.extend(schema.get_idx(node).copied());
        }
    }
    if let Some(info) = shape.reifier_info() {
        out.extend(info.reifier_shape().iter().copied());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_profile_names_no_sparql() {
        assert!(members().len() > 40, "{} members", members().len());
        assert!(!members().contains(&ShaclVocab::sh_sparql_constraint_component()));
        assert!(members().contains(&ShaclVocab::sh_min_count_constraint_component()));
    }
}
