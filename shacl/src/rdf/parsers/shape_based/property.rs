use rudof_rdf::NeighsRDF;
use rudof_rdf::parser::rdf_node_parser::RDFNodeParse;
use rudof_rdf::parser::rdf_node_parser::constructors::ObjectsPropertyParser;
use rudof_rdf::term::Object;
use rudof_rdf::vocab::ShaclVocab;

pub(crate) fn property<RDF: NeighsRDF>() -> impl RDFNodeParse<RDF, Output = Vec<Object>> {
    ObjectsPropertyParser::new(ShaclVocab::sh_property())
}
