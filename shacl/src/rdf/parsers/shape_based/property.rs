use rudof_rdf::rdf_core::NeighsRDF;
use rudof_rdf::rdf_core::parser::rdf_node_parser::RDFNodeParse;
use rudof_rdf::rdf_core::parser::rdf_node_parser::constructors::ObjectsPropertyParser;
use rudof_rdf::rdf_core::term::Object;
use rudof_rdf::rdf_core::vocabs::ShaclVocab;

pub(crate) fn property<RDF: NeighsRDF>() -> impl RDFNodeParse<RDF, Output = Vec<Object>> {
    ObjectsPropertyParser::new(ShaclVocab::sh_property())
}
