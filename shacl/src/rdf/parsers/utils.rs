use crate::ast::ASTComponent;
use crate::types::Value;
use prefixmap::IriRef;
use rudof_iri::IriS;
use rudof_rdf::parser::rdf_node_parser::constructors::{FocusParser, ListParser, ShaclPathParser, TermParser};
use rudof_rdf::parser::rdf_node_parser::utils::term_to_iri;
use rudof_rdf::parser::rdf_node_parser::{ParserExt, RDFNodeParse};
use rudof_rdf::term::literal::ConcreteLiteral;
use rudof_rdf::term::{Iri, Object, Term};
use rudof_rdf::{NeighsRDF, RDFError, Rdf, SHACLPath};

pub(crate) fn parse_components_for_iri<RDF, P>(
    iri: IriS,
    component_parser: P,
) -> impl RDFNodeParse<RDF, Output = Vec<ASTComponent>>
where
    RDF: NeighsRDF,
    P: RDFNodeParse<RDF, Output = ASTComponent>,
{
    component_parser.map_property(iri)
}

/// The values of `parameter`, each an IRI or, as SHACL 1.2 Core allows for
/// `sh:class`, `sh:datatype`, `sh:nodeKind`, `sh:rootClass` and
/// `sh:uniqueValuesFor`, a SHACL list of IRIs: one set per value.
pub(crate) fn iri_sets<RDF: NeighsRDF>(parameter: IriS) -> impl RDFNodeParse<RDF, Output = Vec<Vec<IriS>>> {
    let one = TermParser::new().flat_map(|t: RDF::Term| term_to_iri::<RDF>(&t).map(|iri| vec![iri]));
    let list = ListParser::new()
        .flat_map(|terms: Vec<RDF::Term>| terms.iter().map(term_to_iri::<RDF>).collect::<Result<Vec<_>, _>>());
    one.or(list).map_property(parameter)
}

/// The values of `parameter` as `component`s, each an IRI or a list of them.
pub(crate) fn iri_set_components<RDF: NeighsRDF>(
    parameter: IriS,
    component: fn(Vec<IriRef>) -> ASTComponent,
) -> impl RDFNodeParse<RDF, Output = Vec<ASTComponent>> {
    iri_sets(parameter).map(move |sets| {
        sets.into_iter()
            .map(|iris| component(iris.into_iter().map(IriRef::iri).collect()))
            .collect()
    })
}

/// The values of a shape-expecting `parameter` as `component`s.
pub(crate) fn shape_components<RDF: NeighsRDF>(
    parameter: IriS,
    component: fn(Object) -> ASTComponent,
) -> impl RDFNodeParse<RDF, Output = Vec<ASTComponent>> {
    TermParser::new()
        .flat_map(move |t: RDF::Term| {
            let shape =
                RDF::term_as_object(&t).map_err(|_| RDFError::FailedTermToObjectError { term: t.to_string() })?;
            Ok(component(shape))
        })
        .map_property(parameter)
}

/// The values of `parameter`, each a SHACL property path, as `component`s.
pub(crate) fn path_components<RDF: NeighsRDF>(
    parameter: IriS,
    component: fn(SHACLPath) -> ASTComponent,
) -> impl RDFNodeParse<RDF, Output = Vec<ASTComponent>> {
    FocusParser::new()
        .then(ShaclPathParser::new)
        .map(component)
        .map_property(parameter)
}

pub(crate) fn terms_as_nodes<RDF: Rdf>(terms: Vec<RDF::Term>) -> Result<Vec<Object>, RDFError> {
    terms
        .into_iter()
        .map(|t| {
            let term_name = t.to_string();
            RDF::term_as_object(&t).map_err(|_| RDFError::FailedTermToRDFNodeError { term: term_name })
        })
        .collect()
}

pub(crate) fn term_to_value<RDF: Rdf>(term: &RDF::Term, msg: &str) -> Result<Value, RDFError> {
    if term.is_blank_node() {
        Err(RDFError::ExpectedIriOrBlankNodeError {
            term: term.to_string(),
            error: msg.to_string(),
        })
    } else if let Ok(iri) = RDF::term_as_iri(term) {
        let iri: RDF::IRI = iri;
        let iri_string = iri.as_str();
        let iri_s = IriS::new_unchecked(iri_string);
        Ok(Value::Iri(IriRef::Iri(iri_s)))
    } else if let Ok(literal) = RDF::term_as_literal(term) {
        let literal: RDF::Literal = literal;
        let slit: ConcreteLiteral = literal.clone().try_into().map_err(|_| RDFError::LiteralAsSLiteral {
            literal: literal.to_string(),
        })?;
        Ok(Value::Literal(slit))
    } else {
        // TODO - return error?
        println!("Unexpected code in term_to_value: {term}: {msg}");
        todo!()
    }
}
