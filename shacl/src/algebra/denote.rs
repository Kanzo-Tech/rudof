//! The denotation of a shapes graph: SHACL Core as relational algebra.
//!
//! This is the one place that knows what SHACL means. Each shape, target,
//! path and constraint component becomes [`Op`]s of a [`Plan`]; nothing here
//! knows how a plan is executed.
//!
//! Two questions are asked of a shape:
//!
//! - **Its results** for a focus relation ([`Denoter::emit`]): the rows of each
//!   of its components, then those of its property shapes, whose focus nodes
//!   are this shape's value nodes. A property shape nested under a property
//!   shape is reached once per path that reaches a node, so its focus relation
//!   is a bag: its rows are computed on the distinct nodes and repeated per
//!   occurrence ([`Op::Repeat`]).
//! - **Which candidates fail it** ([`Denoter::fails`]): the nodes of a
//!   candidate relation that the shape reports any result for, of any
//!   severity. This is conformance as `sh:node`, `sh:and`, `sh:or`, `sh:xone`,
//!   `sh:not`, `sh:if` and the qualified value shapes test it. A deactivated
//!   shape fails nothing.
//!
//! The dependency graph of the shapes is acyclic (recursive shapes graphs are
//! refused: SHACL leaves their semantics undefined), so `fails` recurses into
//! strictly lower strata and terminates.

use crate::algebra::{Check, CmpOp, Expr, Kind, Op, Parameters, Plan, PlanBuilder, Pred, RelId, Sort, SortError};
use crate::ir::components::{BasicSparql, Closed, If, Pattern, QualifiedValueShape};
use crate::ir::visitor::IRComponentVisitor;
use crate::ir::{IRComponent, IRSchema, IRShape, ShapeLabelIdx};
use crate::types::{NodeKind, Target};
use rudof_iri::IriS;
use rudof_rdf::SHACLPath;
use rudof_rdf::term::Object;
use rudof_rdf::term::literal::{ConcreteLiteral, Lang};
use rudof_rdf::vocab::ShaclVocab;
use std::collections::HashMap;

/// Why a shapes graph has no denotation here.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DenoteError {
    /// A feature outside the algebra, named.
    #[error("not supported: {0}")]
    Unsupported(String),
    /// The shapes refer to themselves; SHACL leaves their semantics undefined.
    #[error("recursive shapes are refused (their SHACL semantics is undefined): {0}")]
    RecursiveShapes(String),
    #[error("malformed target: {0}")]
    MalformedTarget(String),
    #[error("internal error of the denotation: {0}")]
    Internal(String),
}

impl From<SortError> for DenoteError {
    fn from(e: SortError) -> Self {
        DenoteError::Internal(e.to_string())
    }
}

/// The SHACL 1.2 and SHACL-SPARQL features of `shape` outside the algebra.
fn refuse(shape: &IRShape) -> Result<(), DenoteError> {
    if shape.reifier_info().is_some() {
        return Err(DenoteError::Unsupported(format!(
            "sh:reifierShape (SHACL 1.2) on {}",
            shape.id()
        )));
    }
    if shape
        .components()
        .iter()
        .any(|c| matches!(c, IRComponent::BasicSparql(_)))
    {
        return Err(DenoteError::Unsupported(format!(
            "sh:sparql (SHACL-SPARQL is outside SHACL Core) on {}",
            shape.id()
        )));
    }
    Ok(())
}

/// The plan of `schema`: one check per constraint component of each shape in
/// each context it is reached in.
///
/// Shapes are denoted in the order of the dependency graph's levels, a shape
/// after the shapes it refers to. A shape with targets yields the checks of
/// its components and of its property shapes; a shape without targets is
/// reached only through another (as a property shape, or by `sh:node`,
/// `sh:and`, …). Deactivated shapes yield nothing and conform everywhere.
pub fn denote(schema: &IRSchema) -> Result<Plan, DenoteError> {
    let mut d = Denoter::new(schema)?;
    for level in schema.shapes_with_targets_by_level() {
        for idx in level {
            let shape = d.shape(idx)?;
            if !shape.deactivated() {
                let focus = d.focus(shape)?;
                d.emit(idx, focus, false)?;
            }
        }
    }
    Ok(d.b.finish())
}

/// The plan of one shape and its property shapes: for `focus` when given,
/// otherwise for the shape's own targets. The checks a form revalidates when
/// one node or one shape changes.
pub fn denote_shape(schema: &IRSchema, idx: ShapeLabelIdx, focus: Option<&Object>) -> Result<Plan, DenoteError> {
    let mut d = Denoter::new(schema)?;
    let shape = d.shape(idx)?;
    let focus = match focus {
        Some(node) => d.op(Op::Constants(vec![node.clone()]))?,
        None => d.focus(shape)?,
    };
    d.emit(idx, focus, false)?;
    Ok(d.b.finish())
}

struct Denoter<'a> {
    schema: &'a IRSchema,
    b: PlanBuilder,
    fails: HashMap<(ShapeLabelIdx, RelId), Option<RelId>>,
}

/// Where a path starts.
#[derive(Debug, Clone, Copy)]
enum Start {
    /// The nodes of a node relation.
    Nodes(RelId),
    /// Every node of the data.
    All,
}

/// A path with its inverses pushed down to the predicates.
#[derive(Debug, Clone)]
enum Normal {
    Predicate { iri: IriS, inverse: bool },
    Sequence(Vec<Normal>),
    Alternative(Vec<Normal>),
    ZeroOrMore(Box<Normal>),
    OneOrMore(Box<Normal>),
    ZeroOrOne(Box<Normal>),
}

/// `^(a/b) = ^b/^a`, `^(a|b) = ^a|^b`, `^(p*) = (^p)*`, …: only a predicate is
/// ever inverted.
fn normalise(path: &SHACLPath, inverse: bool) -> Normal {
    match path {
        SHACLPath::Predicate { pred } => Normal::Predicate {
            iri: pred.clone(),
            inverse,
        },
        SHACLPath::Inverse { path } => normalise(path, !inverse),
        SHACLPath::Sequence { paths } => {
            let mut steps: Vec<Normal> = paths.iter().map(|p| normalise(p, inverse)).collect();
            if inverse {
                steps.reverse();
            }
            Normal::Sequence(steps)
        },
        SHACLPath::Alternative { paths } => Normal::Alternative(paths.iter().map(|p| normalise(p, inverse)).collect()),
        SHACLPath::ZeroOrMore { path } => Normal::ZeroOrMore(Box::new(normalise(path, inverse))),
        SHACLPath::OneOrMore { path } => Normal::OneOrMore(Box::new(normalise(path, inverse))),
        SHACLPath::ZeroOrOne { path } => Normal::ZeroOrOne(Box::new(normalise(path, inverse))),
    }
}

/// The rows of one component of a shape, before they become a [`Check`].
struct Rows {
    component: IriS,
    rows: RelId,
    parameters: Option<Vec<(&'static str, String)>>,
}

impl<'a> Denoter<'a> {
    /// A denoter for `schema`, once it is known to have a denotation.
    fn new(schema: &'a IRSchema) -> Result<Self, DenoteError> {
        let graph = schema.dependency_graph();
        if graph.has_cycles() {
            return Err(DenoteError::RecursiveShapes(format!("{graph}")));
        }
        for (_, shape) in schema.iter() {
            refuse(shape)?;
        }
        Ok(Self {
            schema,
            b: PlanBuilder::new(),
            fails: HashMap::new(),
        })
    }

    fn op(&mut self, op: Op) -> Result<RelId, DenoteError> {
        Ok(self.b.add(op)?)
    }

    fn shape(&self, idx: ShapeLabelIdx) -> Result<&'a IRShape, DenoteError> {
        self.schema
            .get_shape_from_idx(&idx)
            .ok_or_else(|| DenoteError::Internal(format!("shape {idx} is not in the schema")))
    }

    fn union(&mut self, rels: Vec<RelId>, sort: Sort) -> Result<RelId, DenoteError> {
        match rels.len() {
            0 => self.op(Op::Empty(sort)),
            1 => Ok(rels[0]),
            _ => self.op(Op::Union(rels)),
        }
    }

    fn distinct_union(&mut self, rels: Vec<RelId>, sort: Sort) -> Result<RelId, DenoteError> {
        let u = self.union(rels, sort)?;
        self.op(Op::Distinct(u))
    }

    // --- targets -------------------------------------------------------------

    /// The focus nodes of `shape`: the union of its targets, distinct.
    fn focus(&mut self, shape: &IRShape) -> Result<RelId, DenoteError> {
        let mut parts = Vec::new();
        for target in shape.targets() {
            parts.push(self.target(target)?);
        }
        self.distinct_union(parts, Sort::Nodes)
    }

    fn target(&mut self, target: &Target) -> Result<RelId, DenoteError> {
        match target {
            Target::Node(node) => {
                if matches!(node, Object::BlankNode(_)) {
                    return Err(DenoteError::MalformedTarget(format!(
                        "sh:targetNode {node} is a blank node"
                    )));
                }
                self.op(Op::Constants(vec![node.clone()]))
            },
            Target::Class(class) | Target::ImplicitClass(class) => match class {
                Object::Iri(iri) => self.op(Op::Class(iri.clone())),
                // Only an IRI names a class.
                _ => self.op(Op::Empty(Sort::Nodes)),
            },
            Target::SubjectsOf(predicate) => {
                let edges = self.op(Op::Predicate(predicate.clone()))?;
                let f = self.op(Op::Focus(edges))?;
                self.op(Op::Distinct(f))
            },
            Target::ObjectsOf(predicate) => {
                let edges = self.op(Op::Predicate(predicate.clone()))?;
                let v = self.op(Op::Values(edges))?;
                self.op(Op::Distinct(v))
            },
            // SHACL 1.2 §3.1.3.6: the nodes of the data graph that conform to
            // the shape.
            Target::Where(shape) => {
                let idx = *self.schema.get_idx(shape).ok_or_else(|| {
                    DenoteError::MalformedTarget(format!("sh:targetWhere value {shape} is not a shape"))
                })?;
                let all = self.op(Op::AllNodes)?;
                let candidates = self.op(Op::Distinct(all))?;
                match self.fails(idx, candidates)? {
                    None => Ok(candidates),
                    Some(fails) => self.op(Op::Filter(candidates, !Pred::Member(Expr::f(), fails))),
                }
            },
            Target::WrongNode(_)
            | Target::WrongClass(_)
            | Target::WrongSubjectsOf(_)
            | Target::WrongObjectsOf(_)
            | Target::WrongImplicitClass(_) => Err(DenoteError::MalformedTarget(format!(
                "{target}: the target value has the wrong term kind"
            ))),
        }
    }

    // --- paths ---------------------------------------------------------------

    /// The distinct `(n, n)` pairs of the start nodes.
    fn identity(&mut self, start: Start) -> Result<RelId, DenoteError> {
        let nodes = match start {
            Start::Nodes(rel) => rel,
            Start::All => self.op(Op::AllNodes)?,
        };
        let distinct = self.op(Op::Distinct(nodes))?;
        self.op(Op::Identity(distinct))
    }

    /// The `(focus, value)` pairs of `path` from `start`, distinct.
    fn path(&mut self, path: &Normal, start: Start) -> Result<RelId, DenoteError> {
        match path {
            Normal::Predicate { iri, inverse } => {
                let mut edges = self.op(Op::Predicate(iri.clone()))?;
                if *inverse {
                    edges = self.op(Op::Inverse(edges))?;
                }
                match start {
                    Start::All => Ok(edges),
                    Start::Nodes(nodes) => self.op(Op::Restrict { pairs: edges, nodes }),
                }
            },
            Normal::Sequence(steps) => {
                let Some((first, rest)) = steps.split_first() else {
                    return self.identity(start);
                };
                let mut acc = self.path(first, start)?;
                for step in rest {
                    let reached = self.op(Op::Values(acc))?;
                    let reached = self.op(Op::Distinct(reached))?;
                    let next = self.path(step, Start::Nodes(reached))?;
                    acc = self.op(Op::Compose(acc, next))?;
                }
                Ok(acc)
            },
            Normal::Alternative(members) => {
                let mut rels = Vec::new();
                for member in members {
                    rels.push(self.path(member, start)?);
                }
                self.distinct_union(rels, Sort::Pairs)
            },
            Normal::ZeroOrOne(inner) => {
                let identity = self.identity(start)?;
                let once = self.path(inner, start)?;
                self.distinct_union(vec![identity, once], Sort::Pairs)
            },
            Normal::OneOrMore(inner) => self.closure(inner, start),
            Normal::ZeroOrMore(inner) => {
                let identity = self.identity(start)?;
                let more = self.closure(inner, start)?;
                self.distinct_union(vec![identity, more], Sort::Pairs)
            },
        }
    }

    /// `p+` from `start`: `p` from the start nodes, then `p` from every node
    /// reached. The step is `p` from every node of the data, so a zero-length
    /// part of `p` holds for all of them.
    fn closure(&mut self, inner: &Normal, start: Start) -> Result<RelId, DenoteError> {
        let base = self.path(inner, start)?;
        let step = self.path(inner, Start::All)?;
        self.op(Op::Closure { base, step })
    }

    /// The focus → value pairs of `shape` for the distinct focus nodes `focus`.
    fn values(&mut self, shape: &IRShape, focus: RelId) -> Result<RelId, DenoteError> {
        match shape.path() {
            None => self.identity(Start::Nodes(focus)),
            Some(path) => self.path(&normalise(path, false), Start::Nodes(focus)),
        }
    }

    // --- shapes --------------------------------------------------------------

    fn component_rows(
        &mut self,
        shape: &'a IRShape,
        focus: RelId,
        values: RelId,
    ) -> Result<Vec<(usize, Rows)>, DenoteError> {
        let mut out = Vec::new();
        for (index, component) in shape.components().iter().enumerate() {
            let mut c = Components {
                d: self,
                shape,
                focus,
                values,
                component: IriS::from(component),
            };
            out.extend(component.accept(&mut c)?.into_iter().map(|rows| (index, rows)));
        }
        Ok(out)
    }

    /// The nodes of `candidates` (distinct) that do not conform to `idx`;
    /// `None` when none can fail.
    fn fails(&mut self, idx: ShapeLabelIdx, candidates: RelId) -> Result<Option<RelId>, DenoteError> {
        if let Some(memo) = self.fails.get(&(idx, candidates)) {
            return Ok(*memo);
        }
        let shape = self.shape(idx)?;
        if shape.deactivated() {
            self.fails.insert((idx, candidates), None);
            return Ok(None);
        }
        let values = self.values(shape, candidates)?;
        let mut failing = Vec::new();
        for (_, rows) in self.component_rows(shape, candidates, values)? {
            failing.push(self.op(Op::Focus(rows.rows))?);
        }
        // A nested property shape takes this shape's value nodes as focus.
        let nested_candidates = match shape {
            IRShape::NodeShape(_) => candidates,
            IRShape::PropertyShape(_) => {
                let v = self.op(Op::Values(values))?;
                self.op(Op::Distinct(v))?
            },
        };
        for nested in shape.property_shapes() {
            let Some(nested_fails) = self.fails(*nested, nested_candidates)? else {
                continue;
            };
            let failing_here = match shape {
                IRShape::NodeShape(_) => nested_fails,
                IRShape::PropertyShape(_) => {
                    let hit = self.op(Op::Filter(values, Pred::Member(Expr::v(), nested_fails)))?;
                    self.op(Op::Focus(hit))?
                },
            };
            failing.push(failing_here);
        }
        let out = if failing.is_empty() {
            None
        } else {
            Some(self.union(failing, Sort::Nodes)?)
        };
        self.fails.insert((idx, candidates), out);
        Ok(out)
    }

    /// The checks of `shape` for the focus relation `focus` (a bag when `bag`).
    fn emit(&mut self, idx: ShapeLabelIdx, focus: RelId, bag: bool) -> Result<(), DenoteError> {
        let shape = self.shape(idx)?;
        if shape.deactivated() {
            return Ok(());
        }
        let distinct = if bag { self.op(Op::Distinct(focus))? } else { focus };
        let values = self.values(shape, distinct)?;
        for (component, rows) in self.component_rows(shape, distinct, values)? {
            let rows_rel = if bag {
                // One copy of each result per occurrence of its focus node.
                self.op(Op::Repeat {
                    rel: rows.rows,
                    bag: focus,
                })?
            } else {
                rows.rows
            };
            self.b.check(Check {
                shape: idx,
                component: rows.component,
                severity: shape.severity().clone(),
                path: shape.path().cloned(),
                parameters: match rows.parameters {
                    Some(own) => Parameters::Own(own),
                    None => Parameters::Component(component),
                },
                rows: rows_rel,
            });
        }
        for nested in shape.property_shapes() {
            let (nested_focus, nested_bag) = match shape {
                IRShape::NodeShape(_) => (focus, bag),
                IRShape::PropertyShape(_) => {
                    let reached = self.op(Op::Repeat {
                        rel: values,
                        bag: focus,
                    })?;
                    (self.op(Op::Values(reached))?, true)
                },
            };
            self.emit(*nested, nested_focus, nested_bag)?;
        }
        Ok(())
    }
}

/// The components of one shape, against its focus relation `F` (distinct) and
/// value relation `VP` (its focus → value pairs).
struct Components<'d, 'a> {
    d: &'d mut Denoter<'a>,
    shape: &'a IRShape,
    focus: RelId,
    values: RelId,
    component: IriS,
}

impl Components<'_, '_> {
    fn one(&self, rows: RelId) -> Vec<Rows> {
        vec![Rows {
            component: self.component.clone(),
            rows,
            parameters: None,
        }]
    }

    /// The distinct value nodes.
    fn value_set(&mut self) -> Result<RelId, DenoteError> {
        let v = self.d.op(Op::Values(self.values))?;
        self.d.op(Op::Distinct(v))
    }

    /// The nodes of the value set that fail `shape`.
    fn fails(&mut self, shape: ShapeLabelIdx) -> Result<Option<RelId>, DenoteError> {
        let candidates = self.value_set()?;
        self.d.fails(shape, candidates)
    }

    /// `(f, v)` rows for the value nodes where `violates` holds.
    fn value_rows(&mut self, violates: Pred) -> Result<Vec<Rows>, DenoteError> {
        let hit = match violates {
            Pred::True => self.values,
            p => self.d.op(Op::Filter(self.values, p))?,
        };
        let rows = self.d.op(Op::PairRows {
            pairs: hit,
            with_value: true,
        })?;
        Ok(self.one(rows))
    }

    /// `(f, —, —)` rows for the focus nodes whose count of values satisfying
    /// `counted` is `op bound`.
    fn count(&mut self, counted: Pred, op: CmpOp, bound: isize) -> Result<RelId, DenoteError> {
        self.d.op(Op::Count {
            focus: self.focus,
            pairs: self.values,
            counted,
            op,
            bound: i64::try_from(bound).unwrap_or(i64::MAX),
        })
    }

    fn range(&mut self, bound: &ConcreteLiteral, op: CmpOp) -> Result<Vec<Rows>, DenoteError> {
        let holds = Pred::and(vec![
            Pred::KindIn(Expr::v(), vec![Kind::Literal]),
            Pred::Compare(Expr::v(), op, Expr::Const(Object::Literal(bound.clone()))),
        ]);
        self.value_rows(!holds)
    }

    fn length(&mut self, len: isize, op: CmpOp) -> Result<Vec<Rows>, DenoteError> {
        let bound = i64::try_from(len).unwrap_or(i64::MAX);
        self.value_rows(!Pred::StrLen(Expr::v(), op, bound))
    }

    /// The other predicate's `(f, v)` pairs.
    fn other(&mut self, predicate: &IriS) -> Result<RelId, DenoteError> {
        self.d.op(Op::Predicate(predicate.clone()))
    }

    /// Values whose `(f, v)` is (`present`) or is not in the other predicate's pairs.
    fn against_other(&mut self, predicate: &IriS, present: bool) -> Result<RelId, DenoteError> {
        let other = self.other(predicate)?;
        let member = Pred::PairMember(Expr::f(), Expr::v(), other);
        let test = if present { member } else { !member };
        let hit = self.d.op(Op::Filter(
            self.values,
            Pred::and(vec![!Pred::KindIn(Expr::f(), vec![Kind::Literal]), test]),
        ))?;
        self.d.op(Op::PairRows {
            pairs: hit,
            with_value: true,
        })
    }

    fn ordered(&mut self, predicate: &IriS, op: CmpOp) -> Result<Vec<Rows>, DenoteError> {
        let other = self.other(predicate)?;
        let rows = self.d.op(Op::PairJoin {
            left: self.values,
            right: other,
            pred: !Pred::Compare(Expr::v(), op, Expr::o()),
        })?;
        Ok(self.one(rows))
    }
}

impl IRComponentVisitor for Components<'_, '_> {
    type Output = Vec<Rows>;
    type Error = DenoteError;

    /// Never reached: every arm below is overridden, so a component added to
    /// the IR has no denotation until someone writes it.
    fn default_component(&mut self) -> Result<Self::Output, Self::Error> {
        Err(DenoteError::Unsupported(format!("the component {}", self.component)))
    }

    // --- value type ---------------------------------------------------------

    fn visit_class(&mut self, class: &Object) -> Result<Self::Output, Self::Error> {
        let extent = match class {
            Object::Iri(iri) => Some(self.d.op(Op::Class(iri.clone()))?),
            _ => None,
        };
        self.value_rows(!Pred::member(Expr::v(), extent))
    }

    fn visit_datatype(&mut self, datatypes: &[IriS]) -> Result<Self::Output, Self::Error> {
        self.value_rows(!Pred::Datatype(Expr::v(), datatypes.to_vec()))
    }

    fn visit_node_kind(&mut self, node_kind: &NodeKind) -> Result<Self::Output, Self::Error> {
        let kinds = match node_kind {
            NodeKind::Iri => vec![Kind::Iri],
            NodeKind::Lit => vec![Kind::Literal],
            NodeKind::BNode => vec![Kind::Blank],
            NodeKind::BNodeOrIri => vec![Kind::Blank, Kind::Iri],
            NodeKind::BNodeOrLit => vec![Kind::Blank, Kind::Literal],
            NodeKind::IriOrLit => vec![Kind::Iri, Kind::Literal],
        };
        self.value_rows(!Pred::KindIn(Expr::v(), kinds))
    }

    // --- cardinality --------------------------------------------------------

    fn visit_min_count(&mut self, count: isize) -> Result<Self::Output, Self::Error> {
        if count <= 0 {
            return Ok(Vec::new());
        }
        let rows = self.count(Pred::True, CmpOp::Lt, count)?;
        Ok(self.one(rows))
    }

    fn visit_max_count(&mut self, count: isize) -> Result<Self::Output, Self::Error> {
        let rows = self.count(Pred::True, CmpOp::Gt, count)?;
        Ok(self.one(rows))
    }

    // --- value range --------------------------------------------------------

    fn visit_min_exclusive(&mut self, lit: &ConcreteLiteral) -> Result<Self::Output, Self::Error> {
        self.range(lit, CmpOp::Gt)
    }

    fn visit_max_exclusive(&mut self, lit: &ConcreteLiteral) -> Result<Self::Output, Self::Error> {
        self.range(lit, CmpOp::Lt)
    }

    fn visit_min_inclusive(&mut self, lit: &ConcreteLiteral) -> Result<Self::Output, Self::Error> {
        self.range(lit, CmpOp::GtEq)
    }

    fn visit_max_inclusive(&mut self, lit: &ConcreteLiteral) -> Result<Self::Output, Self::Error> {
        self.range(lit, CmpOp::LtEq)
    }

    // --- string -------------------------------------------------------------

    fn visit_min_length(&mut self, len: isize) -> Result<Self::Output, Self::Error> {
        self.length(len, CmpOp::GtEq)
    }

    fn visit_max_length(&mut self, len: isize) -> Result<Self::Output, Self::Error> {
        self.length(len, CmpOp::LtEq)
    }

    fn visit_pattern(&mut self, pattern: &Pattern) -> Result<Self::Output, Self::Error> {
        self.value_rows(!Pred::Regex(
            Expr::v(),
            pattern.pattern().clone(),
            pattern.flags().cloned(),
        ))
    }

    fn visit_unique_lang(&mut self, unique: bool) -> Result<Self::Output, Self::Error> {
        if !unique {
            return Ok(Vec::new());
        }
        let rows = self.d.op(Op::LangDuplicates(self.values))?;
        Ok(self.one(rows))
    }

    fn visit_language_in(&mut self, langs: &[Lang]) -> Result<Self::Output, Self::Error> {
        self.value_rows(!Pred::LangIn(Expr::v(), langs.to_vec()))
    }

    // --- property pair ------------------------------------------------------

    fn visit_equals(&mut self, iri: &IriS) -> Result<Self::Output, Self::Error> {
        // Values the other predicate lacks, then other values the path lacks.
        let missing = self.against_other(iri, false)?;
        let other = self.other(iri)?;
        let others_here = self.d.op(Op::Restrict {
            pairs: other,
            nodes: self.focus,
        })?;
        let extra = self.d.op(Op::Filter(
            others_here,
            Pred::and(vec![
                !Pred::KindIn(Expr::f(), vec![Kind::Literal]),
                !Pred::PairMember(Expr::f(), Expr::v(), self.values),
            ]),
        ))?;
        let extra = self.d.op(Op::PairRows {
            pairs: extra,
            with_value: true,
        })?;
        let rows = self.d.op(Op::Union(vec![missing, extra]))?;
        Ok(self.one(rows))
    }

    fn visit_disjoint(&mut self, iri: &IriS) -> Result<Self::Output, Self::Error> {
        let rows = self.against_other(iri, true)?;
        Ok(self.one(rows))
    }

    fn visit_less_than(&mut self, iri: &IriS) -> Result<Self::Output, Self::Error> {
        self.ordered(iri, CmpOp::Lt)
    }

    fn visit_less_than_or_equals(&mut self, iri: &IriS) -> Result<Self::Output, Self::Error> {
        self.ordered(iri, CmpOp::LtEq)
    }

    // --- logical ------------------------------------------------------------

    fn visit_or(&mut self, shapes: &[ShapeLabelIdx]) -> Result<Self::Output, Self::Error> {
        let mut fails_all = Vec::new();
        for shape in shapes {
            let fails = self.fails(*shape)?;
            fails_all.push(Pred::member(Expr::v(), fails));
        }
        self.value_rows(Pred::and(fails_all))
    }

    fn visit_and(&mut self, shapes: &[ShapeLabelIdx]) -> Result<Self::Output, Self::Error> {
        let mut fails_any = Vec::new();
        for shape in shapes {
            let fails = self.fails(*shape)?;
            fails_any.push(Pred::member(Expr::v(), fails));
        }
        self.value_rows(Pred::or(fails_any))
    }

    fn visit_not(&mut self, shape: ShapeLabelIdx) -> Result<Self::Output, Self::Error> {
        let fails = self.fails(shape)?;
        self.value_rows(!Pred::member(Expr::v(), fails))
    }

    fn visit_xone(&mut self, shapes: &[ShapeLabelIdx]) -> Result<Self::Output, Self::Error> {
        let mut conforming = Vec::new();
        for shape in shapes {
            let fails = self.fails(*shape)?;
            conforming.push(!Pred::member(Expr::v(), fails));
        }
        self.value_rows(!Pred::ExactlyOne(conforming))
    }

    fn visit_if(&mut self, if_: &If) -> Result<Self::Output, Self::Error> {
        let cond = self.fails(*if_.cond())?;
        let conforms_cond = !Pred::member(Expr::v(), cond);
        let mut violations = Vec::new();
        if let Some(then) = if_.then() {
            let then = self.fails(*then)?;
            violations.push(Pred::and(vec![conforms_cond.clone(), Pred::member(Expr::v(), then)]));
        }
        if let Some(els) = if_.els() {
            let els = self.fails(*els)?;
            violations.push(Pred::and(vec![!conforms_cond, Pred::member(Expr::v(), els)]));
        }
        if violations.is_empty() {
            return Ok(Vec::new());
        }
        self.value_rows(Pred::or(violations))
    }

    // --- shape-based --------------------------------------------------------

    fn visit_node(&mut self, shape: ShapeLabelIdx) -> Result<Self::Output, Self::Error> {
        match self.fails(shape)? {
            None => Ok(Vec::new()),
            Some(fails) => self.value_rows(Pred::Member(Expr::v(), fails)),
        }
    }

    fn visit_qualified_value_shape(&mut self, qvs: &QualifiedValueShape) -> Result<Self::Output, Self::Error> {
        // A value counts when it conforms to the shape and, for
        // sh:qualifiedValueShapesDisjoint, to none of the sibling shapes.
        let fails = self.fails(*qvs.shape())?;
        let mut counted = vec![!Pred::member(Expr::v(), fails)];
        for sibling in qvs.siblings() {
            let sibling_fails = self.fails(*sibling)?;
            counted.push(Pred::member(Expr::v(), sibling_fails));
        }
        let counted = Pred::and(counted);
        let mut out = Vec::new();
        if let Some(min) = qvs.qualified_min_count() {
            let rows = self.count(counted.clone(), CmpOp::Lt, min)?;
            out.push(Rows {
                component: ShaclVocab::sh_qualified_min_count_constraint_component(),
                rows,
                parameters: Some(vec![("qualifiedMinCount", min.to_string())]),
            });
        }
        if let Some(max) = qvs.qualified_max_count() {
            let rows = self.count(counted, CmpOp::Gt, max)?;
            out.push(Rows {
                component: ShaclVocab::sh_qualified_max_count_constraint_component(),
                rows,
                parameters: Some(vec![("qualifiedMaxCount", max.to_string())]),
            });
        }
        Ok(out)
    }

    fn visit_closed(&mut self, closed: &Closed) -> Result<Self::Output, Self::Error> {
        if !closed.is_closed() {
            return Ok(Vec::new());
        }
        // SHACL §4.8.1: every triple of a value node whose predicate is neither
        // a property of the shape nor ignored, with the predicate as the
        // result path and the object as the value.
        let mut allowed: Vec<IriS> = self.shape.allowed_properties().into_iter().collect();
        allowed.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        let triples = self.d.op(Op::Triples)?;
        let rows = self.d.op(Op::Outgoing {
            pairs: self.values,
            triples,
            allowed,
        })?;
        Ok(self.one(rows))
    }

    fn visit_has_value(&mut self, value: &Object) -> Result<Self::Output, Self::Error> {
        let missing = self.d.op(Op::Filter(
            self.focus,
            !Pred::PairMember(Expr::f(), Expr::Const(value.clone()), self.values),
        ))?;
        let rows = self.d.op(Op::NodeRows(missing))?;
        Ok(self.one(rows))
    }

    fn visit_in(&mut self, values: &[Object]) -> Result<Self::Output, Self::Error> {
        let any = Pred::or(
            values
                .iter()
                .map(|v| Pred::Same(Expr::v(), Expr::Const(v.clone())))
                .collect(),
        );
        self.value_rows(!any)
    }

    // --- status / SPARQL ----------------------------------------------------

    fn visit_deactivated(&mut self, _deactivated: bool) -> Result<Self::Output, Self::Error> {
        // A deactivated shape is never denoted; on an active one the component
        // itself raises nothing.
        Ok(Vec::new())
    }

    fn visit_basic_sparql(&mut self, _sparql: &BasicSparql) -> Result<Self::Output, Self::Error> {
        Err(DenoteError::Unsupported(
            "sh:sparql (SHACL-SPARQL is outside SHACL Core)".to_owned(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(local: &str) -> SHACLPath {
        SHACLPath::iri(IriS::new_unchecked(&format!("http://e/{local}")))
    }

    #[test]
    fn inverse_of_a_sequence_reverses_it() {
        let path = SHACLPath::Inverse {
            path: Box::new(SHACLPath::Sequence {
                paths: vec![p("a"), p("b")],
            }),
        };
        let Normal::Sequence(steps) = normalise(&path, false) else {
            panic!("a sequence")
        };
        let names: Vec<_> = steps
            .iter()
            .map(|s| match s {
                Normal::Predicate { iri, inverse } => (iri.as_str().to_owned(), *inverse),
                _ => panic!("a predicate"),
            })
            .collect();
        assert_eq!(
            names,
            vec![("http://e/b".to_owned(), true), ("http://e/a".to_owned(), true)]
        );
    }
}
