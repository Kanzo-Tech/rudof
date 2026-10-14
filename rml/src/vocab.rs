//! The RML vocabulary: `http://w3id.org/rml/` (RML-Core §3.2), and the few
//! IRIs of other vocabularies the mapping reads or writes.

use rudof_iri::IriS;

/// The RML namespace IRI (RML-Core §3.2: "uses namespace IRI
/// `http://w3id.org/rml/`").
pub const RML: &str = "http://w3id.org/rml/";
pub const RDF: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";
pub const XSD: &str = "http://www.w3.org/2001/XMLSchema#";

/// `rml:{local}`.
pub(crate) fn rml(local: &str) -> IriS {
    IriS::new_unchecked(&format!("{RML}{local}"))
}

/// `rdf:{local}`.
pub(crate) fn rdf(local: &str) -> IriS {
    IriS::new_unchecked(&format!("{RDF}{local}"))
}

/// `xsd:{local}`.
pub(crate) fn xsd(local: &str) -> IriS {
    IriS::new_unchecked(&format!("{XSD}{local}"))
}

/// Whether `iri` is in the RML namespace.
pub(crate) fn is_rml(iri: &IriS) -> bool {
    iri.as_str().starts_with(RML)
}

/// The local names the AST reads and writes; one place, so the parser and the
/// writer cannot disagree on a spelling.
pub(crate) mod term {
    pub const TRIPLES_MAP: &str = "TriplesMap";
    pub const LOGICAL_SOURCE: &str = "LogicalSource";
    pub const SOURCE: &str = "Source";
    pub const SQL2008_TABLE: &str = "SQL2008Table";
    pub const SQL2008_QUERY: &str = "SQL2008Query";

    pub const P_BASE_IRI: &str = "baseIRI";
    pub const P_LOGICAL_SOURCE: &str = "logicalSource";
    pub const P_SOURCE: &str = "source";
    pub const P_REFERENCE_FORMULATION: &str = "referenceFormulation";
    pub const P_ITERATOR: &str = "iterator";
    pub const P_SUBJECT_MAP: &str = "subjectMap";
    pub const P_SUBJECT: &str = "subject";
    pub const P_PREDICATE_OBJECT_MAP: &str = "predicateObjectMap";
    pub const P_PREDICATE_MAP: &str = "predicateMap";
    pub const P_PREDICATE: &str = "predicate";
    pub const P_OBJECT_MAP: &str = "objectMap";
    pub const P_OBJECT: &str = "object";
    pub const P_CONSTANT: &str = "constant";
    pub const P_REFERENCE: &str = "reference";
    pub const P_TEMPLATE: &str = "template";
    pub const P_TERM_TYPE: &str = "termType";
    pub const P_CLASS: &str = "class";
    pub const P_LANGUAGE_MAP: &str = "languageMap";
    pub const P_LANGUAGE: &str = "language";
    pub const P_DATATYPE_MAP: &str = "datatypeMap";
    pub const P_DATATYPE: &str = "datatype";
    pub const P_PARENT_TRIPLES_MAP: &str = "parentTriplesMap";
    pub const P_JOIN_CONDITION: &str = "joinCondition";
    pub const P_CHILD_MAP: &str = "childMap";
    pub const P_PARENT_MAP: &str = "parentMap";
    pub const P_CHILD: &str = "child";
    pub const P_PARENT: &str = "parent";

    pub const IRI: &str = "IRI";
    pub const UNSAFE_IRI: &str = "UnsafeIRI";
    pub const BLANK_NODE: &str = "BlankNode";
    pub const LITERAL: &str = "Literal";
}
