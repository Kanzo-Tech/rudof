use crate::ast::ASTComponent;
use rudof_rdf::NeighsRDF;
use rudof_rdf::parser::rdf_node_parser::constructors::LiteralsPropertyParser;
use rudof_rdf::parser::rdf_node_parser::{ParserExt, RDFNodeParse};
use rudof_rdf::vocab::ShaclVocab;

pub(crate) fn max_inclusive<RDF: NeighsRDF>() -> impl RDFNodeParse<RDF, Output = Vec<ASTComponent>> {
    LiteralsPropertyParser::new(ShaclVocab::sh_max_inclusive())
        .map(|ns| ns.into_iter().map(ASTComponent::MaxInclusive).collect())
}
