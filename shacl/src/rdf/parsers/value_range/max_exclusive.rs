use crate::ast::ASTComponent;
use rudof_rdf::rdf_core::NeighsRDF;
use rudof_rdf::rdf_core::parser::rdf_node_parser::constructors::LiteralsPropertyParser;
use rudof_rdf::rdf_core::parser::rdf_node_parser::{ParserExt, RDFNodeParse};
use rudof_rdf::rdf_core::vocabs::ShaclVocab;

pub(crate) fn max_exclusive<RDF: NeighsRDF>() -> impl RDFNodeParse<RDF, Output = Vec<ASTComponent>> {
    LiteralsPropertyParser::new(ShaclVocab::sh_max_exclusive())
        .map(|ns| ns.into_iter().map(ASTComponent::MaxExclusive).collect())
}
