use crate::ast::{ASTComponent, ASTSchema};
use crate::ir::components::{And, BasicSparql, Closed, Node, Not, Or, Pattern, QualifiedValueShape, Xone};
use crate::ir::dg::{DependencyGraph, PosNeg};
use crate::ir::error::IRError;
use crate::ir::schema::IRSchema;
use crate::ir::shape::IRShape;
use crate::ir::shape_label_idx::ShapeLabelIdx;
use crate::ir::visitor::IRComponentVisitor;
use crate::ir::{convert_iri_ref, convert_value};
use crate::types::NodeKind;
use itertools::Itertools;
use rudof_iri::IriS;
use rudof_rdf::BuildRDF;
use rudof_rdf::term::Object;
use rudof_rdf::term::literal::{ConcreteLiteral, Lang};
use rudof_rdf::vocab::ShaclVocab;
use std::collections::{HashMap, HashSet};
use std::fmt::{Display, Formatter};

/// A compiled SHACL constraint component.
///
/// Trivial components are stored INLINE as their scalar/term payload (the
/// newtype-collapse: `MinCount(isize)`, `Datatype(IriS)`, …). Only the
/// components that carry genuine compiled state keep a struct payload
/// (`Pattern`, `Closed`, `BasicSparql`, `QualifiedValueShape`, and the
/// logical/shape-based ones interning a [`ShapeLabelIdx`]). See
/// [`crate::ir::components`].
#[derive(Debug, Clone)]
pub enum IRComponent {
    Class(Object),
    Datatype(IriS),
    NodeKind(NodeKind),
    MinCount(isize),
    MaxCount(isize),
    MinExclusive(ConcreteLiteral),
    MaxExclusive(ConcreteLiteral),
    MinInclusive(ConcreteLiteral),
    MaxInclusive(ConcreteLiteral),
    MinLength(isize),
    MaxLength(isize),
    Pattern(Pattern),
    UniqueLang(bool),
    LanguageIn(Vec<Lang>),
    Equals(IriS),
    Disjoint(IriS),
    LessThan(IriS),
    LessThanOrEquals(IriS),
    Or(Or),
    And(And),
    Not(Not),
    Xone(Xone),
    Node(Node),
    HasValue(Object),
    In(Vec<Object>),
    QualifiedValueShape(QualifiedValueShape),
    Closed(Closed),
    Deactivated(bool),
    BasicSparql(BasicSparql),
}

impl IRComponent {
    /// Compiles an AST SHACL component to an IR SHACL Component
    /// It returns None for components that are not represented in the IR,
    /// such as sh:closed and sh:deactivated.
    /// It returns a vector of (PosNeg, ShapeLabelIdx) pairs for components that are represented in the IR.
    /// The vector is list of dependant shapes for cases with recursion
    // TODO - Update comment to match current behaviour
    pub fn compile(component: &ASTComponent, ast: &ASTSchema, ir: &mut IRSchema) -> Result<Self, IRError> {
        let result = match component.clone() {
            ASTComponent::Class(object) => IRComponent::Class(object),
            ASTComponent::Datatype(iri) => IRComponent::Datatype(convert_iri_ref(iri)?),
            ASTComponent::NodeKind(nk) => IRComponent::NodeKind(nk),
            ASTComponent::MinCount(n) => IRComponent::MinCount(check_non_negative("sh:minCount", n)?),
            ASTComponent::MaxCount(n) => IRComponent::MaxCount(check_non_negative("sh:maxCount", n)?),
            ASTComponent::MinExclusive(lit) => IRComponent::MinExclusive(lit),
            ASTComponent::MaxExclusive(lit) => IRComponent::MaxExclusive(lit),
            ASTComponent::MinInclusive(lit) => IRComponent::MinInclusive(lit),
            ASTComponent::MaxInclusive(lit) => IRComponent::MaxInclusive(lit),
            ASTComponent::MinLength(l) => IRComponent::MinLength(l),
            ASTComponent::MaxLength(l) => IRComponent::MaxLength(l),
            ASTComponent::Pattern { pattern, flags } => {
                let pattern = Pattern::new(pattern, flags)?;
                IRComponent::Pattern(pattern)
            },
            ASTComponent::UniqueLang(lang) => IRComponent::UniqueLang(lang),
            ASTComponent::LanguageIn(langs) => IRComponent::LanguageIn(langs),
            ASTComponent::Equals(iri) => IRComponent::Equals(convert_iri_ref(iri)?),
            ASTComponent::Disjoint(iri) => IRComponent::Disjoint(convert_iri_ref(iri)?),
            ASTComponent::LessThan(iri) => IRComponent::LessThan(convert_iri_ref(iri)?),
            ASTComponent::LessThanOrEquals(iri) => IRComponent::LessThanOrEquals(convert_iri_ref(iri)?),
            ASTComponent::Or(objs) => {
                let idxs = ir.register_shapes(objs, ast)?;
                IRComponent::Or(Or::new(idxs))
            },
            ASTComponent::And(objs) => {
                let idxs = ir.register_shapes(objs, ast)?;
                IRComponent::And(And::new(idxs))
            },
            ASTComponent::Not(obj) => {
                let idx = ir.register_shape(&obj, None, ast)?;
                IRComponent::Not(Not::new(idx))
            },
            ASTComponent::Xone(objs) => {
                let idxs = ir.register_shapes(objs, ast)?;
                IRComponent::Xone(Xone::new(idxs))
            },
            ASTComponent::Closed {
                is_closed,
                ignored_properties,
            } => IRComponent::Closed(Closed::new(is_closed, ignored_properties.into_iter().collect_vec())),
            ASTComponent::Node(obj) => {
                let idx = ir.register_shape(&obj, None, ast)?;
                IRComponent::Node(Node::new(idx))
            },
            ASTComponent::HasValue(val) => {
                let term = convert_value(val)?;
                IRComponent::HasValue(term)
            },
            ASTComponent::In(vals) => {
                let terms = vals.into_iter().map(convert_value).collect::<Result<Vec<_>, _>>()?;
                IRComponent::In(terms)
            },
            ASTComponent::QualifiedValueShape {
                shape,
                q_min_count,
                q_max_count,
                disjoint,
                siblings,
            } => {
                let idx = ir.register_shape(&shape, None, ast)?;
                let compiled_siblings = ir.register_shapes(siblings, ast)?;

                IRComponent::QualifiedValueShape(QualifiedValueShape::new(
                    idx,
                    q_min_count,
                    q_max_count,
                    disjoint,
                    compiled_siblings,
                ))
            },
            ASTComponent::Deactivated(d) => {
                // TODO - Change for node expr
                IRComponent::Deactivated(d)
            },
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
}

impl IRComponent {
    pub fn register<RDF: BuildRDF>(
        &self,
        id: &Object,
        graph: &mut RDF,
        shape_map: &HashMap<ShapeLabelIdx, IRShape>,
    ) -> Result<(), IRError> {
        match self {
            IRComponent::Class(c) => register_term(&c.clone().into(), ShaclVocab::sh_class(), id, graph),
            IRComponent::Datatype(iri) => register_iri(iri, ShaclVocab::sh_datatype(), id, graph),
            IRComponent::NodeKind(nk) => {
                let iri = match nk {
                    NodeKind::Iri => ShaclVocab::sh_iri_ref(),
                    NodeKind::Lit => ShaclVocab::sh_literal_ref(),
                    NodeKind::BNode => ShaclVocab::sh_blank_node_ref(),
                    NodeKind::BNodeOrIri => ShaclVocab::sh_blank_node_or_iri_ref(),
                    NodeKind::BNodeOrLit => ShaclVocab::sh_blank_node_or_literal_ref(),
                    NodeKind::IriOrLit => ShaclVocab::sh_iri_or_literal_ref(),
                };
                register_iri(iri, ShaclVocab::sh_node_kind(), id, graph)
            },
            IRComponent::MinCount(mc) => register_integer(*mc, ShaclVocab::sh_min_count(), id, graph),
            IRComponent::MaxCount(mc) => register_integer(*mc, ShaclVocab::sh_max_count(), id, graph),
            IRComponent::MinExclusive(me) => register_literal(me, ShaclVocab::sh_min_exclusive(), id, graph),
            IRComponent::MaxExclusive(me) => register_literal(me, ShaclVocab::sh_max_exclusive(), id, graph),
            IRComponent::MinInclusive(mi) => register_literal(mi, ShaclVocab::sh_min_inclusive(), id, graph),
            IRComponent::MaxInclusive(mi) => register_literal(mi, ShaclVocab::sh_max_inclusive(), id, graph),
            IRComponent::MinLength(ml) => register_integer(*ml, ShaclVocab::sh_min_length(), id, graph),
            IRComponent::MaxLength(ml) => register_integer(*ml, ShaclVocab::sh_max_length(), id, graph),
            IRComponent::Pattern(p) => {
                if let Some(flags) = p.flags() {
                    register_literal(&ConcreteLiteral::str(flags), ShaclVocab::sh_flags(), id, graph)?;
                }
                register_literal(&ConcreteLiteral::str(p.pattern()), ShaclVocab::sh_pattern(), id, graph)
            },
            IRComponent::UniqueLang(ul) => register_boolean(*ul, ShaclVocab::sh_unique_lang(), id, graph),
            IRComponent::LanguageIn(langs) => langs.iter().try_for_each(|l| {
                register_literal(
                    &ConcreteLiteral::str(&l.to_string()),
                    ShaclVocab::sh_language_in(),
                    id,
                    graph,
                )
            }),
            IRComponent::Equals(eq) => register_iri(eq, ShaclVocab::sh_equals(), id, graph),
            IRComponent::Disjoint(d) => register_iri(d, ShaclVocab::sh_disjoint(), id, graph),
            IRComponent::LessThan(lt) => register_iri(lt, ShaclVocab::sh_less_than(), id, graph),
            IRComponent::LessThanOrEquals(lte) => register_iri(lte, ShaclVocab::sh_less_than_or_equals(), id, graph),
            IRComponent::Or(or) => or.shapes().iter().try_for_each(|idx| {
                let shape = shape_map.get(idx).ok_or(IRError::ShapeNotFound(*idx))?;
                register_term(&shape.id().clone().into(), ShaclVocab::sh_or(), id, graph)
            }),
            IRComponent::And(and) => and.shapes().iter().try_for_each(|idx| {
                let shape = shape_map.get(idx).ok_or(IRError::ShapeNotFound(*idx))?;
                register_term(&shape.id().clone().into(), ShaclVocab::sh_and(), id, graph)
            }),
            IRComponent::Not(not) => {
                let shape = shape_map.get(not.shape()).ok_or(IRError::ShapeNotFound(*not.shape()))?;
                register_term(&shape.id().clone().into(), ShaclVocab::sh_not(), id, graph)
            },
            IRComponent::Xone(xone) => xone.shapes().iter().try_for_each(|idx| {
                let shape = shape_map.get(idx).ok_or(IRError::ShapeNotFound(*idx))?;
                register_term(&shape.id().clone().into(), ShaclVocab::sh_xone(), id, graph)
            }),
            IRComponent::Node(n) => {
                let shape = shape_map.get(n.shape()).ok_or(IRError::ShapeNotFound(*n.shape()))?;
                register_term(&shape.id().clone().into(), ShaclVocab::sh_node(), id, graph)
            },
            IRComponent::HasValue(hv) => match hv {
                Object::Iri(iri) => register_iri(iri, ShaclVocab::sh_has_value(), id, graph),
                Object::Literal(lit) => register_literal(lit, ShaclVocab::sh_has_value(), id, graph),
                other => Err(IRError::UnexpectedValueTerm(Box::new(other.clone()))),
            },
            IRComponent::In(values) => values.iter().try_for_each(|v| match v {
                Object::Iri(iri) => register_iri(iri, ShaclVocab::sh_in(), id, graph),
                Object::Literal(lit) => register_literal(lit, ShaclVocab::sh_in(), id, graph),
                other => Err(IRError::UnexpectedValueTerm(Box::new(other.clone()))),
            }),
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

                let idx = qvs.shape();
                let shape = shape_map.get(idx).ok_or(IRError::ShapeNotFound(*idx))?;

                register_term(
                    &shape.id().clone().into(),
                    ShaclVocab::sh_qualified_value_shape(),
                    id,
                    graph,
                )
            },
            IRComponent::Closed(closed) => {
                register_boolean(closed.is_closed(), ShaclVocab::sh_closed(), id, graph)?;

                closed
                    .ignored_properties()
                    .iter()
                    .try_for_each(|iri| register_iri(iri, ShaclVocab::sh_ignored_properties(), id, graph))
            },
            IRComponent::Deactivated(deactivated) => {
                // TODO - Adapt for node expression
                register_boolean(*deactivated, ShaclVocab::sh_deactivated(), id, graph)
            },
            IRComponent::BasicSparql(sparql) => {
                // Create a blank node to hold the constraint properties
                let bn = graph
                    .add_bnode()
                    .map_err(|e| IRError::from_rdf_err::<RDF>("add_bnode for sh:sparql", e))?;
                let bn_subj: RDF::Subject = bn.into();
                let bn_term: RDF::Term = bn_subj.clone().into();
                let bn_obj = RDF::term_as_object(&bn_term)?;

                // Register bnode stuff
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

                // Register sh:sparql bnode
                register_term(&bn_term, ShaclVocab::sh_sparql(), id, graph)
            },
        }
    }
}

impl IRComponent {
    pub fn add_edges(
        &self,
        idx: ShapeLabelIdx,
        dg: &mut DependencyGraph,
        posneg: PosNeg,
        ir: &IRSchema,
        cache: &mut HashSet<ShapeLabelIdx>,
    ) {
        // Only the logical/shape-based components contribute dependency edges;
        // every other arm falls through to `default_component` (a no-op). This
        // is the canonical use of [`IRComponentVisitor`]: a handful of overrides
        // over a no-op default.
        let mut visitor = AddEdgesVisitor {
            idx,
            dg,
            posneg,
            ir,
            cache,
        };
        let Ok(()) = self.accept(&mut visitor);
    }
}

/// Walks the interned shape indices of the logical/shape-based components to
/// build the dependency-graph edges. See [`IRComponent::add_edges`].
struct AddEdgesVisitor<'a> {
    idx: ShapeLabelIdx,
    dg: &'a mut DependencyGraph,
    posneg: PosNeg,
    ir: &'a IRSchema,
    cache: &'a mut HashSet<ShapeLabelIdx>,
}

impl AddEdgesVisitor<'_> {
    /// Edge + recursion shared by `sh:or` / `sh:and` / `sh:xone` (positive
    /// polarity) and `sh:node` (single child, same polarity).
    fn walk(&mut self, shape_idx: &ShapeLabelIdx, posneg: PosNeg) {
        if let Some(shape) = self.ir.get_shape_from_idx(shape_idx) {
            self.dg.add_edge(self.idx, *shape_idx, posneg);
            if self.cache.contains(shape_idx) {
                return;
            }
            self.cache.insert(*shape_idx);
            shape.add_edges(*shape_idx, self.dg, posneg, self.ir, self.cache);
        }
    }
}

impl IRComponentVisitor for AddEdgesVisitor<'_> {
    type Output = ();
    type Error = std::convert::Infallible;

    fn default_component(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn visit_or(&mut self, shapes: &[ShapeLabelIdx]) -> Result<(), Self::Error> {
        for shape_idx in shapes {
            self.walk(shape_idx, self.posneg);
        }
        Ok(())
    }

    fn visit_and(&mut self, shapes: &[ShapeLabelIdx]) -> Result<(), Self::Error> {
        for shape_idx in shapes {
            self.walk(shape_idx, self.posneg);
        }
        Ok(())
    }

    fn visit_xone(&mut self, shapes: &[ShapeLabelIdx]) -> Result<(), Self::Error> {
        for shape_idx in shapes {
            self.walk(shape_idx, self.posneg);
        }
        Ok(())
    }

    fn visit_not(&mut self, shape: ShapeLabelIdx) -> Result<(), Self::Error> {
        // `sh:not` flips polarity for both the edge and the recursion.
        self.walk(&shape, self.posneg.change());
        Ok(())
    }

    fn visit_node(&mut self, shape: ShapeLabelIdx) -> Result<(), Self::Error> {
        self.walk(&shape, self.posneg);
        Ok(())
    }

    fn visit_qualified_value_shape(&mut self, qvs: &QualifiedValueShape) -> Result<(), Self::Error> {
        self.dg.add_edge(self.idx, *qvs.shape(), self.posneg);
        // Siblings intentionally do not add edges (preserved from the prior impl).
        Ok(())
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
            IRComponent::UniqueLang(_) => ShaclVocab::sh_unique_lang_constraint_component(),
            IRComponent::LanguageIn(_) => ShaclVocab::sh_language_in_constraint_component(),
            IRComponent::Equals(_) => ShaclVocab::sh_equals_constraint_component(),
            IRComponent::Disjoint(_) => ShaclVocab::sh_disjoint_constraint_component(),
            IRComponent::LessThan(_) => ShaclVocab::sh_less_than_constraint_component(),
            IRComponent::LessThanOrEquals(_) => ShaclVocab::sh_less_than_or_equals_constraint_component(),
            IRComponent::Or(_) => ShaclVocab::sh_or_constraint_component(),
            IRComponent::And(_) => ShaclVocab::sh_and_constraint_component(),
            IRComponent::Not(_) => ShaclVocab::sh_not_constraint_component(),
            IRComponent::Xone(_) => ShaclVocab::sh_xone_constraint_component(),
            IRComponent::Node(_) => ShaclVocab::sh_node_constraint_component(),
            IRComponent::HasValue(_) => ShaclVocab::sh_has_value_constraint_component(),
            IRComponent::In(_) => ShaclVocab::sh_in_constraint_component(),
            IRComponent::QualifiedValueShape(_) => ShaclVocab::sh_qualified_value_shape_constraint_component(),
            IRComponent::Closed(_) => ShaclVocab::sh_closed_constraint_component(),
            IRComponent::Deactivated(_) => ShaclVocab::sh_deactivated_constraint_component(),
            IRComponent::BasicSparql(_) => ShaclVocab::sh_sparql_constraint_component(),
        }
    }
}

impl Display for IRComponent {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            IRComponent::Class(cls) => write!(f, " Class: {cls}"),
            IRComponent::Datatype(dt) => write!(f, " Datatype: {dt}"),
            IRComponent::NodeKind(nk) => write!(f, " NodeKind: {nk:?}"),
            IRComponent::MinCount(n) => write!(f, " MinCount: {n}"),
            IRComponent::MaxCount(mn) => write!(f, " MaxCount: {mn}"),
            IRComponent::MinExclusive(n) => write!(f, " MinExclusive: {n}"),
            IRComponent::MaxExclusive(n) => write!(f, " MaxExclusive: {n}"),
            IRComponent::MinInclusive(n) => write!(f, " MinInclusive: {n}"),
            IRComponent::MaxInclusive(n) => write!(f, " MaxInclusive: {n}"),
            IRComponent::MinLength(n) => write!(f, " MinLength: {n}"),
            IRComponent::MaxLength(n) => write!(f, " MaxLength: {n}"),
            IRComponent::Pattern(pt) => write!(f, " {pt}"),
            IRComponent::UniqueLang(ul) => write!(f, " UniqueLang: {ul}"),
            IRComponent::LanguageIn(langs) => {
                let langs = langs.iter().map(|l| l.to_string()).collect::<Vec<_>>().join(", ");
                write!(f, " LanguageIn: [{langs}]")
            },
            IRComponent::Equals(e) => write!(f, " Equals: {e}"),
            IRComponent::Disjoint(p) => write!(f, " Disjoint: {p}"),
            IRComponent::LessThan(p) => write!(f, " LessThan: {p}"),
            IRComponent::LessThanOrEquals(p) => write!(f, " LessThanOrEquals: {p}"),
            IRComponent::Or(or) => write!(f, " {or}"),
            IRComponent::And(and) => write!(f, " {and}"),
            IRComponent::Not(not) => write!(f, " {not}"),
            IRComponent::Xone(xone) => write!(f, " {xone}"),
            IRComponent::Node(n) => write!(f, " {n}"),
            IRComponent::HasValue(v) => write!(f, " HasValue(HasValue: {v})"),
            IRComponent::In(values) => {
                let values = values.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(", ");
                write!(f, " In[{values}]")
            },
            IRComponent::QualifiedValueShape(qvs) => write!(f, " {qvs}"),
            IRComponent::Closed(closed) => write!(f, "{closed}"),
            IRComponent::Deactivated(deactivated) => write!(f, "Deactivated: {deactivated}"),
            IRComponent::BasicSparql(sparql) => write!(f, " {sparql}"),
        }
    }
}

/// Compiles a SHACL cardinality (`sh:minCount` / `sh:maxCount`) value, rejecting
/// a negative `isize` instead of silently wrapping it. Mirrors the guard the
/// collapsed `MinCount`/`MaxCount` newtypes used to enforce in their `new`.
fn check_non_negative(component: &'static str, value: isize) -> Result<isize, IRError> {
    if value < 0 {
        Err(IRError::NegativeCardinality { component, value })
    } else {
        Ok(value)
    }
}
