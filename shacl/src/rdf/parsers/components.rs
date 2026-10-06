use crate::ast::ASTComponent;
use crate::rdf::error::ShaclParserError;
use crate::rdf::parsers::utils::{iri_set_components, iri_sets, path_components, shape_components, terms_as_nodes};
use crate::rdf::parsers::{
    basic_sparql, closed, has_value, if_, in_component, language_in, pattern, qualified_value_shape,
};
use crate::types::NodeKind;
use rudof_iri::IriS;
use rudof_rdf::NeighsRDF;
use rudof_rdf::parser::rdf_node_parser::constructors::{
    BoolsPropertyParser, IntegersPropertyParser, ListParser, LiteralsPropertyParser,
};
use rudof_rdf::parser::rdf_node_parser::{ParserExt, RDFNodeParse};
use rudof_rdf::term::literal::ConcreteLiteral;
use rudof_rdf::vocab::ShaclVocab as Sh;

type Components<RDF> = Box<dyn RDFNodeParse<RDF, Output = Vec<ASTComponent>>>;

/// Every constraint component of the focus shape, in the order of SHACL 1.2
/// Core §4–§8. `sh:property` is not one here: a shape's property shapes are
/// parsed apart, as shapes.
pub(crate) fn components<RDF: NeighsRDF + 'static>() -> impl RDFNodeParse<RDF, Output = Vec<ASTComponent>> {
    let parsers: Vec<Components<RDF>> = vec![
        // Value type
        Box::new(iri_set_components(Sh::sh_datatype(), ASTComponent::Datatype)),
        Box::new(node_kind()),
        // Cardinality
        integers(Sh::sh_min_count(), ASTComponent::MinCount),
        integers(Sh::sh_max_count(), ASTComponent::MaxCount),
        // Value range
        literals(Sh::sh_min_inclusive(), ASTComponent::MinInclusive),
        literals(Sh::sh_min_exclusive(), ASTComponent::MinExclusive),
        literals(Sh::sh_max_inclusive(), ASTComponent::MaxInclusive),
        literals(Sh::sh_max_exclusive(), ASTComponent::MaxExclusive),
        // String based
        integers(Sh::sh_min_length(), ASTComponent::MinLength),
        integers(Sh::sh_max_length(), ASTComponent::MaxLength),
        Box::new(pattern()),
        booleans(Sh::sh_single_line(), ASTComponent::SingleLine),
        Box::new(language_in()),
        booleans(Sh::sh_unique_lang(), ASTComponent::UniqueLang),
        // List
        Box::new(shape_components(Sh::sh_member_shape(), ASTComponent::MemberShape)),
        integers(Sh::sh_min_list_length(), ASTComponent::MinListLength),
        integers(Sh::sh_max_list_length(), ASTComponent::MaxListLength),
        booleans(Sh::sh_unique_members(), ASTComponent::UniqueMembers),
        // Property pair
        Box::new(path_components(Sh::sh_equals(), ASTComponent::Equals)),
        Box::new(path_components(Sh::sh_disjoint(), ASTComponent::Disjoint)),
        Box::new(path_components(Sh::sh_subset_of(), ASTComponent::SubsetOf)),
        Box::new(path_components(Sh::sh_less_than(), ASTComponent::LessThan)),
        Box::new(path_components(
            Sh::sh_less_than_or_equals(),
            ASTComponent::LessThanOrEquals,
        )),
        // Logical
        Box::new(shape_components(Sh::sh_not(), ASTComponent::Not)),
        shape_lists(Sh::sh_and(), ASTComponent::And),
        shape_lists(Sh::sh_or(), ASTComponent::Or),
        shape_lists(Sh::sh_xone(), ASTComponent::Xone),
        Box::new(if_()),
        // Shape based
        Box::new(shape_components(Sh::sh_node(), ASTComponent::Node)),
        Box::new(shape_components(
            Sh::sh_node_by_expression(),
            ASTComponent::NodeByExpression,
        )),
        Box::new(shape_components(Sh::sh_some_value(), ASTComponent::SomeValue)),
        Box::new(qualified_value_shape()),
        // Other
        Box::new(closed()),
        Box::new(has_value()),
        Box::new(in_component()),
        Box::new(iri_set_components(Sh::sh_root_class(), ASTComponent::RootClass)),
        Box::new(iri_set_components(
            Sh::sh_unique_values_for(),
            ASTComponent::UniqueValuesFor,
        )),
        // SPARQL-based constraints
        Box::new(basic_sparql()),
        booleans(Sh::sh_deactivated(), ASTComponent::Deactivated),
    ];

    iri_set_components(Sh::sh_class(), ASTComponent::Class).combine_many(parsers)
}

fn integers<RDF: NeighsRDF + 'static>(parameter: IriS, component: fn(isize) -> ASTComponent) -> Components<RDF> {
    Box::new(IntegersPropertyParser::new(parameter).map(move |ns| ns.into_iter().map(component).collect()))
}

fn booleans<RDF: NeighsRDF + 'static>(parameter: IriS, component: fn(bool) -> ASTComponent) -> Components<RDF> {
    Box::new(BoolsPropertyParser::new(parameter).map(move |bs| bs.into_iter().map(component).collect()))
}

fn literals<RDF: NeighsRDF + 'static>(
    parameter: IriS,
    component: fn(ConcreteLiteral) -> ASTComponent,
) -> Components<RDF> {
    Box::new(LiteralsPropertyParser::new(parameter).map(move |ls| ls.into_iter().map(component).collect()))
}

/// A list-taking, shape-expecting parameter (`sh:and`, `sh:or`, `sh:xone`).
fn shape_lists<RDF: NeighsRDF + 'static>(
    parameter: IriS,
    component: fn(Vec<rudof_rdf::term::Object>) -> ASTComponent,
) -> Components<RDF> {
    Box::new(
        ListParser::new()
            .flat_map(move |terms| Ok(component(terms_as_nodes::<RDF>(terms)?)))
            .map_property(parameter),
    )
}

/// `sh:nodeKind`: a node kind, or a SHACL list of the four simple ones.
fn node_kind<RDF: NeighsRDF>() -> impl RDFNodeParse<RDF, Output = Vec<ASTComponent>> {
    iri_sets(Sh::sh_node_kind()).flat_map(|sets| {
        sets.into_iter()
            .map(|iris| {
                let kinds = iris.iter().map(node_kind_of).collect::<Result<Vec<_>, _>>()?;
                Ok(ASTComponent::NodeKind(kinds))
            })
            .collect::<Result<Vec<_>, ShaclParserError>>()
            .map_err(|e| rudof_rdf::RDFError::ParseFailError { msg: e.to_string() })
    })
}

fn node_kind_of(iri: &IriS) -> Result<NodeKind, ShaclParserError> {
    Ok(match iri.as_str() {
        Sh::SH_IRI => NodeKind::Iri,
        Sh::SH_LITERAL => NodeKind::Lit,
        Sh::SH_BLANK_NODE => NodeKind::BNode,
        Sh::SH_BLANK_NODE_OR_IRI => NodeKind::BNodeOrIri,
        Sh::SH_BLANK_NODE_OR_LITERAL => NodeKind::BNodeOrLit,
        Sh::SH_IRI_OR_LITERAL => NodeKind::IriOrLit,
        Sh::SH_TRIPLE_TERM => NodeKind::TripleTerm,
        _ => return Err(ShaclParserError::UnknownNodeKind(iri.to_string())),
    })
}
