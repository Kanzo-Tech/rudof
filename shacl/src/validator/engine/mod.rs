mod focus_nodes_ops;
mod native;
#[cfg(feature = "sparql")]
mod sparql;
mod validate;
mod value_nodes_ops;

use crate::ir::{IRComponent, IRPropertyShape, IRSchema, IRShape, ShapeLabelIdx};
use crate::types::Target;
use rudof_iri::IriS;
use rudof_rdf::term::Object;
use rudof_rdf::{NeighsRDF, SHACLPath};
#[cfg(feature = "sparql")]
use std::collections::HashSet;

use crate::error::ValidationError;
use crate::validator::nodes::{FocusNodes, ValueNodes};
use crate::validator::report::ValidationResult;
pub use native::NativeEngine;
#[cfg(feature = "sparql")]
use rudof_rdf::query::QueryRDF;
#[cfg(feature = "sparql")]
pub use sparql::SparqlEngine;
pub use validate::{Validate, validate_focus};

pub trait Engine<S: NeighsRDF>: Sized {
    /// Creates a fresh sibling engine for a parallel task: it copies the
    /// borrowed read-only context (a `Copy` of an `&ref`, e.g. the class index)
    /// and starts with an empty owned cache. No `Box`, no `Arc`: it returns
    /// `Self`, so the engine is never type-erased and never crosses a thread
    /// boundary (each task forks its own inside the worker closure).
    fn fork(&self) -> Self;

    fn evaluate(
        &mut self,
        store: &S,
        shape: &IRShape,
        component: &IRComponent,
        value_nodes: &ValueNodes<S>,
        source_shape: Option<&IRShape>,
        maybe_path: Option<&SHACLPath>,
        shapes_graph: &IRSchema,
    ) -> Result<Vec<ValidationResult>, ValidationError>;

    fn focus_nodes(&self, store: &S, targets: &[Target]) -> Result<FocusNodes<S>, ValidationError> {
        let mut acc: Vec<S::Term> = Vec::new();
        for target in targets {
            let resolved = match target {
                Target::Node(n) => self.target_node(store, n)?,
                Target::Class(c) => self.target_class(store, c)?,
                Target::SubjectsOf(p) => self.target_subject_of(store, p)?,
                Target::ObjectsOf(p) => self.target_object_of(store, p)?,
                Target::ImplicitClass(n) => self.implicit_target_class(store, n)?,
                // Malformed targets propagate a typed error instead of panicking.
                Target::WrongNode(_)
                | Target::WrongClass(_)
                | Target::WrongSubjectsOf(_)
                | Target::WrongObjectsOf(_)
                | Target::WrongImplicitClass(_) => {
                    return Err(ValidationError::MalformedTarget(
                        "target value has the wrong term kind".to_string(),
                    ));
                },
            };
            acc.extend(resolved);
        }

        Ok(FocusNodes::from_iter(acc))
    }

    /// If s is a shape in a shapes graph SG and s has value t for sh:targetNode
    /// in SG then { t } is a target from any data graph for s in SG.
    fn target_node(&self, store: &S, node: &Object) -> Result<FocusNodes<S>, ValidationError>;

    fn target_class(&self, store: &S, class: &Object) -> Result<FocusNodes<S>, ValidationError>;

    fn target_subject_of(&self, store: &S, predicate: &IriS) -> Result<FocusNodes<S>, ValidationError>;

    fn target_object_of(&self, store: &S, predicate: &IriS) -> Result<FocusNodes<S>, ValidationError>;

    fn implicit_target_class(&self, store: &S, shape: &Object) -> Result<FocusNodes<S>, ValidationError>;

    fn path(&self, store: &S, shape: &IRPropertyShape, focus_node: &S::Term) -> Result<FocusNodes<S>, ValidationError> {
        let nodes = store.objects_for_shacl_path(focus_node, shape.path())?;

        Ok(FocusNodes::new(nodes))
    }

    fn record_validation(&mut self, node: Object, shape_idx: ShapeLabelIdx, results: Vec<ValidationResult>);

    fn has_validated(&self, node: &Object, shape_idx: ShapeLabelIdx) -> bool;

    /// Borrows the cached validation results for a given `(node, shape_idx)`
    /// pair, if any. The cache is an owned `HashMap` now, so there is no lock
    /// guard to tie a lifetime to — return a slice.
    fn get_cached_results(&self, node: &Object, shape_idx: ShapeLabelIdx) -> Option<&[ValidationResult]>;
}

#[cfg(feature = "sparql")]
fn select<S: QueryRDF>(store: &S, query: &str, index: &str) -> Result<HashSet<S::Term>, ValidationError> {
    let mut out = HashSet::new();

    let query = store
        .query_select(query)
        .map_err(ValidationError::select_query_error::<S>)?;

    for sol in query.iter() {
        if let Some(sol) = sol.find_solution(index) {
            out.insert(sol.to_owned());
        }
    }

    Ok(out)
}
