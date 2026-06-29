use crate::error::ValidationError;
use crate::ir::{IRComponent, IRSchema, IRShape, ShapeLabelIdx};
use crate::validator::cache::ValidationCache;
use crate::validator::constraints::validate_native;
use crate::validator::engine::Engine;
use crate::validator::index::ClassIndex;
use crate::validator::nodes::{FocusNodes, ValueNodes};
use crate::validator::report::ValidationResult;
use rudof_iri::IriS;
use rudof_rdf::rdf_core::term::{Object, Term, Triple};
use rudof_rdf::rdf_core::vocabs::{RdfVocab, RdfsVocab};
use rudof_rdf::rdf_core::{NeighsRDF, SHACLPath};
use std::fmt::Debug;

/// Native (in-memory) validation engine.
///
/// Borrows a shared, read-only `ClassIndex` (`Sync`, no `Arc`) and owns its
/// validation cache (a plain `HashMap`, mutated via `&mut self`). It contains
/// no `Arc` and no interior mutability, so `&NativeEngine` is `Sync` and can be
/// shared across rayon threads while each task forks its own owned engine.
pub struct NativeEngine<'e> {
    /// Borrowed inverted index mapping classes to instances/subclasses.
    class_index: Option<&'e ClassIndex>,
    /// Owned per-engine validation cache.
    cache: ValidationCache,
}

impl<'e> NativeEngine<'e> {
    pub(crate) fn new(class_index: Option<&'e ClassIndex>) -> Self {
        Self {
            class_index,
            cache: ValidationCache::default(),
        }
    }
}

impl<RDF: NeighsRDF + Debug + 'static> Engine<RDF> for NativeEngine<'_> {
    fn fork(&self) -> Self {
        // Copy the borrowed index ref; start with a fresh empty cache.
        NativeEngine {
            class_index: self.class_index,
            cache: ValidationCache::default(),
        }
    }

    fn evaluate(
        &mut self,
        store: &RDF,
        shape: &IRShape,
        component: &IRComponent,
        value_nodes: &ValueNodes<RDF>,
        source_shape: Option<&IRShape>,
        maybe_path: Option<&SHACLPath>,
        shapes_graph: &IRSchema,
    ) -> Result<Vec<ValidationResult>, ValidationError> {
        // Static dispatch over the IRComponent enum — no trait object.
        validate_native::<RDF, Self>(
            component,
            shape,
            store,
            self,
            value_nodes,
            source_shape,
            maybe_path,
            shapes_graph,
        )
    }

    /// https://www.w3.org/TR/shacl/#targetNode
    fn target_node(&self, _: &RDF, node: &Object) -> Result<FocusNodes<RDF>, ValidationError> {
        let node: RDF::Term = node.clone().into();
        if node.is_blank_node() {
            Err(ValidationError::TargetNodeBNode)
        } else {
            Ok(FocusNodes::single(node.clone()))
        }
    }

    fn target_class(&self, store: &RDF, class: &Object) -> Result<FocusNodes<RDF>, ValidationError> {
        // use the pre-built class index (O(1) lookup)
        if let Some(index) = self.class_index {
            let focus_nodes = index.instances_of(class).map(|obj| -> RDF::Term { obj.clone().into() });
            return Ok(FocusNodes::from_iter(focus_nodes));
        }

        // Fallback: full graph scan (for backwards compatibility if index wasn't built)
        let cls: RDF::Term = class.clone().into();
        let focus_nodes = store
            .shacl_instances_of(&cls)
            .map_err(ValidationError::new_graph_error::<RDF>)?
            .map(|s| RDF::subject_as_term(&s));

        Ok(FocusNodes::from_iter(focus_nodes))
    }

    fn target_subject_of(&self, store: &RDF, predicate: &IriS) -> Result<FocusNodes<RDF>, ValidationError> {
        let pred: RDF::IRI = predicate.clone().into();
        let subjects = store
            .triples_with_predicate(&pred)
            .map_err(ValidationError::new_graph_error::<RDF>)?
            .map(Triple::into_subject)
            .map(Into::into);
        Ok(FocusNodes::from_iter(subjects))
    }

    fn target_object_of(&self, store: &RDF, predicate: &IriS) -> Result<FocusNodes<RDF>, ValidationError> {
        let pred: RDF::IRI = predicate.clone().into();
        let objects = store
            .triples_with_predicate(&pred)
            .map_err(ValidationError::new_graph_error::<RDF>)?
            .map(Triple::into_object);
        Ok(FocusNodes::from_iter(objects))
    }

    fn implicit_target_class(&self, store: &RDF, shape: &Object) -> Result<FocusNodes<RDF>, ValidationError> {
        // use the pre-built class index (O(1) lookup)
        if let Some(index) = self.class_index {
            let instances = index.instances_of_with_subclasses(shape);
            let focus_nodes = instances.into_iter().map(|obj| -> RDF::Term { obj.clone().into() });
            return Ok(FocusNodes::from_iter(focus_nodes));
        }

        // Fallback: full graph scan (for backwards compatibility if index wasn't built)
        let term: RDF::Term = shape.clone().into();
        let targets = store.subjects_for(&RdfVocab::rdf_type().into(), &term)?;

        let subclass_targets = store
            .subjects_for(&RdfsVocab::rdfs_subclass_of_str().into(), &term)?
            .into_iter()
            .flat_map(move |subclass| {
                store
                    .subjects_for(&RdfVocab::rdf_type().into(), &subclass)
                    .into_iter()
                    .flatten()
            });

        Ok(FocusNodes::from_iter(targets.into_iter().chain(subclass_targets)))
    }

    fn record_validation(&mut self, node: Object, shape_idx: ShapeLabelIdx, results: Vec<ValidationResult>) {
        self.cache.record(node, shape_idx, results);
    }

    fn has_validated(&self, node: &Object, shape_idx: ShapeLabelIdx) -> bool {
        self.cache.has_validated(node, shape_idx)
    }

    fn get_cached_results(&self, node: &Object, shape_idx: ShapeLabelIdx) -> Option<&[ValidationResult]> {
        self.cache.get_results(node, shape_idx)
    }
}
