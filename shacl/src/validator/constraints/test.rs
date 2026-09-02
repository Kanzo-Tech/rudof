#[cfg(all(not(target_family = "wasm"), test))]
mod tests {
    use crate::ir::IRSchema;
    use crate::rdf::ShaclParser;
    use crate::types::MessageMap;
    use crate::validator::ShaclValidationMode;
    use crate::validator::processor::{DataValidation, ShaclProcessor};
    use rudof_rdf::RDFFormat;
    use rudof_rdf::backend::ReaderMode;
    use rudof_rdf::term::literal::Lang;
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

    /// `sh:deactivated true` on a *property* shape (SHACL §2.1.6): every RDF term
    /// conforms to it, so it must raise nothing — not even the `sh:minCount` the
    /// same shape declares. Locks in the `ASTPropertyShape::is_deactivated` fix:
    /// it used to read a `deactivated: bool` field the RDF parser never assigned,
    /// so it always answered `false` and the switch was silently ignored.
    #[test]
    fn deactivated_property_shape_reports_nothing() {
        const SHAPE: &str = r#"
prefix sh: <http://www.w3.org/ns/shacl#>
prefix : <http://example.org/>

:S a sh:NodeShape ; sh:targetClass :C ;
  sh:property [ sh:path :p ; sh:minCount 1 SWITCH ] .
:n a :C .
"#;
        // The control: live, the very same shape reports its missing :p.
        assert_eq!(results_len(&SHAPE.replace("SWITCH", "")), 1);
        assert_eq!(results_len(&SHAPE.replace("SWITCH", "; sh:deactivated true")), 0);
    }

    // -- sh:message reaches the report, whatever the component ---------------
    //
    // `sh:message` is declared on the shape, so *every* component that emits a
    // result owes the author their text. The components that build their
    // results by hand used to drop it on the floor; these lock that shut.

    const PREFIXES: &str = r#"
prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#>
prefix sh: <http://www.w3.org/ns/shacl#>
prefix xsd: <http://www.w3.org/2001/XMLSchema#>
prefix : <http://example.org/>
"#;

    /// Validate `graph` (native engine) and return the results' messages.
    fn messages(graph: &str) -> Vec<MessageMap> {
        let graph = format!("{PREFIXES}{graph}");
        let rdf = RdfData::from_str(&graph, &RDFFormat::Turtle, None, &ReaderMode::Strict).unwrap();
        let mut validator: DataValidation = rdf.clone().into();
        let schema = ShaclParser::new(rdf).parse().unwrap();
        let schema_ir: IRSchema = schema.try_into().unwrap();
        let report = validator.validate(&schema_ir, &ShaclValidationMode::Native).unwrap();
        report.results().iter().map(|r| r.message().clone()).collect()
    }

    /// Every result carries the author's `sh:message`, language tag intact.
    fn assert_author_message(graph: &str) {
        let msgs = messages(graph);
        assert!(!msgs.is_empty(), "expected at least one violation, got none");
        let es = Lang::new("es").unwrap();
        for m in &msgs {
            assert_eq!(
                m.get(Some(&es)).map(String::as_str),
                Some("mensaje del autor"),
                "author sh:message missing from result message {m}"
            );
        }
    }

    #[test]
    fn node_keeps_author_message() {
        assert_author_message(
            r#"
:S a sh:NodeShape ; sh:targetClass :C ;
  sh:property [ sh:path :p ; sh:node :Inner ; sh:message "mensaje del autor"@es ] .
:Inner a sh:NodeShape ; sh:property [ sh:path :q ; sh:minCount 1 ] .
:n a :C ; :p :x .
"#,
        );
    }

    #[test]
    fn not_keeps_author_message() {
        assert_author_message(
            r#"
:S a sh:NodeShape ; sh:targetClass :C ;
  sh:property [ sh:path :p ; sh:not [ sh:datatype xsd:string ] ; sh:message "mensaje del autor"@es ] .
:n a :C ; :p "s" .
"#,
        );
    }

    #[test]
    fn xone_keeps_author_message() {
        assert_author_message(
            r#"
:S a sh:NodeShape ; sh:targetClass :C ;
  sh:property [ sh:path :p ;
    sh:xone ( [ sh:datatype xsd:string ] [ sh:minLength 1 ] ) ;
    sh:message "mensaje del autor"@es ] .
:n a :C ; :p "s" .
"#,
        );
    }

    #[test]
    fn or_keeps_author_message() {
        assert_author_message(
            r#"
:S a sh:NodeShape ; sh:targetClass :C ;
  sh:property [ sh:path :p ;
    sh:or ( [ sh:datatype xsd:integer ] [ sh:datatype xsd:boolean ] ) ;
    sh:message "mensaje del autor"@es ] .
:n a :C ; :p "s" .
"#,
        );
    }

    #[test]
    fn and_keeps_author_message() {
        assert_author_message(
            r#"
:S a sh:NodeShape ; sh:targetClass :C ;
  sh:property [ sh:path :p ;
    sh:and ( [ sh:datatype xsd:integer ] [ sh:minInclusive 10 ] ) ;
    sh:message "mensaje del autor"@es ] .
:n a :C ; :p 5 .
"#,
        );
    }

    #[test]
    fn if_keeps_author_message() {
        assert_author_message(
            r#"
:S a sh:NodeShape ; sh:targetClass :C ;
  sh:message "mensaje del autor"@es ;
  sh:if [ sh:path :flag ; sh:hasValue :yes ] ;
  sh:then [ sh:property [ sh:path :thenProp ; sh:minCount 1 ] ] .
:n a :C ; :flag :yes .
"#,
        );
    }

    #[test]
    fn qualified_value_shape_keeps_author_message() {
        assert_author_message(
            r#"
:S a sh:NodeShape ; sh:targetClass :C ;
  sh:property [ sh:path :p ;
    sh:qualifiedValueShape [ sh:datatype xsd:integer ] ;
    sh:qualifiedMinCount 1 ;
    sh:message "mensaje del autor"@es ] .
:n a :C ; :p "s" .
"#,
        );
    }

    #[test]
    fn unique_lang_keeps_author_message() {
        assert_author_message(
            r#"
:S a sh:NodeShape ; sh:targetClass :C ;
  sh:property [ sh:path :p ; sh:uniqueLang true ; sh:message "mensaje del autor"@es ] .
:n a :C ; :p "uno"@es, "dos"@es .
"#,
        );
    }

    #[test]
    fn less_than_keeps_author_message() {
        assert_author_message(
            r#"
:S a sh:NodeShape ; sh:targetClass :C ;
  sh:property [ sh:path :p ; sh:lessThan :q ; sh:message "mensaje del autor"@es ] .
:n a :C ; :p 5 ; :q 1 .
"#,
        );
    }

    #[test]
    fn less_than_or_equals_keeps_author_message() {
        assert_author_message(
            r#"
:S a sh:NodeShape ; sh:targetClass :C ;
  sh:property [ sh:path :p ; sh:lessThanOrEquals :q ; sh:message "mensaje del autor"@es ] .
:n a :C ; :p 5 ; :q 1 .
"#,
        );
    }

    #[test]
    fn disjoint_keeps_author_message() {
        assert_author_message(
            r#"
:S a sh:NodeShape ; sh:targetClass :C ;
  sh:property [ sh:path :p ; sh:disjoint :q ; sh:message "mensaje del autor"@es ] .
:n a :C ; :p "same" ; :q "same" .
"#,
        );
    }

    #[test]
    fn equals_keeps_author_message() {
        assert_author_message(
            r#"
:S a sh:NodeShape ; sh:targetClass :C ;
  sh:property [ sh:path :p ; sh:equals :q ; sh:message "mensaje del autor"@es ] .
:n a :C ; :p "uno" ; :q "dos" .
"#,
        );
    }

    #[test]
    fn closed_keeps_author_message() {
        assert_author_message(
            r#"
:S a sh:NodeShape ; sh:targetClass :C ;
  sh:closed true ;
  sh:ignoredProperties ( rdf:type ) ;
  sh:message "mensaje del autor"@es ;
  sh:property [ sh:path :p ] .
:n a :C ; :p "x" ; :other "y" .
"#,
        );
    }

    /// SHACL §2.1.5 — "If a shape has at least one value for `sh:message` in the
    /// shapes graph, then all validation results produced as a result of the
    /// shape will have **exactly these messages** as their value of
    /// `sh:resultMessage`". The engine's own wording ("MinCount(1) not
    /// satisfied") is therefore suppressed, not merged in under the untagged key
    /// beside the author's three languages.
    #[test]
    fn shape_message_replaces_the_generated_one() {
        let msgs = messages(
            r#"
:S a sh:NodeShape ; sh:targetClass :C ;
  sh:property [ sh:path :p ; sh:minCount 1 ;
    sh:message "mensaje del autor"@es , "author message"@en , "missatge de l'autor"@ca ] .
:n a :C .
"#,
        );
        let m = msgs.first().expect("expected one violation, got none");
        assert_eq!(
            m.messages().len(),
            3,
            "expected exactly the shape's three messages, got {m}"
        );
        for tag in ["es", "en", "ca"] {
            assert!(
                m.get(Some(&Lang::new(tag).unwrap())).is_some(),
                "the shape's {tag} message is missing from {m}"
            );
        }
        assert_eq!(
            m.get(None),
            None,
            "the engine's generated message leaked in beside the shape's: {m}"
        );
    }

    /// The mirror case: a shape declaring no `sh:message` leaves the engine free
    /// to generate one (§3.6.2.7) — and `sh:node` reports the shape's *id*, never
    /// a `Display` dump of the IR.
    #[test]
    fn generated_message_stands_when_the_shape_is_silent() {
        let msgs = messages(
            r#"
:S a sh:NodeShape ; sh:targetClass :C ;
  sh:property [ sh:path :p ; sh:node :Inner ] .
:Inner a sh:NodeShape ; sh:property [ sh:path :q ; sh:minCount 1 ] .
:n a :C ; :p :x .
"#,
        );
        let m = msgs.first().unwrap();
        let default = m.get(None).expect("the engine's own wording, under the default key");
        assert!(
            default.contains("http://example.org/Inner"),
            "expected the node shape id, got {default}"
        );
        assert!(
            !default.contains("Property Shapes"),
            "IR dump leaked into the message: {default}"
        );
    }

    // ---- sh:class is transitive (SHACL §4.4.1 + §1.1) -----------------------
    //
    // "Each value node is a SHACL instance of $class" (§4.4.1), and the SHACL
    // types of a term are its `rdf:type` values *plus the SHACL superclasses of
    // those values* (§1.1) — the transitive `rdfs:subClassOf` closure. The
    // native check used to walk exactly one hop, so a value typed two classes
    // below the constrained one was reported as a violation.

    /// A `sh:class` shapes/data graph: `:node :role :value`, checked against
    /// `sh:class skos:Concept`, over whatever class hierarchy `hierarchy` states.
    fn class_graph(hierarchy: &str, typing: &str) -> String {
        format!(
            r#"
prefix sh: <http://www.w3.org/ns/shacl#>
prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#>
prefix skos: <http://www.w3.org/2004/02/skos/core#>
prefix : <http://example.org/>

:S a sh:NodeShape ;
  sh:targetNode :node ;
  sh:property [ sh:path :role ; sh:class skos:Concept ] .

:node :role :value .
{hierarchy}
{typing}
"#
        )
    }

    #[test]
    fn sh_class_holds_for_a_directly_typed_value() {
        assert_eq!(results_len(&class_graph("", ":value a skos:Concept .")), 0);
    }

    #[test]
    fn sh_class_holds_one_subclass_hop_away() {
        assert_eq!(
            results_len(&class_graph(":Role rdfs:subClassOf skos:Concept .", ":value a :Role .")),
            0,
            "a value typed with a subclass of the constrained class is a SHACL instance of it"
        );
    }

    #[test]
    fn sh_class_holds_two_subclass_hops_away() {
        assert_eq!(
            results_len(&class_graph(
                ":Role rdfs:subClassOf skos:Concept .\n:Officer rdfs:subClassOf :Role .",
                ":value a :Officer .",
            )),
            0,
            "the subclass closure is transitive, not one hop"
        );
    }

    #[test]
    fn sh_class_terminates_on_a_cyclic_class_hierarchy() {
        assert_eq!(
            results_len(&class_graph(
                ":Role rdfs:subClassOf skos:Concept .\n:Officer rdfs:subClassOf :Role .\n:Role rdfs:subClassOf :Officer .",
                ":value a :Officer .",
            )),
            0,
            "a class cycle must not spin the closure walk"
        );
    }

    #[test]
    fn sh_class_violates_for_a_class_outside_the_hierarchy() {
        assert_eq!(
            results_len(&class_graph(
                ":Role rdfs:subClassOf skos:Concept .",
                ":value a :Unrelated ."
            )),
            1,
            "transitivity must not make sh:class vacuous"
        );
    }

    #[test]
    fn sh_class_does_not_walk_the_hierarchy_upwards() {
        // `:Role rdfs:subClassOf skos:Concept` makes every `:Role` a
        // `skos:Concept`, never the reverse: a value typed only `skos:Concept`
        // is not a SHACL instance of `:Role`.
        const GRAPH: &str = r#"
prefix sh: <http://www.w3.org/ns/shacl#>
prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#>
prefix skos: <http://www.w3.org/2004/02/skos/core#>
prefix : <http://example.org/>

:S a sh:NodeShape ;
  sh:targetNode :node ;
  sh:property [ sh:path :role ; sh:class :Role ] .

:node :role :value .
:Role rdfs:subClassOf skos:Concept .
:value a skos:Concept .
"#;
        assert_eq!(results_len(GRAPH), 1, "subclass closure runs downwards only");
    }
}
