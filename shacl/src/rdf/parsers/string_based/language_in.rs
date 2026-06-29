use crate::ast::ASTComponent;
use crate::rdf::parsers::utils::parse_components_for_iri;
use rudof_rdf::parser::rdf_node_parser::constructors::ListParser;
use rudof_rdf::parser::rdf_node_parser::{ParserExt, RDFNodeParse};
use rudof_rdf::term::literal::Lang;
use rudof_rdf::vocab::ShaclVocab;
use rudof_rdf::{NeighsRDF, RDFError};

pub(crate) fn language_in<RDF: NeighsRDF>() -> impl RDFNodeParse<RDF, Output = Vec<ASTComponent>> {
    parse_components_for_iri(
        ShaclVocab::sh_language_in(),
        ListParser::new().flat_map(cnv_language_in_list::<RDF>),
    )
}

fn cnv_language_in_list<RDF: NeighsRDF>(terms: Vec<RDF::Term>) -> Result<ASTComponent, RDFError> {
    let langs: Vec<Lang> = terms.iter().flat_map(RDF::term_as_lang).collect();
    Ok(ASTComponent::LanguageIn(langs))
}
