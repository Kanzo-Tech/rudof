use crate::error::ValidationError;
use crate::ir::{IRNodeShape, IRPropertyShape, IRShape};
use crate::validator::engine::Engine;
use crate::validator::nodes::{FocusNodes, ValueNodes};
use rudof_rdf::rdf_core::NeighsRDF;

pub(crate) trait ValueNodesOps<RDF: NeighsRDF> {
    fn value_nodes<E: Engine<RDF>>(
        &self,
        store: &RDF,
        focus_nodes: &FocusNodes<RDF>,
        engine: &E,
    ) -> Result<ValueNodes<RDF>, ValidationError>;
}

impl<RDF: NeighsRDF> ValueNodesOps<RDF> for IRShape {
    fn value_nodes<E: Engine<RDF>>(
        &self,
        store: &RDF,
        focus_nodes: &FocusNodes<RDF>,
        engine: &E,
    ) -> Result<ValueNodes<RDF>, ValidationError> {
        match self {
            IRShape::NodeShape(ns) => ns.value_nodes(store, focus_nodes, engine),
            IRShape::PropertyShape(ps) => ps.value_nodes(store, focus_nodes, engine),
        }
    }
}

impl<RDF: NeighsRDF> ValueNodesOps<RDF> for IRNodeShape {
    fn value_nodes<E: Engine<RDF>>(
        &self,
        _: &RDF,
        focus_nodes: &FocusNodes<RDF>,
        _: &E,
    ) -> Result<ValueNodes<RDF>, ValidationError> {
        let value_nodes = focus_nodes.iter().map(|n| (n.clone(), FocusNodes::single(n.clone())));
        Ok(ValueNodes::from_iter(value_nodes))
    }
}

impl<RDF: NeighsRDF> ValueNodesOps<RDF> for IRPropertyShape {
    fn value_nodes<E: Engine<RDF>>(
        &self,
        store: &RDF,
        focus_nodes: &FocusNodes<RDF>,
        engine: &E,
    ) -> Result<ValueNodes<RDF>, ValidationError> {
        // `?`-propagate path resolution failures (was a silent `filter_map` drop).
        let mut pairs = Vec::with_capacity(focus_nodes.len());
        for n in focus_nodes.iter() {
            let ts = engine.path(store, self, n)?;
            pairs.push((n.clone(), ts));
        }
        Ok(ValueNodes::from_iter(pairs))
    }
}
