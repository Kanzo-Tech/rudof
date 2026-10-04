//! Target selection: which nodes a shape actually reaches.

#[cfg(all(not(target_family = "wasm"), test))]
mod tests {
    use crate::ir::IRSchema;
    use crate::rdf::ShaclParser;
    use crate::validator::ShaclValidationMode;
    use crate::validator::processor::{DataValidation, ShaclProcessor};
    use rudof_rdf::RDFFormat;
    use rudof_rdf::backend::ReaderMode;
    use sparql_service::RdfData;

    /// Validate `graph` against its own shapes (native engine) and return the
    /// number of validation results.
    fn results_len(graph: &str) -> usize {
        let rdf = RdfData::from_str(graph, &RDFFormat::Turtle, None, &ReaderMode::Strict).unwrap();
        let mut validator: DataValidation = rdf.clone().into();
        let schema = ShaclParser::new(rdf).parse().unwrap();
        let schema_ir: IRSchema = schema.try_into().unwrap();
        let report = validator.validate(&schema_ir, &ShaclValidationMode::Native).unwrap();
        report.results().len()
    }

    /// SHACL §1.1: "The SHACL types of an RDF term ... is the set of its values
    /// for `rdf:type` ... as well as the SHACL superclasses of these values",
    /// and §2.1.3.2 makes the SHACL instances of the class the targets of
    /// `sh:targetClass`. So a node typed `dcat:Role`, in a graph that also
    /// states `dcat:Role rdfs:subClassOf skos:Concept`, is a target of a shape
    /// with `sh:targetClass skos:Concept` — at one hop and at any number of
    /// hops. The native engine used to select only the directly typed nodes, so
    /// every node reached through the class hierarchy went unvalidated.
    #[test]
    fn target_class_follows_the_subclass_chain() {
        const GRAPH: &str = r#"
prefix sh: <http://www.w3.org/ns/shacl#>
prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#>
prefix skos: <http://www.w3.org/2004/02/skos/core#>
prefix dcat: <http://www.w3.org/ns/dcat#>
prefix : <http://example.org/>

:ConceptShape a sh:NodeShape ;
  sh:targetClass skos:Concept ;
  sh:property [ sh:path skos:prefLabel ; sh:minCount 1 ] .

dcat:Role rdfs:subClassOf skos:Concept .
:Officer rdfs:subClassOf dcat:Role .

:direct a skos:Concept .
:one_hop a dcat:Role .
:two_hops a :Officer .
"#;

        // Each of the three nodes is missing skos:prefLabel; before the fix only
        // `:direct` was even looked at.
        assert_eq!(
            results_len(GRAPH),
            3,
            "sh:targetClass must select the SHACL instances of the class, subclasses included"
        );
    }

    /// The implicit class target (§2.1.3.3) selects the same set, so it too has
    /// to close over the whole chain. It used to walk exactly one `rdfs:subClassOf`
    /// hop, which left the grandchild class unvalidated.
    #[test]
    fn implicit_class_target_follows_the_whole_subclass_chain() {
        const GRAPH: &str = r#"
prefix sh: <http://www.w3.org/ns/shacl#>
prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#>
prefix : <http://example.org/>

:Top a sh:NodeShape, rdfs:Class ;
  sh:property [ sh:path :p ; sh:minCount 1 ] .

:Middle rdfs:subClassOf :Top .
:Bottom rdfs:subClassOf :Middle .

:near a :Middle .
:far a :Bottom .
"#;

        assert_eq!(
            results_len(GRAPH),
            2,
            "an implicit class target must reach instances of transitive subclasses"
        );
    }

    /// A cyclic class hierarchy is nonsense but expressible; the closure walk
    /// must terminate on it rather than spin.
    #[test]
    fn a_cyclic_class_hierarchy_terminates() {
        const GRAPH: &str = r#"
prefix sh: <http://www.w3.org/ns/shacl#>
prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#>
prefix : <http://example.org/>

:S a sh:NodeShape ;
  sh:targetClass :A ;
  sh:property [ sh:path :p ; sh:minCount 1 ] .

:A rdfs:subClassOf :B .
:B rdfs:subClassOf :A .

:x a :B .
"#;

        assert_eq!(results_len(GRAPH), 1, "a class cycle must not prevent target selection");
    }
}
