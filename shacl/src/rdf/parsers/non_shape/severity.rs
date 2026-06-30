use crate::types::Severity;
use rudof_rdf::NeighsRDF;
use rudof_rdf::parser::rdf_node_parser::constructors::SingleIriPropertyParser;
use rudof_rdf::parser::rdf_node_parser::{ParserExt, RDFNodeParse};
use rudof_rdf::vocab::ShaclVocab;

pub(crate) fn severity<RDF: NeighsRDF>() -> impl RDFNodeParse<RDF, Output = Severity> {
    SingleIriPropertyParser::new(ShaclVocab::sh_severity()).map(|iri| match iri.as_str() {
        ShaclVocab::SH_VIOLATION => Severity::Violation,
        ShaclVocab::SH_WARNING => Severity::Warning,
        ShaclVocab::SH_INFO => Severity::Info,
        ShaclVocab::SH_DEBUG => Severity::Debug,
        ShaclVocab::SH_TRACE => Severity::Trace,
        _ => Severity::Generic(iri),
    })
}
