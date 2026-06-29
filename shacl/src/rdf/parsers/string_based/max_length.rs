use crate::ast::ASTComponent;
use rudof_rdf::rdf_core::NeighsRDF;
use rudof_rdf::rdf_core::parser::rdf_node_parser::constructors::IntegersPropertyParser;
use rudof_rdf::rdf_core::parser::rdf_node_parser::{ParserExt, RDFNodeParse};
use rudof_rdf::rdf_core::vocabs::ShaclVocab;

pub(crate) fn max_length<RDF: NeighsRDF>() -> impl RDFNodeParse<RDF, Output = Vec<ASTComponent>> {
    IntegersPropertyParser::new(ShaclVocab::sh_max_length())
        .map(|ns| ns.into_iter().map(ASTComponent::MaxLength).collect())
}
