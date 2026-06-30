//! RDF visualization for `rudof_rdf`.
//!
//! This crate converts RDF graphs into a visual model ([`VisualRDFGraph`]) and
//! renders them as PlantUML / UML diagrams. It was extracted out of the core
//! `rudof_rdf` crate so that the core stays free of visualization concerns.

pub mod errors;
mod rdf_visualizer_config;
pub mod style;
pub mod uml_converter;
pub mod utils;
mod visual_rdf_edge;
mod visual_rdf_graph;
mod visual_rdf_node;

pub use rdf_visualizer_config::{RDFVisualizationConfig, UmlShape};
pub use visual_rdf_edge::VisualRDFEdge;
pub use visual_rdf_graph::{EdgeId, NodeId, VisualRDFGraph};
pub use visual_rdf_node::VisualRDFNode;
