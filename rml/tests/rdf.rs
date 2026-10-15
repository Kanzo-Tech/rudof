//! Reading and writing mapping graphs: the round trip, the shortcuts, the
//! defaults of RML-Core §8.4.1, and what is refused rather than dropped.

use rml::Error;
use rml::ast::*;
use rml::rdf::{parse_turtle, write_turtle};
use rudof_iri::IriS;

const PREFIXES: &str = "@prefix rml: <http://w3id.org/rml/> .
@prefix ex: <http://example.com/> .
@prefix xsd: <http://www.w3.org/2001/XMLSchema#> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
";

fn parse(body: &str) -> Result<Mapping, Error> {
    parse_turtle(&format!("{PREFIXES}{body}"), Some("http://example.com/base/"))
}

fn iri(s: &str) -> IriS {
    IriS::new_unchecked(s)
}

fn xsd(local: &str) -> IriS {
    iri(&format!("http://www.w3.org/2001/XMLSchema#{local}"))
}

fn table(name: &str) -> LogicalSource {
    LogicalSource {
        source: None,
        iterable: Iterable::Sql2008Table(name.to_owned()),
    }
}

fn tm(id: &str, source: LogicalSource, subject: TermMap, poms: Vec<PredicateObjectMap>) -> TriplesMap {
    TriplesMap {
        id: Node::Iri(iri(id)),
        base_iri: None,
        logical_source: source,
        subject_map: SubjectMap {
            term: subject,
            classes: Vec::new(),
        },
        predicate_object_maps: poms,
    }
}

fn pom(predicate: &str, object: ObjectMap) -> PredicateObjectMap {
    PredicateObjectMap {
        predicates: vec![TermMap::iri(iri(predicate))],
        objects: vec![object],
    }
}

fn round_trip(m: Mapping) {
    let text = write_turtle(&m).expect("writes");
    let back = parse_turtle(&text, Some("http://example.com/base/")).unwrap_or_else(|e| panic!("{e}\n{text}"));
    assert_eq!(back, m.canonical(), "{text}");
}

/// A mapping that uses every construct the AST has.
fn rich() -> Mapping {
    let person = tm(
        "http://example.com/base/Person",
        LogicalSource {
            source: Some(iri("http://example.com/db")),
            iterable: Iterable::Sql2008Table("Person".into()),
        },
        TermMap::reference("subject").with_term_type(TermType::Iri),
        vec![
            pom(
                "http://example.com/name",
                ObjectMap::Term(TermMap::reference("name").with_datatype(xsd("string"))),
            ),
            pom(
                "http://example.com/label",
                ObjectMap::Term(TermMap::reference("label").with_language(LanguageMap::Constant("en".into()))),
            ),
            pom(
                "http://example.com/kind",
                ObjectMap::Term(TermMap::iri(iri("http://example.com/Kind"))),
            ),
            pom(
                "http://example.com/note",
                ObjectMap::Term(TermMap {
                    expression: Expression::Constant(Constant::Literal(Literal {
                        lexical_form: "n".into(),
                        kind: LiteralKind::Language("es".into()),
                    })),
                    term_type: None,
                    datatype: None,
                    language: None,
                }),
            ),
            pom(
                "http://example.com/year",
                ObjectMap::Term(TermMap {
                    expression: Expression::Constant(Constant::Literal(Literal {
                        lexical_form: "2026".into(),
                        kind: LiteralKind::Datatype(xsd("gYear")),
                    })),
                    term_type: None,
                    datatype: None,
                    language: None,
                }),
            ),
            pom(
                "http://example.com/code",
                ObjectMap::Term(
                    TermMap::template(Template::parse("\\{\\{ {ISO 3166} \\}\\}").unwrap())
                        .with_term_type(TermType::Literal),
                ),
            ),
            pom(
                "http://example.com/home",
                ObjectMap::Term(TermMap::template(
                    Template::parse("http://example.com/h/{home}").unwrap(),
                )),
            ),
            PredicateObjectMap {
                predicates: vec![
                    TermMap::iri(iri("http://example.com/a")),
                    TermMap::template(Template::parse("http://example.com/p/{pred}").unwrap()),
                ],
                objects: vec![
                    ObjectMap::Term(TermMap::reference("x")),
                    ObjectMap::Term(TermMap::reference("y").with_term_type(TermType::UnsafeIri)),
                ],
            },
            pom(
                "http://example.com/knows",
                ObjectMap::Reference(RefObjectMap {
                    parent: Node::Iri(iri("http://example.com/base/Friend")),
                    join_conditions: vec![JoinCondition {
                        child: "friend".into(),
                        parent: "id".into(),
                    }],
                }),
            ),
        ],
    );
    let mut person = person;
    person.base_iri = Some(iri("http://example.com/people/"));
    person.subject_map.classes = vec![iri("http://example.com/Person"), iri("http://example.com/Agent")];
    let friend = tm(
        "http://example.com/base/Friend",
        table("Friend"),
        TermMap::template(Template::parse("http://example.com/f/{id}").unwrap()),
        vec![],
    );
    // A blank subject, a nickname unnested from a list of (value, lang) structs, and a
    // referencing object map with no join: the two logical sources are equal.
    let query = LogicalSource {
        source: None,
        iterable: Iterable::Sql2008Query(
            "SELECT subject, n.value AS value, n.lang AS lang FROM (SELECT subject, unnest(nickname) AS n FROM Person)"
                .into(),
        ),
    };
    let nickname = tm(
        "http://example.com/base/Nickname",
        query.clone(),
        TermMap::reference("subject").with_term_type(TermType::Iri),
        vec![
            pom(
                "http://example.com/nickname",
                ObjectMap::Term(TermMap::reference("value").with_language(LanguageMap::Reference("lang".into()))),
            ),
            pom(
                "http://example.com/same",
                ObjectMap::Reference(RefObjectMap {
                    parent: Node::Iri(iri("http://example.com/base/Blank")),
                    join_conditions: vec![],
                }),
            ),
        ],
    );
    let blank = tm(
        "http://example.com/base/Blank",
        query,
        TermMap::reference("subject").with_term_type(TermType::BlankNode),
        vec![],
    );
    Mapping {
        triples_maps: vec![person, friend, nickname, blank],
        annotations: vec![
            Annotation {
                subject: iri("http://example.com/Person"),
                predicate: iri("http://www.w3.org/2000/01/rdf-schema#label"),
                object: Constant::Literal(Literal {
                    lexical_form: "Person".into(),
                    kind: LiteralKind::Language("en".into()),
                }),
            },
            Annotation {
                subject: iri("http://example.com/name"),
                predicate: iri("http://www.w3.org/2000/01/rdf-schema#comment"),
                object: Constant::Literal(Literal::string("A name")),
            },
        ],
    }
}

#[test]
fn a_mapping_that_uses_every_construct_survives_writing_and_reading() {
    round_trip(rich());
}

#[test]
fn the_written_graph_is_the_mapping_it_was_written_from_whatever_the_order() {
    let mut m = rich();
    m.triples_maps.reverse();
    m.annotations.reverse();
    for t in &mut m.triples_maps {
        t.predicate_object_maps.reverse();
    }
    round_trip(m);
}

#[test]
fn the_constant_shortcuts_and_their_long_forms_are_one_mapping() {
    let short = parse(
        "<#T> rml:logicalSource [ rml:source [] ; rml:referenceFormulation rml:SQL2008Table ; rml:iterator \"t\" ] ;
           rml:subject ex:s ;
           rml:predicateObjectMap [ rml:predicate ex:p ; rml:object ex:o ] ,
                                  [ rml:predicate ex:q ; rml:objectMap [ rml:reference \"c\" ; rml:datatype xsd:integer ; rml:language \"\" ] ] .",
    );
    // `rml:language` on a map that also has a datatype is refused: §8 allows one of the two.
    assert!(matches!(short, Err(Error::Invalid { .. })), "{short:?}");

    let short = parse(
        "<#T> rml:logicalSource [ rml:source [] ; rml:referenceFormulation rml:SQL2008Table ; rml:iterator \"t\" ] ;
           rml:subject ex:s ;
           rml:predicateObjectMap [ rml:predicate ex:p ; rml:object ex:o ] ,
                                  [ rml:predicate ex:q ; rml:objectMap [ rml:reference \"c\" ; rml:datatype xsd:integer ] ] ,
                                  [ rml:predicate ex:r ; rml:objectMap [ rml:reference \"c\" ; rml:language \"en\" ] ] .",
    )
    .unwrap();
    let long = parse(
        "<#T> rml:logicalSource [ rml:source [] ; rml:referenceFormulation rml:SQL2008Table ; rml:iterator \"t\" ] ;
           rml:subjectMap [ rml:constant ex:s ] ;
           rml:predicateObjectMap [ rml:predicateMap [ rml:constant ex:p ] ; rml:objectMap [ rml:constant ex:o ] ] ,
                                  [ rml:predicateMap [ rml:constant ex:q ] ; rml:objectMap [ rml:reference \"c\" ; rml:datatypeMap [ rml:constant xsd:integer ] ] ] ,
                                  [ rml:predicateMap [ rml:constant ex:r ] ; rml:objectMap [ rml:reference \"c\" ; rml:languageMap [ rml:constant \"en\" ] ] ] .",
    )
    .unwrap();
    assert_eq!(short, long);
}

#[test]
fn rml_core_8_4_1_the_default_term_types() {
    let m = parse(
        "<#T> rml:logicalSource [ rml:source [] ; rml:referenceFormulation rml:SQL2008Table ; rml:iterator \"t\" ] ;
           rml:subjectMap [ rml:reference \"s\" ] ;
           rml:predicateObjectMap
             [ rml:predicate ex:a ; rml:objectMap [ rml:reference \"c\" ] ] ,
             [ rml:predicate ex:b ; rml:objectMap [ rml:template \"http://example.com/{c}\" ] ] ,
             [ rml:predicate ex:c ; rml:objectMap [ rml:template \"{c}\" ; rml:language \"en\" ] ] ,
             [ rml:predicate ex:d ; rml:objectMap [ rml:template \"{c}\" ; rml:datatype xsd:string ] ] ,
             [ rml:predicate ex:e ; rml:objectMap [ rml:template \"{c}\" ; rml:termType rml:Literal ] ] .",
    )
    .unwrap();
    let t = &m.triples_maps[0];
    assert_eq!(t.subject_map.term.term_type(Position::Subject), TermType::Iri);
    let object_types: Vec<_> = t
        .predicate_object_maps
        .iter()
        .map(|p| {
            let ObjectMap::Term(o) = &p.objects[0] else { panic!() };
            (p.predicates[0].expression.clone(), o.term_type(Position::Object))
        })
        .collect();
    let of = |p: &str| {
        object_types
            .iter()
            .find(|(e, _)| *e == Expression::Constant(Constant::Iri(iri(&format!("http://example.com/{p}")))))
            .unwrap()
            .1
    };
    assert_eq!(of("a"), TermType::Literal, "reference-valued");
    assert_eq!(of("b"), TermType::Iri, "template-valued, nothing else said");
    assert_eq!(of("c"), TermType::Literal, "has a language map");
    assert_eq!(of("d"), TermType::Literal, "has a datatype map");
    assert_eq!(of("e"), TermType::Literal, "explicit");
}

#[test]
fn a_template_is_read_as_rml_core_7_1_3_reads_it() {
    // RMLTC0010c's literal template, `\\{\\{\\{ {$['ISO 3166']} \\}\\}\\}` in Turtle.
    let t = Template::parse("\\{\\{\\{ {ISO 3166} \\}\\}\\}").unwrap();
    assert_eq!(
        t.0,
        vec![
            TemplatePart::Text("{{{ ".into()),
            TemplatePart::Reference("ISO 3166".into()),
            TemplatePart::Text(" }}}".into())
        ]
    );
    assert_eq!(Template::parse(&t.to_string()).unwrap(), t);
    // The braces of a reference may be escaped (RMLTC0024-style `{$['\\{Name\\}']}`).
    let t = Template::parse("http://example.com/{\\{Name\\}}").unwrap();
    assert_eq!(t.0[1], TemplatePart::Reference("{Name}".into()));
    for bad in ["{", "}", "a{b", "{}", "{a{b}}", "a\\x", "a\\"] {
        assert!(Template::parse(bad).is_err(), "{bad:?}");
    }
}

fn source() -> &'static str {
    "rml:logicalSource [ rml:source [] ; rml:referenceFormulation rml:SQL2008Table ; rml:iterator \"t\" ]"
}

#[test]
fn what_the_ast_does_not_model_is_refused_and_not_dropped() {
    let cases = [
        ("a graph map", format!("<#T> {} ; rml:subjectMap [ rml:reference \"s\" ; rml:graph ex:g ] .", source())),
        (
            "another reference formulation",
            "<#T> rml:logicalSource [ rml:source [] ; rml:referenceFormulation rml:JSONPath ; rml:iterator \"$.a[*]\" ] ; rml:subjectMap [ rml:reference \"s\" ] .".to_owned(),
        ),
        ("a URI term type", format!("<#T> {} ; rml:subjectMap [ rml:reference \"s\" ; rml:termType rml:URI ] .", source())),
        (
            "a reference-valued datatype map",
            format!("<#T> {} ; rml:subjectMap [ rml:reference \"s\" ] ; rml:predicateObjectMap [ rml:predicate ex:p ; rml:objectMap [ rml:reference \"c\" ; rml:datatypeMap [ rml:reference \"d\" ] ] ] .", source()),
        ),
        (
            "a template-valued language map",
            format!("<#T> {} ; rml:subjectMap [ rml:reference \"s\" ] ; rml:predicateObjectMap [ rml:predicate ex:p ; rml:objectMap [ rml:reference \"c\" ; rml:languageMap [ rml:template \"{{l}}\" ] ] ] .", source()),
        ),
        (
            "a blank node map with no expression",
            format!("<#T> {} ; rml:subjectMap [ rml:termType rml:BlankNode ] .", source()),
        ),
        ("a triples map with no logical source", "<#T> rml:subjectMap [ rml:constant ex:s ] .".to_owned()),
    ];
    for (what, body) in cases {
        let r = parse(&body);
        assert!(matches!(r, Err(Error::Unsupported { .. })), "{what}: {r:?}");
    }
}

#[test]
fn what_rml_core_says_a_mapping_must_not_be_is_refused() {
    let cases = [
        // RMLTC0020b-style: "TriplesMap with two subjectMap" is an error case of the suite (§6: exactly one).
        (
            "two subject maps",
            format!("<#T> {} ; rml:subjectMap [ rml:reference \"s\" ] ; rml:subject ex:s .", source()),
        ),
        ("no subject map", format!("<#T> {} ; rml:predicateObjectMap [ rml:predicate ex:p ; rml:object ex:o ] .", source())),
        // The suite's "presence of rml:termType rml:Literal on rml:subjectMap" is an error case (§8.4).
        ("a literal subject", format!("<#T> {} ; rml:subjectMap [ rml:reference \"s\" ; rml:termType rml:Literal ] .", source())),
        (
            "a datatype and a language",
            format!("<#T> {} ; rml:subjectMap [ rml:reference \"s\" ] ; rml:predicateObjectMap [ rml:predicate ex:p ; rml:objectMap [ rml:reference \"c\" ; rml:datatype xsd:string ; rml:language \"en\" ] ] .", source()),
        ),
        (
            "a language on an IRI",
            format!("<#T> {} ; rml:subjectMap [ rml:reference \"s\" ] ; rml:predicateObjectMap [ rml:predicate ex:p ; rml:objectMap [ rml:reference \"c\" ; rml:termType rml:IRI ; rml:language \"en\" ] ] .", source()),
        ),
        ("a logical source with no rml:source (RML-IO §3.1)", "<#T> rml:logicalSource [ rml:referenceFormulation rml:SQL2008Table ; rml:iterator \"t\" ] ; rml:subjectMap [ rml:reference \"s\" ] .".to_owned()),
        ("a relational source with no reference formulation (RML-IO §3.1)", "<#T> rml:logicalSource [ rml:source [] ; rml:iterator \"t\" ] ; rml:subjectMap [ rml:reference \"s\" ] .".to_owned()),
        (
            "a class on an object map",
            format!("<#T> {} ; rml:subjectMap [ rml:reference \"s\" ] ; rml:predicateObjectMap [ rml:predicate ex:p ; rml:objectMap [ rml:reference \"c\" ; rml:class ex:C ] ] .", source()),
        ),
        (
            "an expression map with two expressions",
            format!("<#T> {} ; rml:subjectMap [ rml:reference \"s\" ; rml:template \"{{s}}\" ] .", source()),
        ),
        (
            "a referencing object map to a triples map that is not there",
            format!("<#T> {} ; rml:subjectMap [ rml:reference \"s\" ] ; rml:predicateObjectMap [ rml:predicate ex:p ; rml:objectMap [ rml:parentTriplesMap <#Nope> ] ] .", source()),
        ),
        (
            // §9.2: unequal logical sources MUST have a join condition.
            "a referencing object map across different sources with no join",
            format!("<#T> {} ; rml:subjectMap [ rml:reference \"s\" ] ; rml:predicateObjectMap [ rml:predicate ex:p ; rml:objectMap [ rml:parentTriplesMap <#U> ] ] .
             <#U> rml:logicalSource [ rml:source [] ; rml:referenceFormulation rml:SQL2008Table ; rml:iterator \"u\" ] ; rml:subjectMap [ rml:reference \"s\" ] .", source()),
        ),
    ];
    for (what, body) in cases {
        let r = parse(&body);
        assert!(matches!(r, Err(Error::Invalid { .. })), "{what}: {r:?}");
    }
}

#[test]
fn a_referencing_object_map_needs_no_join_when_the_sources_are_equal() {
    // §9.2: "effectively equal": the parent's logical source is a different node with the same properties.
    let m = parse(&format!(
        "<#T> {} ; rml:subjectMap [ rml:reference \"s\" ] ; rml:predicateObjectMap [ rml:predicate ex:p ; rml:objectMap [ rml:parentTriplesMap <#U> ] ] .
         <#U> {} ; rml:subjectMap [ rml:reference \"u\" ] .",
        source(),
        source()
    ))
    .unwrap();
    assert_eq!(m.triples_maps.len(), 2);
}

#[test]
fn documentation_on_components_is_ignored_and_annotations_are_carried() {
    let m = parse(&format!(
        "<#T> {} ; rdfs:label \"a triples map\" ;
           rml:subjectMap [ rml:reference \"s\" ; rml:class ex:C ; rdfs:comment \"documentation\" ] ;
           rml:predicateObjectMap [ rml:predicate ex:p ; rml:object ex:o ] .
         ex:C rdfs:label \"C\"@en, \"Ce\"@es ; rdfs:subClassOf ex:Super .
         ex:p rdfs:label \"p\" .
         ex:unrelated rdfs:label \"not named by the mapping\" .",
        source()
    ))
    .unwrap();
    let about = |s: &str| m.annotations.iter().filter(|a| a.subject.as_str() == s).count();
    assert_eq!(about("http://example.com/C"), 3);
    assert_eq!(about("http://example.com/p"), 1);
    assert_eq!(about("http://example.com/unrelated"), 0);
    assert_eq!(m.annotations.len(), 4);
}

#[test]
fn the_model_says_what_the_mapping_says_about_each_table() {
    let m = rich();
    let model = m.model();
    let person = model
        .tables
        .iter()
        .find(|t| t.source == Iterable::Sql2008Table("Person".into()))
        .unwrap();
    assert_eq!(person.classes.len(), 2);
    assert_eq!(person.annotations.len(), 1, "the label of the class Person");
    let name = person
        .columns
        .iter()
        .find(|c| c.predicate.as_str() == "http://example.com/name")
        .unwrap();
    assert_eq!(name.value, Expression::Reference("name".into()));
    assert_eq!(name.term_type, TermType::Literal);
    assert_eq!(name.datatype, Some(xsd("string")));
    assert_eq!(name.annotations.len(), 1, "the comment on the predicate name");
    let label = person
        .columns
        .iter()
        .find(|c| c.predicate.as_str() == "http://example.com/label")
        .unwrap();
    assert_eq!(label.language, Some(LanguageMap::Constant("en".into())));
    let kind = person
        .columns
        .iter()
        .find(|c| c.predicate.as_str() == "http://example.com/kind")
        .unwrap();
    assert_eq!(kind.term_type, TermType::Iri);
    assert_eq!(person.relations.len(), 1);
    let nickname = model
        .tables
        .iter()
        .find(|t| matches!(t.source, Iterable::Sql2008Query(_)) && !t.columns.is_empty())
        .unwrap();
    let col = nickname
        .columns
        .iter()
        .find(|c| c.predicate.as_str() == "http://example.com/nickname")
        .unwrap();
    assert_eq!(col.language, Some(LanguageMap::Reference("lang".into())));
    assert_eq!(col.value, Expression::Reference("value".into()));
}

#[test]
fn an_invalid_mapping_is_not_written() {
    let mut m = rich();
    m.triples_maps[0].subject_map.term.language = Some(LanguageMap::Constant("en".into()));
    assert!(matches!(write_turtle(&m), Err(Error::Invalid { .. })));
}

#[test]
fn rml_core_4_1_1_a_triples_map_may_state_its_base_iri() {
    let m = parse(&format!(
        "<#T> rml:baseIRI <http://example.com/baseIRI/> ; {} ; rml:subjectMap [ rml:template \"{{ID}}\" ] .
         <#U> {} ; rml:subjectMap [ rml:template \"{{ID}}\" ] .",
        source(),
        source()
    ))
    .unwrap();
    let base = |name: &str| {
        m.triples_maps
            .iter()
            .find(|t| t.id == Node::Iri(iri(&format!("http://example.com/base/#{name}"))))
            .unwrap()
            .base_iri
            .clone()
    };
    assert_eq!(base("T"), Some(iri("http://example.com/baseIRI/")));
    assert_eq!(base("U"), None);
    let again = parse_turtle(&write_turtle(&m).unwrap(), None).unwrap();
    assert_eq!(again, m);
}

/// The error cases of the KG-Construct suite (RMLTC0023a-e, RMLTC0024a, RMLTC0004b, RMLTC0012c-d) that are about
/// the mapping and not about JSON.
#[test]
fn the_suites_invalid_templates_and_constants_are_errors() {
    let with_template = |template: &str| {
        parse_turtle(
            &format!(
                "{PREFIXES}<#T> {} ; rml:subjectMap [ rml:template \"{template}\" ] .",
                source()
            ),
            None,
        )
    };
    // 0023a: `{{Name}}` encloses a reference that opens a brace.
    assert!(matches!(
        with_template("http://example.com/{{Name}}"),
        Err(Error::Invalid { .. })
    ));
    // 0023b, c, e: `\a` and `\}` are not escapes of Turtle, so the document is not Turtle.
    for template in [
        "http://example.com/{N\\ame}",
        "http://example.com/{Name\\}",
        "http://example.com/{N\\\\\\ame}",
    ] {
        assert!(matches!(with_template(template), Err(Error::Graph(_))), "{template}");
    }
    // 0023f: the escaped braces of a reference are part of it.
    assert!(with_template("http://example.com/{\\\\{Name\\\\}}").is_ok());
    // 0024a: a constant term map with an explicit term type.
    let r = parse(&format!(
        "<#T> {} ; rml:subjectMap [ rml:constant \"School\" ; rml:termType rml:BlankNode ] .",
        source()
    ));
    assert!(matches!(r, Err(Error::Invalid { .. })), "{r:?}");
}
