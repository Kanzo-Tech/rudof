use crate::types::Target;
use rudof_rdf::NeighsRDF;
use rudof_rdf::parser::rdf_node_parser::constructors::IrisPropertyParser;
use rudof_rdf::parser::rdf_node_parser::{ParserExt, RDFNodeParse};
use rudof_rdf::term::Object;
use rudof_rdf::vocab::ShaclVocab;

pub(crate) fn targets_class<RDF: NeighsRDF>() -> impl RDFNodeParse<RDF, Output = Vec<Target>> {
    IrisPropertyParser::new(ShaclVocab::sh_target_class()).flat_map(move |ts| {
        let result = ts.into_iter().map(|iri| Target::Class(Object::Iri(iri))).collect();
        Ok(result)
    })
}
