mod focus_nodes_ops;
mod native;
#[cfg(feature = "sparql")]
mod sparql;
mod test;
mod validate;
mod value_nodes_ops;

use crate::ir::{IRComponent, IRPropertyShape, IRSchema, IRShape, ShapeLabelIdx};
use crate::types::Target;
use rudof_iri::IriS;
use rudof_rdf::term::{Object, Triple};
use rudof_rdf::{NeighsRDF, SHACLPath};
use std::collections::HashSet;
use std::fmt::Debug;

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

    fn focus_nodes(
        &mut self,
        store: &S,
        targets: &[Target],
        shapes_graph: &IRSchema,
    ) -> Result<FocusNodes<S>, ValidationError>
    where
        S: Debug,
    {
        let mut acc: Vec<S::Term> = Vec::new();
        for target in targets {
            let resolved = match target {
                Target::Node(n) => self.target_node(store, n)?,
                Target::Class(c) => self.target_class(store, c)?,
                Target::SubjectsOf(p) => self.target_subject_of(store, p)?,
                Target::ObjectsOf(p) => self.target_object_of(store, p)?,
                Target::ImplicitClass(n) => self.implicit_target_class(store, n)?,
                Target::Where(w) => self.target_where(store, w, shapes_graph)?,
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

    /// SHACL 1.2 Core §3.1.3.6: if `s` has value `w` for `sh:targetWhere`, the
    /// nodes of the data graph that conform to `w` are a target for `s`.
    ///
    /// The nodes of a graph are the subjects and objects of its triples
    /// (RDF 1.2 Concepts), so literals are candidates and an IRI that only
    /// occurs as a predicate is not. Conformance is scoped validation of one
    /// node against `w` yielding no result, whatever its severity; `w`'s own
    /// targets play no part, so a where shape that is itself targeted, or that
    /// refers back, cannot recurse. One pass collects the candidates and `w`
    /// is compiled once, in `shapes_graph`; the engine's cache is shared by
    /// every candidate.
    fn target_where(
        &mut self,
        store: &S,
        shape: &Object,
        shapes_graph: &IRSchema,
    ) -> Result<FocusNodes<S>, ValidationError>
    where
        S: Debug,
    {
        let idx = *shapes_graph
            .get_idx(shape)
            .ok_or_else(|| ValidationError::MalformedTarget(format!("sh:targetWhere value {shape} is not a shape")))?;
        let mut candidates: HashSet<S::Term> = HashSet::new();
        for triple in store.triples().map_err(ValidationError::new_graph_error::<S>)? {
            candidates.insert(triple.subj().clone().into());
            candidates.insert(triple.obj().clone());
        }
        let mut conforming = HashSet::new();
        for node in candidates {
            let object = S::term_as_object(&node)?;
            if validate_focus(store, shapes_graph, self, idx, &object)?.is_empty() {
                conforming.insert(node);
            }
        }
        Ok(FocusNodes::new(conforming))
    }

    /// Whether `node` is a **SHACL instance** of `class`.
    ///
    /// SHACL §1.1 defines the SHACL types of a term as its `rdf:type` values
    /// together with the SHACL superclasses of those values — the transitive
    /// `rdfs:subClassOf` closure — and a term is a SHACL instance of a class
    /// when that class is among its SHACL types.
    ///
    /// Deliberately expressed through [`target_class`](Engine::target_class)
    /// rather than by a traversal of its own. `sh:targetClass` selects exactly
    /// the SHACL instances of its class (§2.1.3.2) and `sh:class` requires
    /// exactly that its value nodes *be* SHACL instances of its class (§4.4.1):
    /// one fact, so one implementation, and no way for target selection and
    /// value checking to drift apart on how far the class hierarchy reaches.
    /// Each engine therefore answers with whatever machinery it already uses to
    /// select targets — the native engine's cycle-safe closure over the
    /// pre-built class index, the SPARQL engine's `rdfs:subClassOf*` path.
    fn is_shacl_instance(&self, store: &S, node: &S::Term, class: &Object) -> Result<bool, ValidationError> {
        Ok(self.target_class(store, class)?.iter().any(|t| t == node))
    }

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
