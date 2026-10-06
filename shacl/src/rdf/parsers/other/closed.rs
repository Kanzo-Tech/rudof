use crate::ast::ASTComponent;
use rudof_iri::IriS;
use rudof_rdf::parser::rdf_node_parser::constructors::{SingleValuePropertyAsListParser, SingleValuePropertyParser};
use rudof_rdf::parser::rdf_node_parser::utils::term_to_bool;
use rudof_rdf::parser::rdf_node_parser::{ParserExt, RDFNodeParse};
use rudof_rdf::term::Iri;
use rudof_rdf::vocab::ShaclVocab;
use rudof_rdf::{NeighsRDF, RDFError};
use std::collections::HashSet;

/// `sh:closed`: a boolean, or `sh:ByTypes` (SHACL 1.2 §8.4.1). The properties
/// `sh:ByTypes` permits depend on the whole shapes graph, so they are filled
/// in once every shape is parsed ([`crate::rdf::ShaclParser`]).
pub(crate) fn closed<RDF: NeighsRDF>() -> impl RDFNodeParse<RDF, Output = Vec<ASTComponent>> {
    SingleValuePropertyParser::new(ShaclVocab::sh_closed())
        .optional()
        .then(move |closed: Option<RDF::Term>| {
            ignored_properties().flat_map(move |ignored_properties| {
                let Some(term) = &closed else {
                    return Ok(Vec::new());
                };
                let by_types =
                    RDF::term_as_iri(term).is_ok_and(|iri: RDF::IRI| iri.as_str() == ShaclVocab::SH_BY_TYPES);
                let is_closed = by_types || term_to_bool::<RDF>(term)?;
                Ok(vec![ASTComponent::Closed {
                    is_closed,
                    ignored_properties,
                    by_types: by_types.then(Vec::new),
                }])
            })
        })
}

fn ignored_properties<RDF: NeighsRDF>() -> impl RDFNodeParse<RDF, Output = HashSet<IriS>> {
    SingleValuePropertyAsListParser::new(ShaclVocab::sh_ignored_properties())
        .optional()
        .flat_map(|is| match is {
            None => Ok(HashSet::new()),
            Some(vs) => {
                let mut hs = HashSet::new();
                for v in vs {
                    if let Ok(iri) = RDF::term_as_iri(&v) {
                        let iri: RDF::IRI = iri;
                        hs.insert(IriS::new_unchecked(iri.as_str()));
                    } else {
                        return Err(RDFError::ExpectedIRIError { term: v.to_string() });
                    }
                }
                Ok(hs)
            },
        })
}
