use crate::ast::ASTComponent;
use crate::rdf::parsers::utils::parse_components_for_iri;
use rudof_rdf::parser::rdf_node_parser::constructors::TermParser;
use rudof_rdf::parser::rdf_node_parser::{ParserExt, RDFNodeParse};
use rudof_rdf::vocab::ShaclVocab;
use rudof_rdf::{NeighsRDF, RDFError};

pub(crate) fn not<RDF: NeighsRDF>() -> impl RDFNodeParse<RDF, Output = Vec<ASTComponent>> {
    parse_components_for_iri(
        ShaclVocab::sh_not(),
        TermParser::new().flat_map(|t| {
            let shape =
                RDF::term_as_object(&t).map_err(|_| RDFError::FailedTermToRDFNodeError { term: t.to_string() })?;
            Ok(ASTComponent::Not(shape))
        }),
    )
}
