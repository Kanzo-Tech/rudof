use crate::types::MessageMap;
use rudof_rdf::parser::rdf_node_parser::constructors::LiteralsPropertyParser;
use rudof_rdf::parser::rdf_node_parser::{ParserExt, RDFNodeParse};
use rudof_rdf::vocab::ShaclVocab;
use rudof_rdf::{NeighsRDF, RDFError};

pub(crate) fn message<RDF: NeighsRDF>() -> impl RDFNodeParse<RDF, Output = MessageMap> {
    LiteralsPropertyParser::new(ShaclVocab::sh_message()).flat_map(|lits| {
        if lits.is_empty() {
            return Err(RDFError::ParseFailError {
                msg: "No sh:message found".to_string(),
            });
        }
        let map = lits.into_iter().fold(MessageMap::new(), |acc, lit| {
            let lang = lit.lang();
            let text = lit.lexical_form().to_string();
            acc.with_message(lang, text)
        });
        Ok(map)
    })
}

/// The literals of `predicate` (`sh:name`, `sh:description`) by language tag; an empty
/// map when there are none. Of two values in one language the greater lexical form is
/// kept, so the answer does not depend on the order they were read in.
pub(crate) fn texts<RDF: NeighsRDF>(predicate: rudof_iri::IriS) -> impl RDFNodeParse<RDF, Output = MessageMap> {
    LiteralsPropertyParser::new(predicate).map(|lits| {
        let mut lits: Vec<_> = lits.iter().map(|l| (l.lang(), l.lexical_form().to_string())).collect();
        lits.sort();
        lits.into_iter()
            .fold(MessageMap::new(), |acc, (lang, text)| acc.with_message(lang, text))
    })
}
