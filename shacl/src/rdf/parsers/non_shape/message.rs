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
