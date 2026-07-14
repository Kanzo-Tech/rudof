use crate::ast::ASTComponent;
use rudof_rdf::NeighsRDF;
use rudof_rdf::parser::rdf_node_parser::constructors::ObjectsPropertyParser;
use rudof_rdf::parser::rdf_node_parser::{ParserExt, RDFNodeParse};
use rudof_rdf::vocab::ShaclVocab;

/// Parses the SHACL-AF conditional constraint `sh:if` / `sh:then` / `sh:else`.
///
/// `sh:if` is required and its (single) object is the condition shape. `sh:then`
/// and `sh:else` are optional single shape references. When there is no `sh:if`
/// on the focus node this produces no component (empty vec), mirroring how
/// `xone()` / `not()` yield nothing when their predicate is absent.
///
/// All three predicates are read from the SAME focus node (the shape declaring
/// the conditional), so — unlike `not`/`xone` — this cannot use
/// `parse_components_for_iri` (which re-focuses on the `sh:if` object and would
/// lose access to the sibling `sh:then`/`sh:else`).
pub(crate) fn if_<RDF: NeighsRDF>() -> impl RDFNodeParse<RDF, Output = Vec<ASTComponent>> {
    ObjectsPropertyParser::new(ShaclVocab::sh_if())
        .and(ObjectsPropertyParser::new(ShaclVocab::sh_then()))
        .and(ObjectsPropertyParser::new(ShaclVocab::sh_else()))
        .flat_map(|((ifs, thens), elses)| {
            Ok(ifs
                .into_iter()
                .map(|cond| ASTComponent::If {
                    cond,
                    then_: thens.first().cloned(),
                    else_: elses.first().cloned(),
                })
                .collect())
        })
}
