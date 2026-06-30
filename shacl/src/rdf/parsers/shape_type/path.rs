use rudof_rdf::parser::rdf_node_parser::constructors::{ShaclPathParser, SingleValuePropertyParser};
use rudof_rdf::parser::rdf_node_parser::{ParserExt, RDFNodeParse};
use rudof_rdf::vocab::ShaclVocab;
use rudof_rdf::{NeighsRDF, SHACLPath};

pub(crate) fn path<RDF: NeighsRDF>() -> impl RDFNodeParse<RDF, Output = SHACLPath> {
    SingleValuePropertyParser::new(ShaclVocab::sh_path()).then(ShaclPathParser::new)
}
