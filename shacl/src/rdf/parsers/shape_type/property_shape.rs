use crate::ast::ASTPropertyShape;
use crate::rdf::parsers::components::components;
use crate::rdf::parsers::non_shape::message;
use crate::rdf::parsers::{path, property, reifier_shape, severity, targets, texts};
use rudof_rdf::NeighsRDF;
use rudof_rdf::parser::rdf_node_parser::constructors::{
    FocusParser, HasTypeParser, LiteralsPropertyParser, ObjectParser, SetFocusParser, SuccessParser,
};
use rudof_rdf::parser::rdf_node_parser::{ParserExt, RDFNodeParse};
use rudof_rdf::term::literal::ConcreteLiteral;
use rudof_rdf::vocab::ShaclVocab;

pub(crate) fn property_shape<RDF: NeighsRDF + 'static>() -> impl RDFNodeParse<RDF, Output = ASTPropertyShape> {
    FocusParser::new().then(move |focus: RDF::Term| {
        HasTypeParser::new(ShaclVocab::sh_property_shape())
            .optional()
            .with(
                ObjectParser::new()
                    .and(path())
                    .then(move |(id, path)| SuccessParser::new(ASTPropertyShape::new(id, path))),
            )
            .then(move |ps| SetFocusParser::new(focus.clone()).with(SuccessParser::new(ps)))
            .then(|ps| {
                severity()
                    .optional()
                    .flat_map(move |sev| Ok(ps.clone().with_severity(sev)))
            })
            .then(|ps| {
                message()
                    .optional()
                    .flat_map(move |msg| Ok(ps.clone().with_message(msg)))
            })
            .then(|ps| texts(ShaclVocab::sh_name()).flat_map(move |name| Ok(ps.clone().with_name(name))))
            .then(|ps| {
                texts(ShaclVocab::sh_description())
                    .flat_map(move |description| Ok(ps.clone().with_description(description)))
            })
            .then(|ps| {
                // `sh:order` is a single decimal; were there several, the least is
                // taken so that the answer does not depend on the read order.
                LiteralsPropertyParser::new(ShaclVocab::sh_order()).flat_map(move |lits| {
                    let order = lits
                        .iter()
                        .filter_map(ConcreteLiteral::numeric_operand)
                        .reduce(|a, b| if b < a { b } else { a });
                    Ok(ps.clone().with_order(order))
                })
            })
            .then(|ps| reifier_shape().flat_map(move |r_shape| Ok(ps.clone().with_reifier_shape(r_shape))))
            .then(|ps| targets().flat_map(move |ts| Ok(ps.clone().with_targets(ts))))
            .then(|ps| {
                property()
                    .flat_map(move |prop_shapes| Ok(ps.clone().with_property_shapes(prop_shapes)))
                    .then(move |ps_with_props| {
                        components().flat_map(move |cs| Ok(ps_with_props.clone().with_components(cs)))
                    })
            })
    })
}
