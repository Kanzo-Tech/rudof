use crate::ast::ASTComponent;
use crate::rdf::parsers::utils::{parse_components_for_iri, term_to_value};
use rudof_rdf::NeighsRDF;
use rudof_rdf::parser::rdf_node_parser::constructors::ListParser;
use rudof_rdf::parser::rdf_node_parser::{ParserExt, RDFNodeParse};
use rudof_rdf::vocab::ShaclVocab;

pub(crate) fn in_component<RDF: NeighsRDF>() -> impl RDFNodeParse<RDF, Output = Vec<ASTComponent>> {
    parse_components_for_iri(
        ShaclVocab::sh_in(),
        ListParser::new().flat_map(|ls| {
            let values = ls
                .iter()
                .flat_map(|t| term_to_value::<RDF>(t, "parsing in list"))
                .collect();
            Ok(ASTComponent::In(values))
        }),
    )
}
