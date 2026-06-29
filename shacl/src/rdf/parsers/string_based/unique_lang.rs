use crate::ast::ASTComponent;
use rudof_rdf::rdf_core::NeighsRDF;
use rudof_rdf::rdf_core::parser::rdf_node_parser::constructors::BoolsPropertyParser;
use rudof_rdf::rdf_core::parser::rdf_node_parser::{ParserExt, RDFNodeParse};
use rudof_rdf::rdf_core::vocabs::ShaclVocab;

pub(crate) fn unique_lang<RDF: NeighsRDF>() -> impl RDFNodeParse<RDF, Output = Vec<ASTComponent>> {
    BoolsPropertyParser::new(ShaclVocab::sh_unique_lang())
        .map(|ns| ns.into_iter().map(ASTComponent::UniqueLang).collect())
}
