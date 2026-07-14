#[cfg(all(not(target_family = "wasm"), test))]
mod tests {
    use crate::ir::IRSchema;
    use crate::rdf::ShaclParser;
    use crate::validator::ShaclValidationMode;
    use crate::validator::processor::{DataValidation, ShaclProcessor};
    use rudof_rdf::RDFFormat;
    use rudof_rdf::backend::ReaderMode;
    use sparql_service::RdfData;

    #[test]
    fn test_min_exclusive_native() {
        let graph = r#"
prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#>
prefix sh: <http://www.w3.org/ns/shacl#>
prefix : <http://example.org/>
prefix xsd: <http://www.w3.org/2001/XMLSchema#>

:MinInclusive a sh:NodeShape ;
  sh:targetClass :Node ;
  sh:property [
    sh:path :p ;
    sh:datatype xsd:double ;
    sh:minInclusive "0.0"^^xsd:double ;
    sh:minCount 1
 ] .

:ok1 a :Node; :p "0"^^xsd:double .
:ok2 a :Node; :p "10.5"^^xsd:double .
:ko1 a :Node; :p "-5.3"^^xsd:double .
:ko2 a :Node; :p "other" .
:ko3 a :Node; :p "other"^^xsd:double .
"#;

        let rdf = RdfData::from_str(graph, &RDFFormat::Turtle, None, &ReaderMode::Strict).unwrap();
        let mut validator: DataValidation = rdf.clone().into();
        let schema = ShaclParser::new(rdf).parse().unwrap();
        let schema_ir: IRSchema = schema.try_into().unwrap();
        let report = validator.validate(&schema_ir, &ShaclValidationMode::Native).unwrap();
        assert_eq!(report.results().len(), 5);
    }

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

    // sh:if / sh:then / sh:else at the node-shape level. The condition is a
    // blank-node shape (as in real usage): `sh:if [ sh:path :p ; sh:hasValue :x ]`.
    const IF_SHAPES: &str = r#"
prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#>
prefix sh: <http://www.w3.org/ns/shacl#>
prefix : <http://example.org/>

:S a sh:NodeShape ;
  sh:targetClass :C ;
  sh:if [ sh:path :flag ; sh:hasValue :yes ] ;
  sh:then [ sh:property [ sh:path :thenProp ; sh:minCount 1 ] ] ;
  sh:else [ sh:property [ sh:path :elseProp ; sh:minCount 1 ] ] .
"#;

    #[test]
    fn if_cond_true_then_ok_conforms() {
        // (a) conforms to cond AND satisfies then -> conforms (0 results).
        let graph = format!("{IF_SHAPES}\n:n a :C ; :flag :yes ; :thenProp \"x\" .");
        assert_eq!(results_len(&graph), 0);
    }

    #[test]
    fn if_cond_true_then_violated() {
        // (b) conforms to cond but violates then -> one violation.
        let graph = format!("{IF_SHAPES}\n:n a :C ; :flag :yes .");
        assert_eq!(results_len(&graph), 1);
    }

    #[test]
    fn if_cond_false_else_ok_conforms() {
        // (c) does NOT conform to cond AND satisfies else -> conforms.
        let graph = format!("{IF_SHAPES}\n:n a :C ; :flag :no ; :elseProp \"y\" .");
        assert_eq!(results_len(&graph), 0);
    }

    #[test]
    fn if_cond_false_else_violated() {
        // (d) does not conform to cond and violates else -> violation.
        let graph = format!("{IF_SHAPES}\n:n a :C ; :flag :no .");
        assert_eq!(results_len(&graph), 1);
    }

    #[test]
    fn if_no_else_cond_false_conforms() {
        // (e) no sh:else and cond false -> conforms (missing branch = no constraint).
        let shapes = r#"
prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#>
prefix sh: <http://www.w3.org/ns/shacl#>
prefix : <http://example.org/>

:S a sh:NodeShape ;
  sh:targetClass :C ;
  sh:if [ sh:path :flag ; sh:hasValue :yes ] ;
  sh:then [ sh:property [ sh:path :thenProp ; sh:minCount 1 ] ] .

:n a :C ; :flag :no .
"#;
        assert_eq!(results_len(shapes), 0);
    }
}
