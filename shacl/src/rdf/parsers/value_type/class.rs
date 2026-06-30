use crate::ast::ASTComponent;
use rudof_rdf::NeighsRDF;
use rudof_rdf::parser::rdf_node_parser::constructors::ObjectsPropertyParser;
use rudof_rdf::parser::rdf_node_parser::{ParserExt, RDFNodeParse};
use rudof_rdf::vocab::ShaclVocab;

pub(crate) fn class<RDF: NeighsRDF>() -> impl RDFNodeParse<RDF, Output = Vec<ASTComponent>> {
    ObjectsPropertyParser::new(ShaclVocab::sh_class()).map(|ns| ns.into_iter().map(ASTComponent::Class).collect())
}
