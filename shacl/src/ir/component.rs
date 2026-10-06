use crate::ast::{ASTComponent, ASTSchema};
use crate::ir::components::{BasicSparql, Closed, If, Pattern, QualifiedValueShape};
use crate::ir::dg::{DependencyGraph, PosNeg};
use crate::ir::error::IRError;
use crate::ir::schema::IRSchema;
use crate::ir::shape::IRShape;
use crate::ir::shape_label_idx::ShapeLabelIdx;
use crate::ir::{convert_iri_ref, convert_value};
use crate::types::NodeKind;
use itertools::Itertools;
use prefixmap::IriRef;
use rudof_iri::IriS;
use rudof_rdf::term::Object;
use rudof_rdf::term::literal::{ConcreteLiteral, Lang};
use rudof_rdf::vocab::ShaclVocab;
use rudof_rdf::{BuildRDF, SHACLPath};
use std::collections::{HashMap, HashSet};
use std::fmt::{Display, Formatter};

/// A compiled SHACL constraint component.
///
/// Components are stored INLINE as their payload: a scalar, a term, the
/// [`ShapeLabelIdx`] of the shapes they refer to. Only the components that
/// carry genuine compiled state keep a struct payload (see
/// [`crate::ir::components`]).
#[derive(Debug, Clone)]
pub enum IRComponent {
    /// The permitted classes: one, or the members of an `sh:class` list.
    Class(Vec<IriS>),
    /// The permitted datatypes: one, or the members of an `sh:datatype` list.
    Datatype(Vec<IriS>),
    /// The permitted node kinds: one, or the members of an `sh:nodeKind` list.
    NodeKind(Vec<NodeKind>),
    MinCount(isize),
    MaxCount(isize),
    MinExclusive(ConcreteLiteral),
    MaxExclusive(ConcreteLiteral),
    MinInclusive(ConcreteLiteral),
    MaxInclusive(ConcreteLiteral),
    MinLength(isize),
    MaxLength(isize),
    Pattern(Pattern),
    SingleLine(bool),
    UniqueLang(bool),
    LanguageIn(Vec<Lang>),
    MemberShape(ShapeLabelIdx),
    MinListLength(isize),
    MaxListLength(isize),
    UniqueMembers(bool),
    Equals(SHACLPath),
    Disjoint(SHACLPath),
    SubsetOf(SHACLPath),
    LessThan(SHACLPath),
    LessThanOrEquals(SHACLPath),
    Or(Vec<ShapeLabelIdx>),
    And(Vec<ShapeLabelIdx>),
    Not(ShapeLabelIdx),
    Xone(Vec<ShapeLabelIdx>),
    If(If),
    Node(ShapeLabelIdx),
    NodeByExpression(ShapeLabelIdx),
    SomeValue(ShapeLabelIdx),
    HasValue(Object),
    In(Vec<Object>),
    RootClass(Vec<IriS>),
    UniqueValuesFor(Vec<IriS>),
    QualifiedValueShape(QualifiedValueShape),
    Closed(Closed),
    Deactivated(bool),
    BasicSparql(BasicSparql),
}

fn iris(iris: Vec<IriRef>) -> Result<Vec<IriS>, IRError> {
    iris.into_iter().map(convert_iri_ref).collect()
}

impl IRComponent {
    /// Compiles an AST component, registering the shapes it refers to.
    pub fn compile(component: &ASTComponent, ast: &ASTSchema, ir: &mut IRSchema) -> Result<Self, IRError> {
        let shape = |ir: &mut IRSchema, o: &Object| ir.register_shape(o, None, ast);
        let result = match component.clone() {
            ASTComponent::Class(cs) => IRComponent::Class(iris(cs)?),
            ASTComponent::Datatype(ds) => IRComponent::Datatype(iris(ds)?),
            ASTComponent::NodeKind(nk) => IRComponent::NodeKind(nk),
            ASTComponent::MinCount(n) => IRComponent::MinCount(check_non_negative("sh:minCount", n)?),
            ASTComponent::MaxCount(n) => IRComponent::MaxCount(check_non_negative("sh:maxCount", n)?),
            ASTComponent::MinExclusive(lit) => IRComponent::MinExclusive(lit),
            ASTComponent::MaxExclusive(lit) => IRComponent::MaxExclusive(lit),
            ASTComponent::MinInclusive(lit) => IRComponent::MinInclusive(lit),
            ASTComponent::MaxInclusive(lit) => IRComponent::MaxInclusive(lit),
            ASTComponent::MinLength(l) => IRComponent::MinLength(l),
            ASTComponent::MaxLength(l) => IRComponent::MaxLength(l),
            ASTComponent::Pattern { pattern, flags } => IRComponent::Pattern(Pattern::new(pattern, flags)?),
            ASTComponent::SingleLine(b) => IRComponent::SingleLine(b),
            ASTComponent::UniqueLang(b) => IRComponent::UniqueLang(b),
            ASTComponent::LanguageIn(langs) => IRComponent::LanguageIn(langs),
            ASTComponent::MemberShape(o) => IRComponent::MemberShape(shape(ir, &o)?),
            ASTComponent::MinListLength(n) => IRComponent::MinListLength(n),
            ASTComponent::MaxListLength(n) => IRComponent::MaxListLength(n),
            ASTComponent::UniqueMembers(b) => IRComponent::UniqueMembers(b),
            ASTComponent::Equals(p) => IRComponent::Equals(p),
            ASTComponent::Disjoint(p) => IRComponent::Disjoint(p),
            ASTComponent::SubsetOf(p) => IRComponent::SubsetOf(p),
            ASTComponent::LessThan(p) => IRComponent::LessThan(p),
            ASTComponent::LessThanOrEquals(p) => IRComponent::LessThanOrEquals(p),
            ASTComponent::Or(objs) => IRComponent::Or(ir.register_shapes(objs, ast)?),
            ASTComponent::And(objs) => IRComponent::And(ir.register_shapes(objs, ast)?),
            ASTComponent::Not(o) => IRComponent::Not(shape(ir, &o)?),
            ASTComponent::Xone(objs) => IRComponent::Xone(ir.register_shapes(objs, ast)?),
            ASTComponent::If { cond, then_, else_ } => {
                let cond = shape(ir, &cond)?;
                let then_ = then_.as_ref().map(|o| shape(ir, o)).transpose()?;
                let else_ = else_.as_ref().map(|o| shape(ir, o)).transpose()?;
                IRComponent::If(If::new(cond, then_, else_))
            },
            ASTComponent::Closed {
                is_closed,
                ignored_properties,
                by_types,
            } => {
                let mut ignored = ignored_properties.into_iter().collect_vec();
                ignored.sort_by(|a, b| a.as_str().cmp(b.as_str()));
                IRComponent::Closed(Closed::new(is_closed, ignored, by_types))
            },
            ASTComponent::Node(o) => IRComponent::Node(shape(ir, &o)?),
            ASTComponent::NodeByExpression(o) => IRComponent::NodeByExpression(shape(ir, &o)?),
            ASTComponent::SomeValue(o) => IRComponent::SomeValue(shape(ir, &o)?),
            ASTComponent::HasValue(val) => IRComponent::HasValue(convert_value(val)?),
            ASTComponent::In(vals) => {
                IRComponent::In(vals.into_iter().map(convert_value).collect::<Result<Vec<_>, _>>()?)
            },
            ASTComponent::RootClass(cs) => IRComponent::RootClass(iris(cs)?),
            ASTComponent::UniqueValuesFor(ps) => IRComponent::UniqueValuesFor(iris(ps)?),
            ASTComponent::QualifiedValueShape {
                shape: qvs,
                q_min_count,
                q_max_count,
                disjoint,
                siblings,
            } => {
                let idx = shape(ir, &qvs)?;
                let siblings = ir.register_shapes(siblings, ast)?;
                IRComponent::QualifiedValueShape(QualifiedValueShape::new(
                    idx,
                    q_min_count,
                    q_max_count,
                    disjoint,
                    siblings,
                ))
            },
            ASTComponent::Deactivated(d) => IRComponent::Deactivated(d),
            ASTComponent::BasicSparql {
                select,
                deactivated,
                prefixes,
                message,
            } => IRComponent::BasicSparql(
                BasicSparql::new(select)
                    .with_deactivated(deactivated)
                    .with_prefixes(prefixes)
                    .with_message(message),
            ),
        };
        Ok(result)
    }

    /// The shapes the component refers to, with the polarity a conforming
    /// value node has against each: `sh:not` flips it.
    pub fn shapes(&self) -> Vec<(ShapeLabelIdx, bool)> {
        let pos = |s: &ShapeLabelIdx| (*s, true);
        match self {
            IRComponent::Or(ss) | IRComponent::And(ss) | IRComponent::Xone(ss) => ss.iter().map(pos).collect(),
            IRComponent::Not(s) => vec![(*s, false)],
            IRComponent::Node(s)
            | IRComponent::NodeByExpression(s)
            | IRComponent::SomeValue(s)
            | IRComponent::MemberShape(s) => vec![pos(s)],
            IRComponent::If(if_) => std::iter::once(if_.cond())
                .chain(if_.then())
                .chain(if_.els())
                .map(pos)
                .collect(),
            // The sibling shapes are compared, not depended on.
            IRComponent::QualifiedValueShape(qvs) => vec![pos(qvs.shape())],
            _ => Vec::new(),
        }
    }

    /// The dependency-graph edges of the component: one to each shape it
    /// refers to, and that shape's own edges.
    pub fn add_edges(
        &self,
        idx: ShapeLabelIdx,
        dg: &mut DependencyGraph,
        posneg: PosNeg,
        ir: &IRSchema,
        cache: &mut HashSet<ShapeLabelIdx>,
    ) {
        for (shape_idx, positive) in self.shapes() {
            let posneg = if positive { posneg } else { posneg.change() };
            let Some(shape) = ir.get_shape_from_idx(&shape_idx) else {
                continue;
            };
            dg.add_edge(idx, shape_idx, posneg);
            if cache.insert(shape_idx) {
                shape.add_edges(shape_idx, dg, posneg, ir, cache);
            }
        }
    }
}

impl IRComponent {
    pub fn register<RDF: BuildRDF>(
        &self,
        id: &Object,
        graph: &mut RDF,
        shape_map: &HashMap<ShapeLabelIdx, IRShape>,
    ) -> Result<(), IRError> {
        let shape_term = |idx: &ShapeLabelIdx| -> Result<RDF::Term, IRError> {
            let shape = shape_map.get(idx).ok_or(IRError::ShapeNotFound(*idx))?;
            Ok(shape.id().clone().into())
        };
        let shapes = |idxs: &[ShapeLabelIdx], predicate: IriS, graph: &mut RDF| {
            idxs.iter()
                .try_for_each(|idx| register_term(&shape_term(idx)?, predicate.clone(), id, graph))
        };
        // As `sh:in` does, list-valued parameters are written as repeated
        // values; the list form is not reconstructed.
        let iris = |iris: &[IriS], predicate: IriS, graph: &mut RDF| {
            iris.iter()
                .try_for_each(|iri| register_iri(iri, predicate.clone(), id, graph))
        };
        let path = |path: &SHACLPath, predicate: IriS, graph: &mut RDF| match path {
            SHACLPath::Predicate { pred } => register_iri(pred, predicate, id, graph),
            other => Err(IRError::UnsupportedPathSerialization(Box::new(other.clone()))),
        };
        match self {
            IRComponent::Class(cs) => iris(cs, ShaclVocab::sh_class(), graph),
            IRComponent::Datatype(ds) => iris(ds, ShaclVocab::sh_datatype(), graph),
            IRComponent::NodeKind(nks) => {
                let kinds = nks.iter().map(node_kind_iri).collect_vec();
                iris(&kinds, ShaclVocab::sh_node_kind(), graph)
            },
            IRComponent::MinCount(n) => register_integer(*n, ShaclVocab::sh_min_count(), id, graph),
            IRComponent::MaxCount(n) => register_integer(*n, ShaclVocab::sh_max_count(), id, graph),
            IRComponent::MinExclusive(l) => register_literal(l, ShaclVocab::sh_min_exclusive(), id, graph),
            IRComponent::MaxExclusive(l) => register_literal(l, ShaclVocab::sh_max_exclusive(), id, graph),
            IRComponent::MinInclusive(l) => register_literal(l, ShaclVocab::sh_min_inclusive(), id, graph),
            IRComponent::MaxInclusive(l) => register_literal(l, ShaclVocab::sh_max_inclusive(), id, graph),
            IRComponent::MinLength(n) => register_integer(*n, ShaclVocab::sh_min_length(), id, graph),
            IRComponent::MaxLength(n) => register_integer(*n, ShaclVocab::sh_max_length(), id, graph),
            IRComponent::Pattern(p) => {
                if let Some(flags) = p.flags() {
                    register_literal(&ConcreteLiteral::str(flags), ShaclVocab::sh_flags(), id, graph)?;
                }
                register_literal(&ConcreteLiteral::str(p.pattern()), ShaclVocab::sh_pattern(), id, graph)
            },
            IRComponent::SingleLine(b) => register_boolean(*b, ShaclVocab::sh_single_line(), id, graph),
            IRComponent::UniqueLang(b) => register_boolean(*b, ShaclVocab::sh_unique_lang(), id, graph),
            IRComponent::LanguageIn(langs) => langs.iter().try_for_each(|l| {
                register_literal(
                    &ConcreteLiteral::str(&l.to_string()),
                    ShaclVocab::sh_language_in(),
                    id,
                    graph,
                )
            }),
            IRComponent::MemberShape(s) => shapes(&[*s], ShaclVocab::sh_member_shape(), graph),
            IRComponent::MinListLength(n) => register_integer(*n, ShaclVocab::sh_min_list_length(), id, graph),
            IRComponent::MaxListLength(n) => register_integer(*n, ShaclVocab::sh_max_list_length(), id, graph),
            IRComponent::UniqueMembers(b) => register_boolean(*b, ShaclVocab::sh_unique_members(), id, graph),
            IRComponent::Equals(p) => path(p, ShaclVocab::sh_equals(), graph),
            IRComponent::Disjoint(p) => path(p, ShaclVocab::sh_disjoint(), graph),
            IRComponent::SubsetOf(p) => path(p, ShaclVocab::sh_subset_of(), graph),
            IRComponent::LessThan(p) => path(p, ShaclVocab::sh_less_than(), graph),
            IRComponent::LessThanOrEquals(p) => path(p, ShaclVocab::sh_less_than_or_equals(), graph),
            IRComponent::Or(ss) => shapes(ss, ShaclVocab::sh_or(), graph),
            IRComponent::And(ss) => shapes(ss, ShaclVocab::sh_and(), graph),
            IRComponent::Not(s) => shapes(&[*s], ShaclVocab::sh_not(), graph),
            IRComponent::Xone(ss) => shapes(ss, ShaclVocab::sh_xone(), graph),
            IRComponent::If(if_) => {
                shapes(&[*if_.cond()], ShaclVocab::sh_if(), graph)?;
                shapes(
                    &if_.then().copied().into_iter().collect_vec(),
                    ShaclVocab::sh_then(),
                    graph,
                )?;
                shapes(
                    &if_.els().copied().into_iter().collect_vec(),
                    ShaclVocab::sh_else(),
                    graph,
                )
            },
            IRComponent::Node(s) => shapes(&[*s], ShaclVocab::sh_node(), graph),
            IRComponent::NodeByExpression(s) => shapes(&[*s], ShaclVocab::sh_node_by_expression(), graph),
            IRComponent::SomeValue(s) => shapes(&[*s], ShaclVocab::sh_some_value(), graph),
            IRComponent::HasValue(v) => register_value(v, ShaclVocab::sh_has_value(), id, graph),
            IRComponent::In(vs) => vs
                .iter()
                .try_for_each(|v| register_value(v, ShaclVocab::sh_in(), id, graph)),
            IRComponent::RootClass(cs) => iris(cs, ShaclVocab::sh_root_class(), graph),
            IRComponent::UniqueValuesFor(ps) => iris(ps, ShaclVocab::sh_unique_values_for(), graph),
            IRComponent::QualifiedValueShape(qvs) => {
                if let Some(value) = qvs.qualified_min_count() {
                    register_integer(value, ShaclVocab::sh_qualified_min_count(), id, graph)?;
                }
                if let Some(value) = qvs.qualified_max_count() {
                    register_integer(value, ShaclVocab::sh_qualified_max_count(), id, graph)?;
                }
                if let Some(value) = qvs.qualified_value_shapes_disjoint() {
                    register_boolean(value, ShaclVocab::sh_qualified_value_shapes_disjoint(), id, graph)?;
                }
                shapes(&[*qvs.shape()], ShaclVocab::sh_qualified_value_shape(), graph)
            },
            IRComponent::Closed(closed) => {
                if closed.by_types().is_some() {
                    register_iri(&ShaclVocab::sh_by_types(), ShaclVocab::sh_closed(), id, graph)?;
                } else {
                    register_boolean(closed.is_closed(), ShaclVocab::sh_closed(), id, graph)?;
                }
                iris(closed.ignored_properties(), ShaclVocab::sh_ignored_properties(), graph)
            },
            IRComponent::Deactivated(d) => register_boolean(*d, ShaclVocab::sh_deactivated(), id, graph),
            IRComponent::BasicSparql(sparql) => {
                let bn = graph
                    .add_bnode()
                    .map_err(|e| IRError::from_rdf_err::<RDF>("add_bnode for sh:sparql", e))?;
                let bn_subj: RDF::Subject = bn.into();
                let bn_term: RDF::Term = bn_subj.clone().into();
                let bn_obj = RDF::term_as_object(&bn_term)?;
                register_literal(
                    &ConcreteLiteral::str(sparql.select()),
                    ShaclVocab::sh_select(),
                    &bn_obj,
                    graph,
                )?;
                if let Some(message) = sparql.message() {
                    message
                        .iter_literals()
                        .try_for_each(|lit| register_literal(&lit, ShaclVocab::sh_message(), &bn_obj, graph))?;
                }
                if let Some(deactivated) = sparql.deactivated() {
                    register_boolean(deactivated, ShaclVocab::sh_deactivated(), &bn_obj, graph)?;
                }
                if let Some(prefixes) = sparql.prefixes() {
                    prefixes
                        .iter()
                        .try_for_each(|(_, iri)| register_iri(iri, ShaclVocab::sh_prefixes(), &bn_obj, graph))?;
                }
                register_term(&bn_term, ShaclVocab::sh_sparql(), id, graph)
            },
        }
    }
}

fn node_kind_iri(nk: &NodeKind) -> IriS {
    match nk {
        NodeKind::Iri => ShaclVocab::sh_iri(),
        NodeKind::Lit => ShaclVocab::sh_literal(),
        NodeKind::BNode => ShaclVocab::sh_blank_node(),
        NodeKind::BNodeOrIri => ShaclVocab::sh_blank_node_or_iri(),
        NodeKind::BNodeOrLit => ShaclVocab::sh_blank_node_or_literal(),
        NodeKind::IriOrLit => ShaclVocab::sh_iri_or_literal(),
        NodeKind::TripleTerm => ShaclVocab::sh_triple_term(),
    }
}

fn register_value<RDF: BuildRDF>(
    value: &Object,
    predicate: IriS,
    node: &Object,
    graph: &mut RDF,
) -> Result<(), IRError> {
    match value {
        Object::Iri(iri) => register_iri(iri, predicate, node, graph),
        Object::Literal(lit) => register_literal(lit, predicate, node, graph),
        other => Err(IRError::UnexpectedValueTerm(Box::new(other.clone()))),
    }
}

fn register_integer<RDF: BuildRDF>(
    value: isize,
    predicate: IriS,
    node: &Object,
    graph: &mut RDF,
) -> Result<(), IRError> {
    // isize -> i128 is always a widening (lossless) cast.
    let value = value as i128;
    let literal: RDF::Literal = value.into();
    register_term(&literal.into(), predicate, node, graph)
}

fn register_boolean<RDF: BuildRDF>(
    value: bool,
    predicate: IriS,
    node: &Object,
    graph: &mut RDF,
) -> Result<(), IRError> {
    let literal: RDF::Literal = value.into();
    register_term(&literal.into(), predicate, node, graph)
}

fn register_literal<RDF: BuildRDF>(
    value: &ConcreteLiteral,
    predicate: IriS,
    node: &Object,
    graph: &mut RDF,
) -> Result<(), IRError> {
    let literal: RDF::Literal = value.lexical_form().into();
    register_term(&literal.into(), predicate, node, graph)
}

fn register_iri<RDF: BuildRDF>(value: &IriS, predicate: IriS, node: &Object, graph: &mut RDF) -> Result<(), IRError> {
    register_term(&value.clone().into(), predicate, node, graph)
}

fn register_term<RDF: BuildRDF>(
    value: &RDF::Term,
    predicate: IriS,
    node: &Object,
    graph: &mut RDF,
) -> Result<(), IRError> {
    let subject: RDF::Subject = node
        .clone()
        .try_into()
        .map_err(|_| IRError::InvalidShapeId(Box::new(node.clone())))?;
    graph
        .add_triple(subject, predicate, value.clone())
        .map_err(|e| IRError::from_rdf_err::<RDF>("add triple", e))
}

impl From<&IRComponent> for IriS {
    fn from(value: &IRComponent) -> Self {
        match value {
            IRComponent::Class(_) => ShaclVocab::sh_class_constraint_component(),
            IRComponent::Datatype(_) => ShaclVocab::sh_datatype_constraint_component(),
            IRComponent::NodeKind(_) => ShaclVocab::sh_node_kind_constraint_component(),
            IRComponent::MinCount(_) => ShaclVocab::sh_min_count_constraint_component(),
            IRComponent::MaxCount(_) => ShaclVocab::sh_max_count_constraint_component(),
            IRComponent::MinExclusive(_) => ShaclVocab::sh_min_exclusive_constraint_component(),
            IRComponent::MaxExclusive(_) => ShaclVocab::sh_max_exclusive_constraint_component(),
            IRComponent::MinInclusive(_) => ShaclVocab::sh_min_inclusive_constraint_component(),
            IRComponent::MaxInclusive(_) => ShaclVocab::sh_max_inclusive_constraint_component(),
            IRComponent::MinLength(_) => ShaclVocab::sh_min_length_constraint_component(),
            IRComponent::MaxLength(_) => ShaclVocab::sh_max_length_constraint_component(),
            IRComponent::Pattern(_) => ShaclVocab::sh_pattern_constraint_component(),
            IRComponent::SingleLine(_) => ShaclVocab::sh_single_line_constraint_component(),
            IRComponent::UniqueLang(_) => ShaclVocab::sh_unique_lang_constraint_component(),
            IRComponent::LanguageIn(_) => ShaclVocab::sh_language_in_constraint_component(),
            IRComponent::MemberShape(_) => ShaclVocab::sh_member_shape_constraint_component(),
            IRComponent::MinListLength(_) => ShaclVocab::sh_min_list_length_constraint_component(),
            IRComponent::MaxListLength(_) => ShaclVocab::sh_max_list_length_constraint_component(),
            IRComponent::UniqueMembers(_) => ShaclVocab::sh_unique_members_constraint_component(),
            IRComponent::Equals(_) => ShaclVocab::sh_equals_constraint_component(),
            IRComponent::Disjoint(_) => ShaclVocab::sh_disjoint_constraint_component(),
            IRComponent::SubsetOf(_) => ShaclVocab::sh_subset_of_constraint_component(),
            IRComponent::LessThan(_) => ShaclVocab::sh_less_than_constraint_component(),
            IRComponent::LessThanOrEquals(_) => ShaclVocab::sh_less_than_or_equals_constraint_component(),
            IRComponent::Or(_) => ShaclVocab::sh_or_constraint_component(),
            IRComponent::And(_) => ShaclVocab::sh_and_constraint_component(),
            IRComponent::Not(_) => ShaclVocab::sh_not_constraint_component(),
            IRComponent::Xone(_) => ShaclVocab::sh_xone_constraint_component(),
            IRComponent::If(_) => ShaclVocab::sh_if_constraint_component(),
            IRComponent::Node(_) => ShaclVocab::sh_node_constraint_component(),
            IRComponent::NodeByExpression(_) => ShaclVocab::sh_node_by_expression_constraint_component(),
            IRComponent::SomeValue(_) => ShaclVocab::sh_some_value_constraint_component(),
            IRComponent::HasValue(_) => ShaclVocab::sh_has_value_constraint_component(),
            IRComponent::In(_) => ShaclVocab::sh_in_constraint_component(),
            IRComponent::RootClass(_) => ShaclVocab::sh_root_class_constraint_component(),
            IRComponent::UniqueValuesFor(_) => ShaclVocab::sh_unique_values_for_constraint_component(),
            IRComponent::QualifiedValueShape(_) => ShaclVocab::sh_qualified_value_shape_constraint_component(),
            IRComponent::Closed(_) => ShaclVocab::sh_closed_constraint_component(),
            IRComponent::Deactivated(_) => ShaclVocab::sh_deactivated_constraint_component(),
            IRComponent::BasicSparql(_) => ShaclVocab::sh_sparql_constraint_component(),
        }
    }
}

impl Display for IRComponent {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        fn list<T: Display>(xs: &[T]) -> String {
            xs.iter().map(|x| x.to_string()).join(", ")
        }
        match self {
            IRComponent::Class(cs) => write!(f, "Class[{}]", list(cs)),
            IRComponent::Datatype(ds) => write!(f, "Datatype[{}]", list(ds)),
            IRComponent::NodeKind(nks) => write!(f, "NodeKind[{}]", list(nks)),
            IRComponent::MinCount(n) => write!(f, "MinCount({n})"),
            IRComponent::MaxCount(n) => write!(f, "MaxCount({n})"),
            IRComponent::MinExclusive(n) => write!(f, "MinExclusive({n})"),
            IRComponent::MaxExclusive(n) => write!(f, "MaxExclusive({n})"),
            IRComponent::MinInclusive(n) => write!(f, "MinInclusive({n})"),
            IRComponent::MaxInclusive(n) => write!(f, "MaxInclusive({n})"),
            IRComponent::MinLength(n) => write!(f, "MinLength({n})"),
            IRComponent::MaxLength(n) => write!(f, "MaxLength({n})"),
            IRComponent::Pattern(p) => write!(f, "{p}"),
            IRComponent::SingleLine(b) => write!(f, "SingleLine({b})"),
            IRComponent::UniqueLang(b) => write!(f, "UniqueLang({b})"),
            IRComponent::LanguageIn(langs) => write!(f, "LanguageIn[{}]", list(langs)),
            IRComponent::MemberShape(s) => write!(f, "MemberShape({s})"),
            IRComponent::MinListLength(n) => write!(f, "MinListLength({n})"),
            IRComponent::MaxListLength(n) => write!(f, "MaxListLength({n})"),
            IRComponent::UniqueMembers(b) => write!(f, "UniqueMembers({b})"),
            IRComponent::Equals(p) => write!(f, "Equals({p})"),
            IRComponent::Disjoint(p) => write!(f, "Disjoint({p})"),
            IRComponent::SubsetOf(p) => write!(f, "SubsetOf({p})"),
            IRComponent::LessThan(p) => write!(f, "LessThan({p})"),
            IRComponent::LessThanOrEquals(p) => write!(f, "LessThanOrEquals({p})"),
            IRComponent::Or(ss) => write!(f, "Or[{}]", list(ss)),
            IRComponent::And(ss) => write!(f, "And[{}]", list(ss)),
            IRComponent::Not(s) => write!(f, "Not({s})"),
            IRComponent::Xone(ss) => write!(f, "Xone[{}]", list(ss)),
            IRComponent::If(if_) => write!(f, "{if_}"),
            IRComponent::Node(s) => write!(f, "Node({s})"),
            IRComponent::NodeByExpression(s) => write!(f, "NodeByExpression({s})"),
            IRComponent::SomeValue(s) => write!(f, "SomeValue({s})"),
            IRComponent::HasValue(v) => write!(f, "HasValue({v})"),
            IRComponent::In(vs) => write!(f, "In[{}]", list(vs)),
            IRComponent::RootClass(cs) => write!(f, "RootClass[{}]", list(cs)),
            IRComponent::UniqueValuesFor(ps) => write!(f, "UniqueValuesFor[{}]", list(ps)),
            IRComponent::QualifiedValueShape(qvs) => write!(f, "{qvs}"),
            IRComponent::Closed(closed) => write!(f, "{closed}"),
            IRComponent::Deactivated(d) => write!(f, "Deactivated({d})"),
            IRComponent::BasicSparql(sparql) => write!(f, "{sparql}"),
        }
    }
}

/// Compiles a SHACL cardinality (`sh:minCount` / `sh:maxCount`) value, rejecting
/// a negative `isize` instead of silently wrapping it.
fn check_non_negative(component: &'static str, value: isize) -> Result<isize, IRError> {
    if value < 0 {
        Err(IRError::NegativeCardinality { component, value })
    } else {
        Ok(value)
    }
}
