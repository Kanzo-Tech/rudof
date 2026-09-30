#[cfg(all(not(target_family = "wasm"), test))]
mod tests {
    use crate::ir::IRSchema;
    use crate::messages::MessageCatalog;
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
        messages_with(graph, MessageCatalog::builtin().clone())
    }

    /// As [`messages`], under a given message catalog.
    fn messages_with(graph: &str, catalog: MessageCatalog) -> Vec<MessageMap> {
        let graph = format!("{PREFIXES}{graph}");
        let rdf = RdfData::from_str(&graph, &RDFFormat::Turtle, None, &ReaderMode::Strict).unwrap();
        let mut validator: DataValidation = rdf.clone().into();
        let schema = ShaclParser::new(rdf).parse().unwrap();
        let schema_ir: IRSchema = IRSchema::try_from(schema).unwrap().with_messages(catalog);
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
            "the engine's message leaked in beside the shape's: {m}"
        );
        assert_eq!(
            m.get(Some(&Lang::new("en").unwrap())).map(String::as_str),
            Some("author message")
        );
    }

    // -- default messages: generated from the catalog when the shape has none --
    //
    // SHACL §3.6.2.7: a processor "MAY automatically generate other values for
    // sh:resultMessage" where the constraint has no `sh:message`. The wording is
    // the catalog's (`messages/`), one message per language, placeholders filled.

    /// The message of the one result `graph` produces, in `lang`.
    fn one_message(graph: &str, lang: &str) -> String {
        let msgs = messages(graph);
        assert_eq!(msgs.len(), 1, "expected exactly one violation, got {msgs:?}");
        msgs[0]
            .get(Some(&Lang::new(lang).unwrap()))
            .unwrap_or_else(|| panic!("no @{lang} message in {}", msgs[0]))
            .clone()
    }

    /// A shape with one property constraint (`constraint`) and a focus node
    /// whose `:p` values are `values`.
    fn property_case(constraint: &str, values: &str) -> String {
        format!(
            "
:S a sh:NodeShape ; sh:targetClass :C ; sh:property [ sh:path :p ; {constraint} ] .
:n a :C ; {values} .
"
        )
    }

    #[test]
    fn silent_shape_gets_one_message_per_catalog_language() {
        let msgs = messages(&property_case("sh:minCount 2", ""));
        let m = &msgs[0];
        let mut langs: Vec<_> = m.iter().map(|(l, _)| l.as_ref().map(|l| l.as_str())).collect();
        langs.sort();
        assert_eq!(langs, [Some("ca"), Some("en"), Some("es")], "{m}");
        assert_eq!(m.get(None), None, "no untagged debug string: {m}");
    }

    #[test]
    fn parameters_fill_the_placeholders() {
        let en = |constraint: &str, values: &str| one_message(&property_case(constraint, values), "en");
        assert_eq!(en("sh:minCount 2", ":p 1"), "At least 2 value(s) required");
        assert_eq!(en("sh:maxCount 1", ":p 1, 2"), "At most 1 value(s) allowed");
        assert_eq!(
            en("sh:datatype xsd:integer", ":p \"x\""),
            "“x” is not of type xsd:integer"
        );
        assert_eq!(en("sh:class :K", ":p :v"), "Value must be an instance of :K");
        assert_eq!(en("sh:nodeKind sh:IRI", ":p \"x\""), "Expected kind of value: Iri");
        assert_eq!(
            en("sh:pattern \"^a\"", ":p \"b\""),
            "Value does not match the pattern ^a"
        );
        assert_eq!(en("sh:minLength 3", ":p \"b\""), "At least 3 character(s) required");
        assert_eq!(en("sh:maxLength 1", ":p \"bb\""), "At most 1 character(s) allowed");
        assert_eq!(en("sh:in (1 2)", ":p 3"), "“3” is not one of: 1, 2");
        assert_eq!(en("sh:hasValue :v", ":p :w"), "Required value missing: :v");
        assert_eq!(
            en("sh:languageIn (\"en\" \"es\")", ":p \"x\"@fr"),
            "Language must be one of: en, es"
        );
        assert_eq!(en("sh:minInclusive 5", ":p 1"), "Value must be at least 5");
        assert_eq!(en("sh:maxExclusive 5", ":p 9"), "Value must be less than 5");
        assert_eq!(
            en("sh:uniqueLang true", ":p \"a\"@en, \"b\"@en"),
            "Only one value per language is allowed"
        );
        assert_eq!(en("sh:equals :q", ":p 1"), "Values must be the same as those of :q");
        assert_eq!(
            en("sh:not [ sh:datatype xsd:string ]", ":p \"s\""),
            "This value is not allowed here"
        );
        assert_eq!(
            en(
                "sh:or ( [ sh:datatype xsd:integer ] [ sh:datatype xsd:boolean ] )",
                ":p \"s\""
            ),
            "Value must meet at least one of the alternatives"
        );
        assert_eq!(
            en(
                "sh:qualifiedValueShape [ sh:datatype xsd:integer ] ; sh:qualifiedMinCount 2",
                ":p 1"
            ),
            "At least 2 matching value(s) required"
        );
    }

    #[test]
    fn languages_share_the_parameters() {
        let graph = property_case("sh:minCount 2", "");
        assert_eq!(one_message(&graph, "es"), "Se requieren al menos 2 valor(es)");
        assert_eq!(one_message(&graph, "ca"), "Calen com a mínim 2 valor(s)");
    }

    #[test]
    fn loaded_messages_add_a_language_and_override_a_wording() {
        let mut catalog = MessageCatalog::builtin().clone();
        catalog
            .load(
                r#"@prefix sh: <http://www.w3.org/ns/shacl#> .
sh:MinCountConstraintComponent sh:message "Au moins {$minCount} valeur(s)"@fr , "Need {$minCount}!"@en ."#,
                &RDFFormat::Turtle,
            )
            .unwrap();
        let m = &messages_with(&property_case("sh:minCount 2", ""), catalog)[0];
        assert_eq!(m.get(Some(&Lang::new("fr").unwrap())).unwrap(), "Au moins 2 valeur(s)");
        assert_eq!(m.get(Some(&Lang::new("en").unwrap())).unwrap(), "Need 2!");
        // Untouched languages keep the built-in wording.
        assert_eq!(
            m.get(Some(&Lang::new("es").unwrap())).unwrap(),
            "Se requieren al menos 2 valor(es)"
        );
        assert_eq!(m.messages().len(), 4);
    }

    #[test]
    fn unnamed_component_takes_the_generic_message_or_none() {
        let mut catalog = MessageCatalog::default();
        catalog
            .load(
                r#"@prefix sh: <http://www.w3.org/ns/shacl#> . sh:ConstraintComponent sh:message "Nope"@en ."#,
                &RDFFormat::Turtle,
            )
            .unwrap();
        let graph = property_case("sh:minCount 2", "");
        assert_eq!(
            messages_with(&graph, catalog)[0]
                .get(Some(&Lang::new("en").unwrap()))
                .unwrap(),
            "Nope"
        );
        // No entry and no generic one: no message at all, never a debug string.
        assert!(
            messages_with(&graph, MessageCatalog::default())[0]
                .messages()
                .is_empty()
        );
    }

    #[test]
    fn node_shape_result_is_generated_too() {
        let m = one_message(
            r#"
:S a sh:NodeShape ; sh:targetClass :C ; sh:property [ sh:path :p ; sh:node :Inner ] .
:Inner a sh:NodeShape ; sh:property [ sh:path :q ; sh:minCount 1 ] .
:n a :C ; :p :x .
"#,
            "en",
        );
        assert_eq!(m, "Value does not meet the requirements of the referenced shape");
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
