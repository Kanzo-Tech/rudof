use crate::ast::ASTComponent;
use rudof_rdf::NeighsRDF;
use rudof_rdf::parser::rdf_node_parser::constructors::BoolsPropertyParser;
use rudof_rdf::parser::rdf_node_parser::{ParserExt, RDFNodeParse};
use rudof_rdf::vocab::ShaclVocab;

pub(crate) fn unique_lang<RDF: NeighsRDF>() -> impl RDFNodeParse<RDF, Output = Vec<ASTComponent>> {
    BoolsPropertyParser::new(ShaclVocab::sh_unique_lang())
        .map(|ns| ns.into_iter().map(ASTComponent::UniqueLang).collect())
}
