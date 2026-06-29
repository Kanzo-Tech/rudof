use rudof_rdf::rdf_core::parser::rdf_node_parser::constructors::{ShaclPathParser, SingleValuePropertyParser};
use rudof_rdf::rdf_core::parser::rdf_node_parser::{ParserExt, RDFNodeParse};
use rudof_rdf::rdf_core::vocabs::ShaclVocab;
use rudof_rdf::rdf_core::{NeighsRDF, SHACLPath};

pub(crate) fn path<RDF: NeighsRDF>() -> impl RDFNodeParse<RDF, Output = SHACLPath> {
    SingleValuePropertyParser::new(ShaclVocab::sh_path()).then(ShaclPathParser::new)
}
