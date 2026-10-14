#[cfg(test)]
mod tests {
    use crate::ast::{ASTComponent, ASTNodeShape, ASTShape};
    use crate::rdf::ShaclParser;
    use crate::types::Target;
    use rudof_iri::iri;
    use rudof_rdf::RDFFormat;
    use rudof_rdf::backend::{OxigraphInMemory, ReaderMode};
    use rudof_rdf::term::Object;

    #[test]
    fn test_language_in() {
        let shape = r#"
            @prefix :    <http://example.org/> .
            @prefix sh:  <http://www.w3.org/ns/shacl#> .
            @prefix xsd: <http://www.w3.org/2001/XMLSchema#> .

            :TestShape a sh:NodeShape ;
                sh:targetNode "Hello"@en ;
                sh:languageIn ( "en" "fr" ) .
        "#;

        let shape_id = Object::iri(iri!("http://example.org/TestShape"));
        let graph = OxigraphInMemory::from_str(shape, &RDFFormat::Turtle, None, &ReaderMode::default()).unwrap();
        let ast = ShaclParser::new(graph).parse().unwrap();
        let shape = match ast.get_shape(&shape_id).unwrap() {
            ASTShape::NodeShape(ns) => ns,
            _ => unreachable!(),
        };

        match shape.components().first().unwrap() {
            ASTComponent::LanguageIn(langs) => {
                assert_eq!(langs.len(), 2);
                assert_eq!(langs[0].as_str(), "en");
                assert_eq!(langs[1].as_str(), "fr");
            },
            _ => unreachable!(),
        }
    }

    #[test]
    fn test_parse_shacl_rdf() {
        let graph = r#"
            @prefix sh: <http://www.w3.org/ns/shacl#> .
            @prefix : <http://example.org/> .

            :Shape a sh:NodeShape ;
                sh:targetClass :TargetClass .
        "#;
        let shape_id = Object::iri(iri!("http://example.org/Shape"));

        let rdf = OxigraphInMemory::from_str(graph, &RDFFormat::Turtle, None, &ReaderMode::Strict).unwrap();
        let ast = ShaclParser::new(rdf).parse().unwrap();
        let shape = ast.get_shape(&shape_id).unwrap();
        let expected_node_shape = ASTNodeShape::new(shape_id)
            .with_targets(vec![Target::Class(Object::iri(iri!("http://example.org/TargetClass")))]);

        let expected_shape = ASTShape::node_shape(expected_node_shape);
        assert_eq!(*shape, expected_shape);
    }

    const PREFIXES: &str = r#"
        @prefix :     <http://example.org/> .
        @prefix sh:   <http://www.w3.org/ns/shacl#> .
        @prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
        @prefix xsd:  <http://www.w3.org/2001/XMLSchema#> .
    "#;

    fn parse(statements: &[&str]) -> crate::ast::ASTSchema {
        let text = format!("{PREFIXES}\n{}", statements.join("\n"));
        let graph = OxigraphInMemory::from_str(&text, &RDFFormat::Turtle, None, &ReaderMode::Strict).unwrap();
        ShaclParser::new(graph).parse().unwrap()
    }

    fn ex(local: &str) -> Object {
        Object::iri(rudof_iri::IriS::new_unchecked(&format!("http://example.org/{local}")))
    }

    #[test]
    fn extra_carries_non_shacl_triples_with_language_and_datatype() {
        let ast = parse(&[r#":S a sh:NodeShape ; sh:targetClass :C ;
                rdfs:label "persona"@es , "person"@en ;
                :weight "3"^^xsd:integer ;
                :see :Other ;
                sh:property :P .
               :P sh:path :name ; rdfs:comment "plain" ; :note "n"^^xsd:string ."#]);
        let ASTShape::NodeShape(ns) = ast.get_shape(&ex("S")).unwrap() else {
            panic!("node shape")
        };
        let extra = ns.extra();
        // `sh:*` and `rdf:type` are not extra; the rest is, sorted by predicate then value.
        let predicates: Vec<&str> = extra.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(
            predicates,
            [
                "http://example.org/see",
                "http://example.org/weight",
                "http://www.w3.org/2000/01/rdf-schema#label",
                "http://www.w3.org/2000/01/rdf-schema#label",
            ]
        );
        let labels: Vec<String> = extra
            .iter()
            .filter(|(p, _)| p.as_str().ends_with("#label"))
            .map(|(_, o)| o.to_string())
            .collect();
        assert!(
            labels.iter().any(|l| l.contains("persona") && l.contains("@es")),
            "{labels:?}"
        );
        assert!(
            labels.iter().any(|l| l.contains("person") && l.contains("@en")),
            "{labels:?}"
        );
        let weight = &extra[1].1;
        assert!(weight.to_string().contains("integer"), "datatype kept: {weight}");
        assert_eq!(extra[0].1, ex("Other"));

        let ASTShape::PropertyShape(ps) = ast.get_shape(&ex("P")).unwrap() else {
            panic!("property shape")
        };
        assert_eq!(ps.extra().len(), 2);
        assert!(
            ps.extra()
                .iter()
                .all(|(p, _)| !p.as_str().starts_with("http://www.w3.org/ns/shacl#"))
        );
    }

    #[test]
    fn sh_order_is_parsed_on_property_shapes() {
        let ast = parse(&[
            ":A a sh:PropertyShape ; sh:path :a ; sh:order 2 .",
            ":B a sh:PropertyShape ; sh:path :b ; sh:order 1.5 .",
            ":C a sh:PropertyShape ; sh:path :c .",
        ]);
        let order = |l: &str| match ast.get_shape(&ex(l)).unwrap() {
            ASTShape::PropertyShape(ps) => ps.order().cloned(),
            _ => panic!("property shape"),
        };
        use rudof_rdf::term::literal::NumericLiteral;
        assert!(order("A").unwrap() > order("B").unwrap());
        assert!(order("B").unwrap() > NumericLiteral::integer(1));
        assert!(order("C").is_none());
    }

    #[test]
    fn the_ast_does_not_depend_on_the_order_of_the_input() {
        let blocks: [&str; 7] = [
            ":Z a sh:NodeShape ; sh:targetClass :Zc ; sh:property :Pz1 , :Pz2 ; rdfs:label \"z\"@en , \"zed\" .",
            ":A a sh:NodeShape ; sh:targetClass :Ac ; sh:property :Pa ; :k 1 , 2 .",
            ":Pz1 sh:path :p1 ; sh:minCount 1 ; sh:order 2 ; sh:class :X , :Y .",
            ":Pz2 sh:path :p2 ; sh:datatype xsd:string ; sh:order 1 ; sh:nodeKind sh:Literal .",
            ":Pa sh:path :pa ; sh:in ( 1 2 3 ) ; sh:or ( :Sx :Sy ) .",
            ":Sx sh:class :X .",
            ":Sy sh:class :Y .",
        ];
        let render = |order: &[usize]| {
            let statements: Vec<&str> = order.iter().map(|i| blocks[*i]).collect();
            let ast = parse(&statements);
            ast.iter()
                .map(|(id, shape)| format!("{id} -> {}", serde_json::to_string(shape).unwrap()))
                .collect::<Vec<_>>()
                .join("\n")
        };
        let first = render(&[0, 1, 2, 3, 4, 5, 6]);
        for order in [
            [6, 5, 4, 3, 2, 1, 0],
            [2, 0, 4, 6, 1, 3, 5],
            [5, 1, 3, 0, 2, 6, 4],
            [3, 6, 4, 1, 0, 5, 2],
        ] {
            let got = render(&order);
            assert_eq!(first, got, "order {order:?}");
        }
    }
}
