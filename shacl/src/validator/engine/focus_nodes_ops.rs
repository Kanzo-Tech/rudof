use crate::error::ValidationError;
use crate::ir::{IRSchema, IRShape};
use crate::validator::engine::Engine;
use crate::validator::nodes::FocusNodes;
use rudof_rdf::NeighsRDF;
use std::fmt::Debug;

pub(crate) trait FocusNodesOps<RDF: NeighsRDF + Debug> {
    fn focus_nodes<E: Engine<RDF>>(
        &self,
        store: &RDF,
        engine: &mut E,
        shapes_graph: &IRSchema,
    ) -> Result<FocusNodes<RDF>, ValidationError>;
}

impl<RDF: NeighsRDF + Debug> FocusNodesOps<RDF> for IRShape {
    fn focus_nodes<E: Engine<RDF>>(
        &self,
        store: &RDF,
        engine: &mut E,
        shapes_graph: &IRSchema,
    ) -> Result<FocusNodes<RDF>, ValidationError> {
        // Bubble the typed error (MalformedTarget / graph error) instead of `.expect`.
        engine.focus_nodes(store, self.targets(), shapes_graph)
    }
}
