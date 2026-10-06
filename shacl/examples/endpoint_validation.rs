//! Validates a node of Wikidata, read through its SPARQL endpoint.
//!
//! Only the arcs the shapes reach from their focus nodes are fetched: the
//! evaluator looks up a path's predicate node by node.

#[cfg(target_family = "wasm")]
fn main() {}

#[cfg(not(target_family = "wasm"))]
fn main() -> anyhow::Result<()> {
    use prefixmap::PrefixMap;
    use rudof_iri::iri;
    use rudof_rdf::RDFFormat;
    use rudof_rdf::backend::{OxigraphEndpoint, OxigraphInMemory, ReaderMode};
    use shacl::ir::IRSchema;
    use shacl::rdf::ShaclParser;

    let shapes = r#"
        @prefix ex:  <http://example.org/> .
        @prefix wd:  <http://www.wikidata.org/entity/> .
        @prefix wdt: <http://www.wikidata.org/prop/direct/> .
        @prefix sh:  <http://www.w3.org/ns/shacl#> .
        @prefix xsd: <http://www.w3.org/2001/XMLSchema#> .

        ex:WikidataExampleShape
            a sh:NodeShape ;
            sh:targetNode wd:Q80 ;
            sh:property [
                sh:path     wdt:P1477 ;
                sh:minCount 1 ;
                sh:maxCount 1 ;
                sh:datatype xsd:string ;
            ] .
    "#;

    let graph = OxigraphInMemory::from_str(shapes, &RDFFormat::Turtle, None, &ReaderMode::default())?;
    let schema = IRSchema::try_from(&ShaclParser::new(graph).parse()?)?;
    let wikidata = OxigraphEndpoint::new(&iri!("https://query.wikidata.org/sparql"), &PrefixMap::default())?;

    let report = shacl::validator::validate(&schema, &wikidata)?;
    println!("{report}");
    Ok(())
}
