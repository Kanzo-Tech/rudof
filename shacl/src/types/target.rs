use crate::ir::error::IRError;
use rudof_iri::IriS;
use rudof_rdf::rdf_core::BuildRDF;
use rudof_rdf::rdf_core::term::Object;
use rudof_rdf::rdf_core::vocabs::{RdfVocab, RdfsVocab, ShaclVocab};
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};

/// Represents target declarations
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Target {
    Node(Object), // TODO - Replace with node expr
    Class(Object),
    SubjectsOf(IriS),
    ObjectsOf(IriS),
    ImplicitClass(Object),

    // The following target declaration are not well-formed, but we keep them
    // to generate violation errors for them
    WrongNode(Object),
    WrongClass(Object),
    WrongSubjectsOf(Object),
    WrongObjectsOf(Object),
    WrongImplicitClass(Object),
}

impl Target {
    pub fn register<RDF: BuildRDF>(&self, id: &Object, graph: &mut RDF) -> Result<(), IRError> {
        let node: RDF::Subject = id
            .clone()
            .try_into()
            .map_err(|_| IRError::InvalidShapeId(Box::new(id.clone())))?;

        // The malformed `Wrong*` targets serialize like their well-formed
        // counterparts (they exist only to round-trip and to raise violations).
        let result = match self {
            Target::Node(n) | Target::WrongNode(n) => graph.add_triple(node, ShaclVocab::sh_target_node(), n.clone()),
            Target::Class(c) | Target::WrongClass(c) => {
                graph.add_triple(node, ShaclVocab::sh_target_class(), c.clone())
            },
            Target::SubjectsOf(s) => graph.add_triple(node, ShaclVocab::sh_target_subjects_of(), s.clone()),
            Target::WrongSubjectsOf(s) => graph.add_triple(node, ShaclVocab::sh_target_subjects_of(), s.clone()),
            Target::ObjectsOf(o) => graph.add_triple(node, ShaclVocab::sh_target_objects_of(), o.clone()),
            Target::WrongObjectsOf(o) => graph.add_triple(node, ShaclVocab::sh_target_objects_of(), o.clone()),
            // TODO - In SHACL 1.2, add sh_shape_class ?
            Target::ImplicitClass(_) | Target::WrongImplicitClass(_) => {
                graph.add_triple(node, RdfVocab::rdf_type().clone(), RdfsVocab::rdfs_class())
            },
        };

        result.map_err(|e| IRError::from_rdf_err::<RDF>("add target", e))
    }
}

impl Display for Target {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Target::Node(o) => write!(f, "targetNode({o})"),
            Target::Class(o) => write!(f, "targetClass({o})"),
            Target::SubjectsOf(iri) => write!(f, "targetSubjectsOf({iri})"),
            Target::ObjectsOf(iri) => write!(f, "targetObjectsOf({iri})"),
            Target::ImplicitClass(o) => write!(f, "targetImplicitClass({o})"),
            Target::WrongNode(o) => write!(f, "targetNode({o})"),
            Target::WrongClass(o) => write!(f, "targetClass({o})"),
            Target::WrongSubjectsOf(iri) => write!(f, "targetSubjectsOf({iri})"),
            Target::WrongObjectsOf(iri) => write!(f, "targetObjectsOf({iri})"),
            Target::WrongImplicitClass(o) => write!(f, "targetImplicitClass({o})"),
        }
    }
}
