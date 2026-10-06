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
//! A shape the engine does not check ([`profile`](super::profile)) yields no
//! checks, and nothing checked depends on its conformance; the rest of the
//! dependency graph is acyclic, so `fails` recurses into strictly lower strata
//! and terminates.

use crate::algebra::profile::{self, Unchecked};
use crate::algebra::{Check, CmpOp, Expr, Key, Kind, Op, Parameters, Plan, PlanBuilder, Pred, RelId, Sort, SortError};
use crate::ir::components::{Closed, QualifiedValueShape};
use crate::ir::{IRComponent, IRSchema, IRShape, ReifierInfo, ShapeLabelIdx};
use crate::types::{NodeKind, Severity, Target};
use rudof_iri::IriS;
use rudof_rdf::SHACLPath;
use rudof_rdf::term::Object;
use rudof_rdf::term::literal::ConcreteLiteral;
use rudof_rdf::vocab::{RdfVocab, RdfsVocab, ShaclVocab};
use std::collections::HashMap;

/// Why a shapes graph has no denotation here.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DenoteError {
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

/// The plan of `schema`: one check per constraint component of each shape in
/// each context it is reached in.
///
/// A shape with targets yields the checks of its components and of its
/// property shapes for its focus nodes. A shape without targets is reached only through another (as a property shape, or by `sh:node`,
/// `sh:and`, …). Deactivated shapes yield nothing and conform everywhere.
pub fn denote(schema: &IRSchema) -> Result<Plan, DenoteError> {
    let mut d = Denoter::new(schema)?;
    let mut shapes: Vec<ShapeLabelIdx> = schema
        .iter()
        .filter_map(|(id, _)| schema.get_idx(id).copied())
        .collect();
    shapes.sort_unstable();
    for idx in shapes {
        let shape = d.shape(idx)?;
        if shape.deactivated() {
            continue;
        }
        if let Some(focus) = d.focus(shape)? {
            d.emit(idx, focus, false)?;
        }
    }
    Ok(d.finish())
}

/// The plan of one shape and its property shapes: for `focus` when given,
/// otherwise for the shape's own targets. The checks a form revalidates when
/// one node or one shape changes.
pub fn denote_shape(schema: &IRSchema, idx: ShapeLabelIdx, focus: Option<&Object>) -> Result<Plan, DenoteError> {
    let mut d = Denoter::new(schema)?;
    let shape = d.shape(idx)?;
    let focus = match focus {
        Some(node) => d.op(Op::Constants(vec![node.clone()]))?,
        None => match d.focus(shape)? {
            Some(focus) => focus,
            None => d.op(Op::Empty(Sort::Nodes))?,
        },
    };
    d.emit(idx, focus, false)?;
    Ok(d.finish())
}

struct Denoter<'a> {
    schema: &'a IRSchema,
    b: PlanBuilder,
    fails: HashMap<(ShapeLabelIdx, RelId), Option<RelId>>,
    unchecked: HashMap<ShapeLabelIdx, Unchecked>,
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
    fn new(schema: &'a IRSchema) -> Result<Self, DenoteError> {
        Ok(Self {
            schema,
            b: PlanBuilder::new(),
            fails: HashMap::new(),
            unchecked: profile::unchecked(schema)?,
        })
    }

    /// The plan, with the unchecked shapes in the order of their nodes.
    fn finish(self) -> Plan {
        let mut plan = self.b.finish();
        plan.unchecked = self.unchecked.into_values().collect();
        plan.unchecked.sort_by_key(|u| u.shape.to_string());
        plan
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

    /// The focus nodes of `shape`, distinct: those of its targets. `None` for
    /// a shape with none.
    ///
    /// TODO: `sh:shape` in the data graph (SHACL 1.2 §3.1.3.7), whose section
    /// is still a TODO in the draft. It is left out until the section settles.
    fn focus(&mut self, shape: &IRShape) -> Result<Option<RelId>, DenoteError> {
        let mut parts = Vec::new();
        for target in shape.targets() {
            parts.push(self.target(target)?);
        }
        if parts.is_empty() {
            return Ok(None);
        }
        Ok(Some(self.distinct_union(parts, Sort::Nodes)?))
    }

    fn target(&mut self, target: &Target) -> Result<RelId, DenoteError> {
        match target {
            Target::Node(node) => self.op(Op::Constants(vec![node.clone()])),
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
                    let composed = self.op(Op::Compose(acc, next))?;
                    acc = self.op(Op::Distinct(composed))?;
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

    /// The rows of each component of `shape`, by the component's index; those
    /// of `sh:reifierShape`, which the shape states apart, have none.
    fn component_rows(
        &mut self,
        shape: &'a IRShape,
        focus: RelId,
        values: RelId,
    ) -> Result<Vec<(Option<usize>, Rows)>, DenoteError> {
        let mut out = Vec::new();
        for (index, component) in shape.components().iter().enumerate() {
            let mut c = Components {
                d: self,
                shape,
                focus,
                values,
                component: IriS::from(component),
            };
            out.extend(c.denote(component)?.into_iter().map(|rows| (Some(index), rows)));
        }
        if let Some(info) = shape.reifier_info() {
            let mut c = Components {
                d: self,
                shape,
                focus,
                values,
                component: ShaclVocab::sh_reifier_shape_constraint_component(),
            };
            out.extend(c.reifier(info)?.into_iter().map(|rows| (None, rows)));
        }
        Ok(out)
    }

    /// The nodes of `candidates` (distinct) that do not conform to `idx`;
    /// `None` when none can fail.
    fn fails(&mut self, idx: ShapeLabelIdx, candidates: RelId) -> Result<Option<RelId>, DenoteError> {
        if let Some(memo) = self.fails.get(&(idx, candidates)) {
            return Ok(*memo);
        }
        if let Some(u) = self.unchecked.get(&idx) {
            return Err(DenoteError::Internal(format!(
                "the conformance of {}, an unchecked shape",
                u.shape
            )));
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
        if shape.deactivated() || self.unchecked.contains_key(&idx) {
            return Ok(());
        }
        let distinct = if bag { self.op(Op::Distinct(focus))? } else { focus };
        let values = self.values(shape, distinct)?;
        for (index, rows) in self.component_rows(shape, distinct, values)? {
            let rows_rel = if bag {
                // One copy of each result per occurrence of its focus node.
                self.op(Op::Repeat {
                    rel: rows.rows,
                    bag: focus,
                })?
            } else {
                rows.rows
            };
            // What a reifier says of the one constraint overrides the shape.
            let annotation = index.map(|i| shape.annotation(i));
            let severity: Severity = annotation
                .and_then(|a| a.severity.clone())
                .unwrap_or_else(|| shape.severity().clone());
            let message = annotation
                .and_then(|a| a.message.clone())
                .or_else(|| shape.message().cloned())
                .filter(|m| !m.messages().is_empty());
            self.b.check(Check {
                shape: idx,
                component: rows.component,
                severity,
                path: shape.path().cloned(),
                message,
                parameters: match (rows.parameters, index) {
                    (Some(own), _) => Parameters::Own(own),
                    (None, Some(i)) => Parameters::Component(i),
                    (None, None) => Parameters::Own(Vec::new()),
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

/// The SHACL lists among a value set (SHACL 1.2 Core §1.4).
struct Lists {
    /// The value nodes that are not well-formed SHACL lists.
    malformed: RelId,
    /// `(v, cell)` for each cell of each value node but `rdf:nil`, distinct:
    /// one per member of a well-formed list.
    cells: RelId,
}

fn predicate(iri: IriS) -> Normal {
    Normal::Predicate { iri, inverse: false }
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

    /// `(f, v)` rows for the pairs of `pairs` where `violates` holds.
    fn rows_of(&mut self, pairs: RelId, violates: Pred) -> Result<Vec<Rows>, DenoteError> {
        let hit = match violates {
            Pred::True => pairs,
            p => self.d.op(Op::Filter(pairs, p))?,
        };
        let rows = self.d.op(Op::PairRows {
            pairs: hit,
            with_value: true,
        })?;
        Ok(self.one(rows))
    }

    /// `(f, v)` rows for the value nodes where `violates` holds.
    fn value_rows(&mut self, violates: Pred) -> Result<Vec<Rows>, DenoteError> {
        self.rows_of(self.values, violates)
    }

    /// The focus nodes of `focus` whose count of pairs of `pairs` satisfying
    /// `counted` is `op bound`, as `(f, —, —)` rows.
    fn count_of(
        &mut self,
        focus: RelId,
        pairs: RelId,
        counted: Pred,
        op: CmpOp,
        bound: isize,
    ) -> Result<RelId, DenoteError> {
        self.d.op(Op::Count {
            focus,
            pairs,
            counted,
            op,
            bound: i64::try_from(bound).unwrap_or(i64::MAX),
        })
    }

    /// `(f, —, —)` rows for the focus nodes whose count of values satisfying
    /// `counted` is `op bound`.
    fn count(&mut self, counted: Pred, op: CmpOp, bound: isize) -> Result<Vec<Rows>, DenoteError> {
        let rows = self.count_of(self.focus, self.values, counted, op, bound)?;
        Ok(self.one(rows))
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

    /// The `(f, o)` pairs of the other path of a property pair component, from
    /// the focus nodes.
    fn other(&mut self, path: &SHACLPath) -> Result<RelId, DenoteError> {
        self.d.path(&normalise(path, false), Start::Nodes(self.focus))
    }

    fn ordered(&mut self, path: &SHACLPath, op: CmpOp) -> Result<Vec<Rows>, DenoteError> {
        let other = self.other(path)?;
        let pairs = self.d.op(Op::PairJoin {
            left: self.values,
            right: other,
            pred: !Pred::Compare(Expr::v(), op, Expr::o()),
        })?;
        self.rows_of(pairs, Pred::True)
    }

    /// The value nodes as SHACL lists: a value node is one when it is
    /// `rdf:nil` without `rdf:first` or `rdf:rest`, or has exactly one of each
    /// with the `rdf:rest` a list, and does not reach itself by `rdf:rest+`.
    fn lists(&mut self) -> Result<Lists, DenoteError> {
        let values = self.value_set()?;
        let rest = predicate(RdfVocab::rdf_rest());
        let first = predicate(RdfVocab::rdf_first());
        let nil = Expr::Const(Object::Iri(RdfVocab::rdf_nil()));
        let reached = self
            .d
            .path(&Normal::ZeroOrMore(Box::new(rest.clone())), Start::Nodes(values))?;
        let cells = self.d.op(Op::Values(reached))?;
        let cells = self.d.op(Op::Distinct(cells))?;
        let firsts = self.d.path(&first, Start::Nodes(cells))?;
        let rests = self.d.path(&rest, Start::Nodes(cells))?;
        let is_nil = Pred::Same(Expr::f(), nil.clone());
        let mut bad = Vec::new();
        for (arcs, op, bound, at_nil) in [
            (firsts, CmpOp::Gt, 0, true),
            (rests, CmpOp::Gt, 0, true),
            (firsts, CmpOp::Lt, 1, false),
            (firsts, CmpOp::Gt, 1, false),
            (rests, CmpOp::Lt, 1, false),
            (rests, CmpOp::Gt, 1, false),
        ] {
            let counted = self.count_of(cells, arcs, Pred::True, op, bound)?;
            let here = if at_nil { is_nil.clone() } else { !is_nil.clone() };
            let hit = self.d.op(Op::Filter(counted, here))?;
            bad.push(self.d.op(Op::Focus(hit))?);
        }
        let cycles = self.d.closure(&rest, Start::Nodes(cells))?;
        let cycles = self.d.op(Op::Filter(cycles, Pred::Same(Expr::f(), Expr::v())))?;
        bad.push(self.d.op(Op::Focus(cycles))?);
        let bad = self.d.union(bad, Sort::Nodes)?;
        let malformed = self.d.op(Op::Filter(reached, Pred::Member(Expr::v(), bad)))?;
        let malformed = self.d.op(Op::Focus(malformed))?;
        let malformed = self.d.op(Op::Distinct(malformed))?;
        let cells = self.d.op(Op::Filter(reached, !Pred::Same(Expr::v(), nil)))?;
        Ok(Lists { malformed, cells })
    }

    /// `(v, member)` for each member of each value node, once per cell (a bag).
    fn members(&mut self, lists: &Lists) -> Result<RelId, DenoteError> {
        let cells = self.d.op(Op::Values(lists.cells))?;
        let cells = self.d.op(Op::Distinct(cells))?;
        let firsts = self.d.path(&predicate(RdfVocab::rdf_first()), Start::Nodes(cells))?;
        self.d.op(Op::Compose(lists.cells, firsts))
    }

    /// The rows of a list component: a value node that is not a well-formed
    /// list, or whose list is one of `failing` (nodes).
    fn list_rows(&mut self, lists: &Lists, failing: Option<RelId>) -> Result<Vec<Rows>, DenoteError> {
        self.value_rows(Pred::or(vec![
            Pred::Member(Expr::v(), lists.malformed),
            Pred::member(Expr::v(), failing),
        ]))
    }

    fn list_length(&mut self, op: CmpOp, bound: isize) -> Result<Vec<Rows>, DenoteError> {
        let lists = self.lists()?;
        let values = self.value_set()?;
        let counted = self.count_of(values, lists.cells, Pred::True, op, bound)?;
        let failing = self.d.op(Op::Focus(counted))?;
        self.list_rows(&lists, Some(failing))
    }

    /// `sh:uniqueValuesFor` (SHACL 1.2 Core §8.7): for each value node `V`, each
    /// other target node of the shape with exactly the values of `V` for every
    /// property, as the value.
    fn unique_values_for(&mut self, properties: &[IriS]) -> Result<Vec<Rows>, DenoteError> {
        let Some(targets) = self.d.focus(self.shape)? else {
            return Ok(Vec::new());
        };
        let values = self.value_set()?;
        let mut arcs = Vec::new();
        let mut sharing = Vec::new();
        for p in properties {
            let of_values = self.d.path(&predicate(p.clone()), Start::Nodes(values))?;
            let of_targets = self.d.path(&predicate(p.clone()), Start::Nodes(targets))?;
            let to_targets = self.d.op(Op::Inverse(of_targets))?;
            sharing.push(self.d.op(Op::Compose(of_values, to_targets))?);
            arcs.push((of_values, of_targets));
        }
        // `(V, Other)` with a value in common; then those without a difference.
        let candidates = self.d.distinct_union(sharing, Sort::Pairs)?;
        let flipped = self.d.op(Op::Inverse(candidates))?;
        let mut same = vec![!Pred::Same(Expr::f(), Expr::v())];
        for (of_values, of_targets) in arcs {
            let lacks = self.d.op(Op::PairJoin {
                left: candidates,
                right: of_values,
                pred: !Pred::PairMember(Expr::v(), Expr::o(), of_targets),
            })?;
            let adds = self.d.op(Op::PairJoin {
                left: flipped,
                right: of_targets,
                pred: !Pred::PairMember(Expr::v(), Expr::o(), of_values),
            })?;
            let adds = self.d.op(Op::Inverse(adds))?;
            same.push(!Pred::PairMember(Expr::f(), Expr::v(), lacks));
            same.push(!Pred::PairMember(Expr::f(), Expr::v(), adds));
        }
        let same = self.d.op(Op::Filter(candidates, Pred::and(same)))?;
        let pairs = self.d.op(Op::Compose(self.values, same))?;
        self.rows_of(pairs, Pred::True)
    }

    /// The rows of `component`.
    fn denote(&mut self, component: &IRComponent) -> Result<Vec<Rows>, DenoteError> {
        let v = Expr::v;
        match component {
            // --- value type ---------------------------------------------------
            IRComponent::Class(classes) => {
                let mut any = Vec::new();
                for class in classes {
                    any.push(Pred::Member(v(), self.d.op(Op::Class(class.clone()))?));
                }
                self.value_rows(!Pred::or(any))
            },
            IRComponent::Datatype(datatypes) => self.value_rows(!Pred::Datatype(v(), datatypes.clone())),
            IRComponent::NodeKind(node_kinds) => {
                let mut kinds = Vec::new();
                for node_kind in node_kinds {
                    kinds.extend(match node_kind {
                        NodeKind::Iri => vec![Kind::Iri],
                        NodeKind::Lit => vec![Kind::Literal],
                        NodeKind::BNode => vec![Kind::Blank],
                        NodeKind::BNodeOrIri => vec![Kind::Blank, Kind::Iri],
                        NodeKind::BNodeOrLit => vec![Kind::Blank, Kind::Literal],
                        NodeKind::IriOrLit => vec![Kind::Iri, Kind::Literal],
                        NodeKind::TripleTerm => vec![Kind::TripleTerm],
                    });
                }
                self.value_rows(!Pred::KindIn(v(), kinds))
            },
            // --- cardinality --------------------------------------------------
            IRComponent::MinCount(count) if *count <= 0 => Ok(Vec::new()),
            IRComponent::MinCount(count) => self.count(Pred::True, CmpOp::Lt, *count),
            IRComponent::MaxCount(count) => self.count(Pred::True, CmpOp::Gt, *count),
            // --- value range --------------------------------------------------
            IRComponent::MinExclusive(lit) => self.range(lit, CmpOp::Gt),
            IRComponent::MaxExclusive(lit) => self.range(lit, CmpOp::Lt),
            IRComponent::MinInclusive(lit) => self.range(lit, CmpOp::GtEq),
            IRComponent::MaxInclusive(lit) => self.range(lit, CmpOp::LtEq),
            // --- string -------------------------------------------------------
            IRComponent::MinLength(len) => self.length(*len, CmpOp::GtEq),
            IRComponent::MaxLength(len) => self.length(*len, CmpOp::LtEq),
            IRComponent::Pattern(pattern) => {
                self.value_rows(!Pred::Regex(v(), pattern.pattern().clone(), pattern.flags().cloned()))
            },
            IRComponent::SingleLine(false) | IRComponent::UniqueLang(false) => Ok(Vec::new()),
            // A literal whose lexical form matches `[\f\r\n\v]`.
            IRComponent::SingleLine(true) => self.value_rows(Pred::and(vec![
                Pred::KindIn(v(), vec![Kind::Literal]),
                Pred::Regex(v(), "[\u{0C}\r\n\u{0B}]".to_owned(), None),
            ])),
            IRComponent::UniqueLang(true) => {
                let rows = self.d.op(Op::Duplicates {
                    pairs: self.values,
                    key: Key::Lang,
                })?;
                Ok(self.one(rows))
            },
            IRComponent::LanguageIn(langs) => self.value_rows(!Pred::LangIn(v(), langs.clone())),
            // --- list ---------------------------------------------------------
            IRComponent::MemberShape(shape) => {
                let lists = self.lists()?;
                let members = self.members(&lists)?;
                let member_set = self.d.op(Op::Values(members))?;
                let member_set = self.d.op(Op::Distinct(member_set))?;
                let failing = match self.d.fails(*shape, member_set)? {
                    None => None,
                    Some(fails) => {
                        let hit = self.d.op(Op::Filter(members, Pred::Member(v(), fails)))?;
                        Some(self.d.op(Op::Focus(hit))?)
                    },
                };
                self.list_rows(&lists, failing)
            },
            IRComponent::MinListLength(n) => self.list_length(CmpOp::Lt, *n),
            IRComponent::MaxListLength(n) => self.list_length(CmpOp::Gt, *n),
            IRComponent::UniqueMembers(unique) => {
                let lists = self.lists()?;
                let failing = if *unique {
                    let members = self.members(&lists)?;
                    let duplicates = self.d.op(Op::Duplicates {
                        pairs: members,
                        key: Key::Term,
                    })?;
                    Some(self.d.op(Op::Focus(duplicates))?)
                } else {
                    None
                };
                self.list_rows(&lists, failing)
            },
            // --- property pair ------------------------------------------------
            IRComponent::Equals(path) => {
                // Values the other path lacks, then values of the other path
                // this one lacks.
                let other = self.other(path)?;
                let missing = self
                    .d
                    .op(Op::Filter(self.values, !Pred::PairMember(Expr::f(), v(), other)))?;
                let extra = self
                    .d
                    .op(Op::Filter(other, !Pred::PairMember(Expr::f(), v(), self.values)))?;
                let both = self.d.op(Op::Union(vec![missing, extra]))?;
                self.rows_of(both, Pred::True)
            },
            IRComponent::Disjoint(path) => {
                let other = self.other(path)?;
                self.value_rows(Pred::PairMember(Expr::f(), v(), other))
            },
            IRComponent::SubsetOf(path) => {
                let other = self.other(path)?;
                self.value_rows(!Pred::PairMember(Expr::f(), v(), other))
            },
            IRComponent::LessThan(path) => self.ordered(path, CmpOp::Lt),
            IRComponent::LessThanOrEquals(path) => self.ordered(path, CmpOp::LtEq),
            // --- logical ------------------------------------------------------
            IRComponent::Or(shapes) => {
                let mut fails_all = Vec::new();
                for shape in shapes {
                    fails_all.push(Pred::member(v(), self.fails(*shape)?));
                }
                self.value_rows(Pred::and(fails_all))
            },
            IRComponent::And(shapes) => {
                let mut fails_any = Vec::new();
                for shape in shapes {
                    fails_any.push(Pred::member(v(), self.fails(*shape)?));
                }
                self.value_rows(Pred::or(fails_any))
            },
            IRComponent::Not(shape) => {
                let fails = self.fails(*shape)?;
                self.value_rows(!Pred::member(v(), fails))
            },
            IRComponent::Xone(shapes) => {
                let mut conforming = Vec::new();
                for shape in shapes {
                    conforming.push(!Pred::member(v(), self.fails(*shape)?));
                }
                self.value_rows(!Pred::ExactlyOne(conforming))
            },
            IRComponent::If(if_) => {
                let conforms_cond = !Pred::member(v(), self.fails(*if_.cond())?);
                let mut violations = Vec::new();
                if let Some(then) = if_.then() {
                    let then = self.fails(*then)?;
                    violations.push(Pred::and(vec![conforms_cond.clone(), Pred::member(v(), then)]));
                }
                if let Some(els) = if_.els() {
                    let els = self.fails(*els)?;
                    violations.push(Pred::and(vec![!conforms_cond, Pred::member(v(), els)]));
                }
                if violations.is_empty() {
                    return Ok(Vec::new());
                }
                self.value_rows(Pred::or(violations))
            },
            // --- shape-based --------------------------------------------------
            // `sh:nodeByExpression` with an IRI: the shape it names (§5.3.2).
            IRComponent::Node(shape) | IRComponent::NodeByExpression(shape) => match self.fails(*shape)? {
                None => Ok(Vec::new()),
                Some(fails) => self.value_rows(Pred::Member(v(), fails)),
            },
            IRComponent::SomeValue(shape) => {
                let fails = self.fails(*shape)?;
                self.count(!Pred::member(v(), fails), CmpOp::Lt, 1)
            },
            IRComponent::QualifiedValueShape(qvs) => self.qualified_value_shape(qvs),
            // --- other --------------------------------------------------------
            IRComponent::Closed(closed) => self.closed(closed),
            IRComponent::HasValue(value) => {
                let missing = self.d.op(Op::Filter(
                    self.focus,
                    !Pred::PairMember(Expr::f(), Expr::Const(value.clone()), self.values),
                ))?;
                let rows = self.d.op(Op::NodeRows(missing))?;
                Ok(self.one(rows))
            },
            IRComponent::In(values) => {
                let any = values
                    .iter()
                    .map(|value| Pred::Same(v(), Expr::Const(value.clone())))
                    .collect();
                self.value_rows(!Pred::or(any))
            },
            IRComponent::RootClass(roots) => {
                // The roots and every class below them by `rdfs:subClassOf*`.
                let roots = self
                    .d
                    .op(Op::Constants(roots.iter().cloned().map(Object::Iri).collect()))?;
                let below = Normal::ZeroOrMore(Box::new(Normal::Predicate {
                    iri: RdfsVocab::rdfs_subclass_of_str(),
                    inverse: true,
                }));
                let below = self.d.path(&below, Start::Nodes(roots))?;
                let classes = self.d.op(Op::Values(below))?;
                self.value_rows(!Pred::and(vec![
                    Pred::KindIn(v(), vec![Kind::Iri]),
                    Pred::Member(v(), classes),
                ]))
            },
            IRComponent::UniqueValuesFor(properties) => self.unique_values_for(properties),
            // A deactivated shape is never denoted; on an active one the
            // component itself raises nothing.
            IRComponent::Deactivated(_) => Ok(Vec::new()),
            // Outside the profile: its shape is unchecked, and never denoted.
            IRComponent::BasicSparql(_) => Err(DenoteError::Internal(format!(
                "sh:sparql on {}, an unchecked shape",
                self.shape.id()
            ))),
        }
    }

    fn qualified_value_shape(&mut self, qvs: &QualifiedValueShape) -> Result<Vec<Rows>, DenoteError> {
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
            let rows = self.count_of(self.focus, self.values, counted.clone(), CmpOp::Lt, min)?;
            out.push(Rows {
                component: ShaclVocab::sh_qualified_min_count_constraint_component(),
                rows,
                parameters: Some(vec![("qualifiedMinCount", min.to_string())]),
            });
        }
        if let Some(max) = qvs.qualified_max_count() {
            let rows = self.count_of(self.focus, self.values, counted, CmpOp::Gt, max)?;
            out.push(Rows {
                component: ShaclVocab::sh_qualified_max_count_constraint_component(),
                rows,
                parameters: Some(vec![("qualifiedMaxCount", max.to_string())]),
            });
        }
        Ok(out)
    }

    /// SHACL 1.2 §8.4: every triple of a value node whose predicate is not
    /// permitted, with the predicate as the result path and the object as the
    /// value.
    fn closed(&mut self, closed: &Closed) -> Result<Vec<Rows>, DenoteError> {
        if !closed.is_closed() {
            return Ok(Vec::new());
        }
        let (mut allowed, by_type) = match closed.by_types() {
            Some(by_type) => {
                let mut allowed = closed.ignored_properties().clone();
                allowed.push(RdfVocab::rdf_type());
                (allowed, by_type.clone())
            },
            None => (self.shape.allowed_properties().into_iter().collect(), Vec::new()),
        };
        allowed.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        allowed.dedup();
        let triples = self.d.op(Op::Triples)?;
        let rows = self.d.op(Op::Outgoing {
            pairs: self.values,
            triples,
            allowed,
            by_type,
        })?;
        Ok(self.one(rows))
    }

    /// `sh:reifierShape` and `sh:reificationRequired` (SHACL 1.2 §8.3.3): the
    /// value nodes whose triple `(f, p, v)` has a reifier failing a reifier
    /// shape, or, when reification is required, has none.
    fn reifier(&mut self, info: &ReifierInfo) -> Result<Vec<Rows>, DenoteError> {
        let reifies = self.d.op(Op::Predicate(RdfVocab::rdf_reifies()))?;
        let reifiers = self.d.op(Op::Focus(reifies))?;
        let reifiers = self.d.op(Op::Distinct(reifiers))?;
        let mut failing = Vec::new();
        for shape in info.reifier_shape() {
            failing.push(Pred::member(Expr::f(), self.d.fails(*shape, reifiers)?));
        }
        let triple = Expr::triple(Expr::f(), info.predicate().clone(), Expr::v());
        let mut violates = Vec::new();
        if !failing.is_empty() {
            let bad = self.d.op(Op::Filter(reifies, Pred::or(failing)))?;
            let bad = self.d.op(Op::Values(bad))?;
            violates.push(Pred::Member(triple.clone(), bad));
        }
        if info.reification_required() {
            let reified = self.d.op(Op::Values(reifies))?;
            violates.push(!Pred::Member(triple, reified));
        }
        if violates.is_empty() {
            return Ok(Vec::new());
        }
        self.value_rows(Pred::or(violates))
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
