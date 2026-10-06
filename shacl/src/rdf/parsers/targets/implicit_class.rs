use crate::types::Target;
use rudof_rdf::parser::rdf_node_parser::constructors::{FocusParser, InstancesParser};
use rudof_rdf::parser::rdf_node_parser::{ParserExt, RDFNodeParse};
use rudof_rdf::vocab::{RdfsVocab, ShaclVocab};
use rudof_rdf::{NeighsRDF, RDFError};

/// The implicit class target (SHACL 1.2 §2.1.2.3): a shape that is also a
/// class targets its instances. `sh:ShapeClass` is a subclass of both
/// `sh:NodeShape` and `rdfs:Class`, so its instances are such shapes without
/// the shapes graph having to say so.
pub(crate) fn targets_implicit_class<RDF: NeighsRDF>() -> impl RDFNodeParse<RDF, Output = Vec<Target>> {
    InstancesParser::new(RdfsVocab::rdfs_class())
        .and(InstancesParser::new(ShaclVocab::sh_property_shape()))
        .and(InstancesParser::new(ShaclVocab::sh_node_shape()))
        .and(InstancesParser::new(ShaclVocab::sh_shape_class()))
        .and(FocusParser::new())
        .flat_map(
            move |((((classes, property_shapes), node_shapes), shape_classes), focus): (_, RDF::Term)| {
                let is = |set: &Vec<RDF::Subject>| set.iter().any(|s| RDF::subject_as_term(s) == focus);
                let shape_and_class =
                    is(&shape_classes) || (is(&classes) && (is(&property_shapes) || is(&node_shapes)));
                if !shape_and_class {
                    return Ok(Vec::new());
                }
                let name = focus.to_string();
                let object =
                    RDF::term_as_object(&focus).map_err(|_| RDFError::FailedTermToObjectError { term: name })?;
                Ok(vec![Target::ImplicitClass(object)])
            },
        )
}
