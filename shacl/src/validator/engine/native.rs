use crate::error::ValidationError;
use crate::ir::{IRComponent, IRSchema, IRShape, ShapeLabelIdx};
use crate::validator::cache::ValidationCache;
use crate::validator::constraints::validate_native;
use crate::validator::engine::Engine;
use crate::validator::index::ClassIndex;
use crate::validator::nodes::{FocusNodes, ValueNodes};
use crate::validator::report::ValidationResult;
use rudof_iri::IriS;
use rudof_rdf::term::{Object, Term, Triple};
use rudof_rdf::vocab::{RdfVocab, RdfsVocab};
use rudof_rdf::{NeighsRDF, SHACLPath};
use std::collections::HashSet;
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

    /// The SHACL instances of `class` (SHACL §1.1): every node whose `rdf:type`
    /// is `class` or any *transitive* `rdfs:subClassOf` subclass of it.
    ///
    /// Backs both `sh:targetClass` and the implicit class target, which select
    /// exactly this set (§2.1.3.2, §2.1.3.3).
    fn shacl_instances<RDF: NeighsRDF>(&self, store: &RDF, class: &Object) -> Result<FocusNodes<RDF>, ValidationError> {
        // Pre-built class index: the closure is walked over the index maps.
        if let Some(index) = self.class_index {
            let focus_nodes = index
                .shacl_instances_of(class)
                .into_iter()
                .map(|obj| -> RDF::Term { obj.clone().into() });
            return Ok(FocusNodes::from_iter(focus_nodes));
        }

        // Fallback: walk the graph (for backwards compatibility if index wasn't built).
        let rdf_type: RDF::IRI = RdfVocab::rdf_type().into();
        let subclass_of: RDF::IRI = RdfsVocab::rdfs_subclass_of_str().into();

        let mut instances: HashSet<RDF::Term> = HashSet::new();
        let mut seen: HashSet<RDF::Term> = HashSet::new();
        let mut pending: Vec<RDF::Term> = vec![class.clone().into()];

        // `seen` also makes a cyclic class hierarchy terminate.
        while let Some(cls) = pending.pop() {
            if !seen.insert(cls.clone()) {
                continue;
            }
            instances.extend(store.subjects_for(&rdf_type, &cls)?);
            pending.extend(store.subjects_for(&subclass_of, &cls)?);
        }

        Ok(FocusNodes::from_iter(instances))
    }
}

impl<RDF: NeighsRDF + Debug> Engine<RDF> for NativeEngine<'_> {
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

    /// https://www.w3.org/TR/shacl/#targetClass
    ///
    /// The targets are the *SHACL instances* of the class (§2.1.3.2), which by
    /// §1.1 include the instances of every transitive `rdfs:subClassOf`
    /// subclass, not only the directly typed nodes.
    fn target_class(&self, store: &RDF, class: &Object) -> Result<FocusNodes<RDF>, ValidationError> {
        self.shacl_instances(store, class)
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

    /// https://www.w3.org/TR/shacl/#implicit-targetClass
    ///
    /// Same target set as `sh:targetClass`, the shape itself playing the class.
    fn implicit_target_class(&self, store: &RDF, shape: &Object) -> Result<FocusNodes<RDF>, ValidationError> {
        self.shacl_instances(store, shape)
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
