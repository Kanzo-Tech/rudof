use crate::error::ValidationError;
use crate::ir::IRShape;
use crate::validator::engine::Engine;
use crate::validator::nodes::FocusNodes;
use rudof_rdf::NeighsRDF;

pub(crate) trait FocusNodesOps<RDF: NeighsRDF> {
    fn focus_nodes<E: Engine<RDF>>(&self, store: &RDF, engine: &E) -> Result<FocusNodes<RDF>, ValidationError>;
}

impl<RDF: NeighsRDF> FocusNodesOps<RDF> for IRShape {
    fn focus_nodes<E: Engine<RDF>>(&self, store: &RDF, engine: &E) -> Result<FocusNodes<RDF>, ValidationError> {
        // Bubble the typed error (MalformedTarget / graph error) instead of `.expect`.
        engine.focus_nodes(store, self.targets())
    }
}
