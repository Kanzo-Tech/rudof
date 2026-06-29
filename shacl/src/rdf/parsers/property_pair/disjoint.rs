use crate::ast::ASTComponent;
use prefixmap::IriRef;
use rudof_rdf::NeighsRDF;
use rudof_rdf::parser::rdf_node_parser::constructors::IrisPropertyParser;
use rudof_rdf::parser::rdf_node_parser::{ParserExt, RDFNodeParse};
use rudof_rdf::vocab::ShaclVocab;

pub(crate) fn disjoint<RDF: NeighsRDF>() -> impl RDFNodeParse<RDF, Output = Vec<ASTComponent>> {
    IrisPropertyParser::new(ShaclVocab::sh_disjoint()).map(|ns| {
        ns.into_iter()
            .map(|n| {
                let iri = IriRef::iri(n);
                ASTComponent::Disjoint(iri)
            })
            .collect()
    })
}
