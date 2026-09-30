use crate::types::Target;
use rudof_rdf::parser::rdf_node_parser::constructors::ObjectsPropertyParser;
use rudof_rdf::parser::rdf_node_parser::{ParserExt, RDFNodeParse};
use rudof_rdf::term::Object;
use rudof_rdf::vocab::ShaclVocab;
use rudof_rdf::{NeighsRDF, RDFError};

/// `sh:targetWhere` (SHACL 1.2 Core §3.1.3.6): each value is a shape, so an IRI or a blank node.
pub(crate) fn targets_where<RDF: NeighsRDF>() -> impl RDFNodeParse<RDF, Output = Vec<Target>> {
    ObjectsPropertyParser::new(ShaclVocab::sh_target_where()).flat_map(|ts| {
        ts.into_iter()
            .map(|t| match t {
                Object::Literal(l) => Err(RDFError::ExpectedIriOrBlankNodeFoundLiteral { literal: l.to_string() }),
                shape => Ok(Target::Where(shape)),
            })
            .collect()
    })
}
