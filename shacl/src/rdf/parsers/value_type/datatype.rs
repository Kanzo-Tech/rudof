use crate::ast::ASTComponent;
use prefixmap::IriRef;
use rudof_rdf::NeighsRDF;
use rudof_rdf::parser::rdf_node_parser::constructors::{ListParser, TermParser};
use rudof_rdf::parser::rdf_node_parser::utils::term_to_iri;
use rudof_rdf::parser::rdf_node_parser::{ParserExt, RDFNodeParse};
use rudof_rdf::vocab::ShaclVocab;

/// `sh:datatype`. Its value is an IRI or, in SHACL 1.2 Core (§7.1.2), a SHACL list
/// of IRIs; either way it is one constraint over a set of datatypes.
pub(crate) fn datatype<RDF: NeighsRDF>() -> impl RDFNodeParse<RDF, Output = Vec<ASTComponent>> {
    let one = TermParser::new().flat_map(|t: RDF::Term| term_to_iri::<RDF>(&t).map(|iri| vec![iri]));
    let list = ListParser::new()
        .flat_map(|terms: Vec<RDF::Term>| terms.iter().map(term_to_iri::<RDF>).collect::<Result<Vec<_>, _>>());
    one.or(list)
        .map(|iris| ASTComponent::Datatype(iris.into_iter().map(IriRef::iri).collect()))
        .map_property(ShaclVocab::sh_datatype())
}
