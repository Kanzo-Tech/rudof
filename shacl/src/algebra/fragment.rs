//! Shape Fragments: the triples that make the conforming nodes conform.
//!
//! The fragment of a shapes graph over a data graph `G` is the union, over
//! each shape `S` with targets and each focus node `v` of `S` that conforms to
//! `S`, of the neighbourhood `B(v, G, φ ∧ τ)` of `S`'s shape expression `φ` and
//! target expression `τ` (Delva, Dimou, Jakubowski and Van den Bussche, *Shape
//! Fragments*, arXiv:2112.11796, and *Data Provenance for SHACL*, EDBT 2023,
//! Table 2). It is sufficient (Theorem 3.4): `v` conforms to `S` in every graph
//! between its neighbourhood and `G`, so a subgraph that keeps the fragment
//! keeps every reason its nodes conform. It is ShEx's subsetting, in SHACL.
//!
//! The papers define the neighbourhood on a shape in negation normal form;
//! here negation is a flag carried down, and each constraint component of a
//! shape with path `E` (the identity for a node shape) gives, for the values
//! the rule names, the triples of the walks along `E` to them
//! ([`Fragmenter::trace`]) and their own neighbourhood:
//!
//! | component | holds | fails |
//! |---|---|---|
//! | a test of the value (`sh:datatype`, `sh:pattern`, `sh:in`, …) | every value | the values it fails for |
//! | `sh:class` | every value, and its `rdf:type/rdfs:subClassOf*` walk to the class | the values it fails for, and every such walk of theirs |
//! | `sh:minCount` | every value | nothing |
//! | `sh:maxCount` | nothing | every value |
//! | `sh:hasValue c` | `c` | every value |
//! | `sh:equals p` | every value, and every `p` walk | the values each path lacks of the other |
//! | `sh:disjoint p` | nothing | the values both paths reach, by both |
//! | `sh:lessThan p`, `sh:lessThanOrEquals p` | nothing | the values that break the order, by both paths |
//! | `sh:uniqueLang` | nothing | every value (the papers keep only those that share a language; a superset is as sufficient) |
//! | `sh:closed` | nothing | the triples it does not permit |
//! | `sh:node`, `sh:not`, `sh:and`, `sh:or`, `sh:xone` | every value, in the shapes it conforms to, or fails, as the rule needs | the values it fails for, likewise |
//! | qualified value shapes | the values the bound reads | likewise, negated |
//!
//! A target gives the triples that make the node a target: its
//! `rdf:type/rdfs:subClassOf*` walk to the class, or its triples with the
//! predicate of `sh:targetSubjectsOf` or `sh:targetObjectsOf`.
//!
//! What a fragment is defined for is a profile ([`profile::FRAGMENTS`]): a
//! shape outside it has no fragment, and the plan lists it as unchecked.

use crate::algebra::denote::{Components, Denoter, Normal, Rows, Start, normalise};
use crate::algebra::{CmpOp, DenoteError, Expr, Op, Plan, Pred, RelId, Sort, profile};
use crate::ir::components::QualifiedValueShape;
use crate::ir::{IRComponent, IRSchema, IRShape, ShapeLabelIdx};
use crate::types::Target;
use rudof_iri::IriS;
use rudof_rdf::SHACLPath;
use rudof_rdf::term::Object;
use rudof_rdf::vocab::{RdfVocab, RdfsVocab, ShaclVocab};
use std::collections::HashMap;

/// The plan of the fragment of `schema`: no checks, and the
/// [`Plan::fragment`] relation. When `scoped`, the focus nodes are those among
/// [`Op::Scope`], as [`denote`](super::denote) restricts them.
pub fn fragment(schema: &IRSchema, scoped: bool) -> Result<Plan, DenoteError> {
    let mut d = Denoter::new(schema, profile::fragment_members(), true)?;
    if scoped {
        d.scope = Some(d.op(Op::Scope)?);
    }
    let mut shapes: Vec<ShapeLabelIdx> = schema
        .iter()
        .filter_map(|(id, _)| schema.get_idx(id).copied())
        .collect();
    shapes.sort_unstable();
    let mut f = Fragmenter {
        d: &mut d,
        memo: HashMap::new(),
    };
    let mut parts = Vec::new();
    for idx in shapes {
        let shape = f.d.shape(idx)?;
        if shape.deactivated() || f.d.unchecked.contains_key(&idx) {
            continue;
        }
        let Some(focus) = f.d.focus(shape)? else {
            continue;
        };
        let fails = f.d.fails(idx, focus)?;
        let conforming = f.keep(focus, fails)?;
        for target in shape.targets() {
            parts.extend(f.target(target, conforming)?);
        }
        parts.extend(f.shape(idx, conforming, false)?);
    }
    let fragment = f.d.distinct_union(parts, Sort::Triples)?;
    d.b.fragment(fragment);
    Ok(d.finish())
}

/// The neighbourhoods of a plan's shapes, each built once.
struct Fragmenter<'d, 'a> {
    d: &'d mut Denoter<'a>,
    /// The triples of a shape over a node relation, held or failed.
    memo: HashMap<(ShapeLabelIdx, RelId, bool), Vec<RelId>>,
}

fn predicate(iri: IriS, inverse: bool) -> Normal {
    Normal::Predicate { iri, inverse }
}

/// `rdf:type/rdfs:subClassOf*`: from an instance to its classes (SHACL §1.1).
fn typed() -> Normal {
    Normal::Sequence(vec![
        predicate(RdfVocab::rdf_type(), false),
        Normal::ZeroOrMore(Box::new(predicate(RdfsVocab::rdfs_subclass_of_str(), false))),
    ])
}

fn outside(shape: &IRShape, component: &IRComponent) -> DenoteError {
    DenoteError::Internal(format!(
        "{} on {}, outside the fragments profile",
        IriS::from(component),
        shape.id()
    ))
}

impl<'a> Fragmenter<'_, 'a> {
    fn op(&mut self, op: Op) -> Result<RelId, DenoteError> {
        self.d.op(op)
    }

    /// The distinct values of a pair relation.
    fn value_set(&mut self, pairs: RelId) -> Result<RelId, DenoteError> {
        let values = self.op(Op::Values(pairs))?;
        self.op(Op::Distinct(values))
    }

    /// The nodes of `nodes` that are not in `fails`.
    fn keep(&mut self, nodes: RelId, fails: Option<RelId>) -> Result<RelId, DenoteError> {
        match fails {
            None => Ok(nodes),
            Some(fails) => self.op(Op::Filter(nodes, !Pred::Member(Expr::f(), fails))),
        }
    }

    /// The nodes of `nodes` that are in `fails`.
    fn only(&mut self, nodes: RelId, fails: RelId) -> Result<RelId, DenoteError> {
        self.op(Op::Filter(nodes, Pred::Member(Expr::f(), fails)))
    }

    fn components<'s>(&'s mut self, shape: &'a IRShape, focus: RelId, values: RelId) -> Components<'s, 'a> {
        Components {
            d: self.d,
            shape,
            focus,
            values,
            component: ShaclVocab::sh_qualified_min_count_constraint_component(),
        }
    }

    // --- walks ---------------------------------------------------------------

    /// The triples of every walk along `path` from the focus to the value of
    /// a pair of `ends`, a subset of the path's pairs. → Triples
    fn trace(&mut self, path: &Normal, ends: RelId) -> Result<RelId, DenoteError> {
        match path {
            Normal::Predicate { iri, inverse } => self.op(Op::Arcs {
                pairs: ends,
                predicate: iri.clone(),
                inverse: *inverse,
            }),
            Normal::Sequence(steps) => match steps.as_slice() {
                [] => self.op(Op::Empty(Sort::Triples)),
                [one] => self.trace(one, ends),
                [first, rest @ ..] => {
                    let rest = Normal::Sequence(rest.to_vec());
                    let start = self.start(ends)?;
                    let left = self.d.path(first, Start::Nodes(start))?;
                    let middle = self.value_set(left)?;
                    let right = self.d.path(&rest, Start::Nodes(middle))?;
                    let firsts = self.via(left, right, ends, true)?;
                    let rests = self.via(left, right, ends, false)?;
                    let parts = vec![self.trace(first, firsts)?, self.trace(&rest, rests)?];
                    self.d.union(parts, Sort::Triples)
                },
            },
            Normal::Alternative(members) => {
                let mut parts = Vec::new();
                for member in members {
                    let ends = self.within(member, ends)?;
                    parts.push(self.trace(member, ends)?);
                }
                self.d.union(parts, Sort::Triples)
            },
            Normal::ZeroOrOne(inner) => {
                let ends = self.within(inner, ends)?;
                self.trace(inner, ends)
            },
            // Every step `(a, b)` of a walk `n E* a E b E* x`: the pairs
            // `(a, x)` of `E/E*` on a walk from `n`, then the steps of those.
            Normal::ZeroOrMore(inner) | Normal::OneOrMore(inner) => {
                let star = Normal::ZeroOrMore(inner.clone());
                let plus = Normal::Sequence(vec![(**inner).clone(), star.clone()]);
                let start = self.start(ends)?;
                let reached = self.d.path(&star, Start::Nodes(start))?;
                let from = self.value_set(reached)?;
                let onward = self.d.path(&plus, Start::Nodes(from))?;
                let onward = self.via(reached, onward, ends, false)?;
                let start = self.start(onward)?;
                let step = self.d.path(inner, Start::Nodes(start))?;
                let next = self.value_set(step)?;
                let rest = self.d.path(&star, Start::Nodes(next))?;
                let steps = self.via(step, rest, onward, true)?;
                self.trace(inner, steps)
            },
        }
    }

    /// The distinct focus nodes of a pair or rows relation.
    fn start(&mut self, pairs: RelId) -> Result<RelId, DenoteError> {
        let focus = self.op(Op::Focus(pairs))?;
        self.op(Op::Distinct(focus))
    }

    /// The pairs of `ends` that `path` relates.
    fn within(&mut self, path: &Normal, ends: RelId) -> Result<RelId, DenoteError> {
        let start = self.start(ends)?;
        let pairs = self.d.path(path, Start::Nodes(start))?;
        self.op(Op::Filter(ends, Pred::PairMember(Expr::f(), Expr::v(), pairs)))
    }

    /// The pairs of `left` (or of `right`) on a walk `a left m right b` with
    /// `(a, b)` in `ends`, distinct.
    fn via(&mut self, left: RelId, right: RelId, ends: RelId, keep_left: bool) -> Result<RelId, DenoteError> {
        let back = self.op(Op::Inverse(left))?;
        let kept = if keep_left {
            // (m, a) of left with some (m, b) of right, (a, b) in ends.
            let joined = self.op(Op::PairJoin {
                left: back,
                right,
                pred: Pred::PairMember(Expr::v(), Expr::o(), ends),
            })?;
            self.op(Op::Inverse(joined))?
        } else {
            self.op(Op::PairJoin {
                left: right,
                right: back,
                pred: Pred::PairMember(Expr::o(), Expr::v(), ends),
            })?
        };
        self.op(Op::Distinct(kept))
    }

    /// The walks along `shape`'s path for the pairs `pairs`; none for a node
    /// shape, whose path is the identity.
    fn along(&mut self, shape: &IRShape, pairs: RelId) -> Result<Vec<RelId>, DenoteError> {
        match shape.path() {
            None => Ok(Vec::new()),
            Some(path) => Ok(vec![self.trace(&normalise(path, false), pairs)?]),
        }
    }

    /// The walks along `path` for the pairs `pairs`.
    fn other(&mut self, path: &SHACLPath, pairs: RelId) -> Result<RelId, DenoteError> {
        self.trace(&normalise(path, false), pairs)
    }

    /// The `rdf:type/rdfs:subClassOf*` walks of `nodes` to one of `classes`,
    /// or to any class.
    fn typed(&mut self, nodes: RelId, classes: Option<&[IriS]>) -> Result<RelId, DenoteError> {
        let path = typed();
        let pairs = self.d.path(&path, Start::Nodes(nodes))?;
        let ends = match classes {
            None => pairs,
            Some(classes) => self.op(Op::Filter(
                pairs,
                Pred::or(
                    classes
                        .iter()
                        .map(|c| Pred::Same(Expr::v(), Expr::Const(Object::Iri(c.clone()))))
                        .collect(),
                ),
            ))?,
        };
        self.trace(&path, ends)
    }

    // --- targets and shapes --------------------------------------------------

    /// The triples that make each node of `nodes` a target of `target`.
    fn target(&mut self, target: &Target, nodes: RelId) -> Result<Vec<RelId>, DenoteError> {
        let walks = |f: &mut Self, path: Normal| -> Result<Vec<RelId>, DenoteError> {
            let pairs = f.d.path(&path, Start::Nodes(nodes))?;
            Ok(vec![f.trace(&path, pairs)?])
        };
        match target {
            Target::Node(_) => Ok(Vec::new()),
            Target::Class(Object::Iri(class)) | Target::ImplicitClass(Object::Iri(class)) => {
                Ok(vec![self.typed(nodes, Some(std::slice::from_ref(class)))?])
            },
            Target::Class(_) | Target::ImplicitClass(_) => Ok(Vec::new()),
            Target::SubjectsOf(p) => walks(self, predicate(p.clone(), false)),
            Target::ObjectsOf(p) => walks(self, predicate(p.clone(), true)),
            other => Err(DenoteError::Internal(format!("{other}: outside the fragments profile"))),
        }
    }

    /// The neighbourhood of `idx` for the nodes `nodes`, which conform to it,
    /// or, when `negated`, fail it.
    fn shape(&mut self, idx: ShapeLabelIdx, nodes: RelId, negated: bool) -> Result<Vec<RelId>, DenoteError> {
        if let Some(memo) = self.memo.get(&(idx, nodes, negated)) {
            return Ok(memo.clone());
        }
        let shape = self.d.shape(idx)?;
        if shape.deactivated() {
            return Ok(Vec::new());
        }
        if let Some(u) = self.d.unchecked.get(&idx) {
            return Err(DenoteError::Internal(format!(
                "the fragment of {}, outside the profile",
                u.shape
            )));
        }
        let values = self.d.values(shape, nodes)?;
        let mut parts = Vec::new();
        if negated {
            let mut rows: HashMap<usize, Vec<Rows>> = HashMap::new();
            for (index, r) in self.d.component_rows(shape, nodes, values)? {
                if let Some(index) = index {
                    rows.entry(index).or_default().push(r);
                }
            }
            for (index, component) in shape.components().iter().enumerate() {
                let rows = rows.remove(&index).unwrap_or_default();
                parts.extend(self.failed(shape, component, nodes, values, rows)?);
            }
        } else {
            for component in shape.components() {
                parts.extend(self.held(shape, component, nodes, values)?);
            }
        }
        for nested in shape.property_shapes() {
            match shape {
                IRShape::NodeShape(_) if !negated => parts.extend(self.shape(*nested, nodes, false)?),
                IRShape::NodeShape(_) => {
                    if let Some(fails) = self.d.fails(*nested, nodes)? {
                        let fails = self.op(Op::Distinct(fails))?;
                        parts.extend(self.shape(*nested, fails, true)?);
                    }
                },
                IRShape::PropertyShape(_) => {
                    let reached = self.value_set(values)?;
                    if !negated {
                        parts.extend(self.along(shape, values)?);
                        parts.extend(self.shape(*nested, reached, false)?);
                    } else if let Some(fails) = self.d.fails(*nested, reached)? {
                        let hit = self.op(Op::Filter(values, Pred::Member(Expr::v(), fails)))?;
                        parts.extend(self.along(shape, hit)?);
                        let failing = self.value_set(hit)?;
                        parts.extend(self.shape(*nested, failing, true)?);
                    }
                },
            }
        }
        parts.sort_unstable();
        parts.dedup();
        self.memo.insert((idx, nodes, negated), parts.clone());
        Ok(parts)
    }

    /// The neighbourhood of a component that `nodes`, with the value pairs
    /// `values`, conform to.
    fn held(
        &mut self,
        shape: &'a IRShape,
        component: &IRComponent,
        nodes: RelId,
        values: RelId,
    ) -> Result<Vec<RelId>, DenoteError> {
        use IRComponent as C;
        let mut out = Vec::new();
        let x = self.value_set(values)?;
        match component {
            C::Datatype(_)
            | C::NodeKind(_)
            | C::MinCount(_)
            | C::MinExclusive(_)
            | C::MaxExclusive(_)
            | C::MinInclusive(_)
            | C::MaxInclusive(_)
            | C::MinLength(_)
            | C::MaxLength(_)
            | C::Pattern(_)
            | C::SingleLine(_)
            | C::LanguageIn(_)
            | C::In(_) => out.extend(self.along(shape, values)?),
            C::Class(classes) => {
                out.extend(self.along(shape, values)?);
                out.push(self.typed(x, Some(classes))?);
            },
            C::MaxCount(_)
            | C::UniqueLang(_)
            | C::Disjoint(_)
            | C::LessThan(_)
            | C::LessThanOrEquals(_)
            | C::Closed(_)
            | C::Deactivated(_) => {},
            C::HasValue(value) => {
                let hit = self.op(Op::Filter(values, Pred::Same(Expr::v(), Expr::Const(value.clone()))))?;
                out.extend(self.along(shape, hit)?);
            },
            C::Equals(path) => {
                out.extend(self.along(shape, values)?);
                let other = self.components(shape, nodes, values).other(path)?;
                out.push(self.other(path, other)?);
            },
            C::Node(s) => {
                out.extend(self.along(shape, values)?);
                out.extend(self.shape(*s, x, false)?);
            },
            C::Not(s) => {
                out.extend(self.along(shape, values)?);
                out.extend(self.shape(*s, x, true)?);
            },
            C::And(shapes) => {
                out.extend(self.along(shape, values)?);
                for s in shapes {
                    out.extend(self.shape(*s, x, false)?);
                }
            },
            C::Or(shapes) | C::Xone(shapes) => {
                out.extend(self.along(shape, values)?);
                for s in shapes {
                    let fails = self.d.fails(*s, x)?;
                    let conforming = self.keep(x, fails)?;
                    out.extend(self.shape(*s, conforming, false)?);
                    // Exactly one conforms: the others fail.
                    if let (C::Xone(_), Some(fails)) = (component, fails) {
                        let fails = self.op(Op::Distinct(fails))?;
                        out.extend(self.shape(*s, fails, true)?);
                    }
                }
            },
            C::QualifiedValueShape(qvs) => {
                if qvs.qualified_min_count().is_some() {
                    out.extend(self.qualified(shape, qvs, nodes, values, values, true)?);
                }
                if qvs.qualified_max_count().is_some() {
                    out.extend(self.qualified(shape, qvs, nodes, values, values, false)?);
                }
            },
            other => return Err(outside(shape, other)),
        }
        Ok(out)
    }

    /// The neighbourhood of a component that the nodes of `rows` fail.
    fn failed(
        &mut self,
        shape: &'a IRShape,
        component: &IRComponent,
        nodes: RelId,
        values: RelId,
        rows: Vec<Rows>,
    ) -> Result<Vec<RelId>, DenoteError> {
        use IRComponent as C;
        let mut out = Vec::new();
        let Some(all) = rows.first().map(|r| r.rows) else {
            return Ok(out);
        };
        // The violating values, and the failing focus nodes.
        let hit = self.op(Op::RowPairs(all))?;
        let x = self.value_set(hit)?;
        let failing = self.start(all)?;
        match component {
            C::Datatype(_)
            | C::NodeKind(_)
            | C::MinExclusive(_)
            | C::MaxExclusive(_)
            | C::MinInclusive(_)
            | C::MaxInclusive(_)
            | C::MinLength(_)
            | C::MaxLength(_)
            | C::Pattern(_)
            | C::SingleLine(_)
            | C::LanguageIn(_)
            | C::In(_) => out.extend(self.along(shape, hit)?),
            C::Class(_) => {
                out.extend(self.along(shape, hit)?);
                out.push(self.typed(x, None)?);
            },
            C::MinCount(_) | C::Deactivated(_) => {},
            C::MaxCount(_) | C::UniqueLang(_) | C::HasValue(_) => {
                let pairs = self.op(Op::Restrict {
                    pairs: values,
                    nodes: failing,
                })?;
                out.extend(self.along(shape, pairs)?);
            },
            C::Equals(path) => {
                let other = self.components(shape, nodes, values).other(path)?;
                let missing = self.op(Op::Filter(values, !Pred::PairMember(Expr::f(), Expr::v(), other)))?;
                let extra = self.op(Op::Filter(other, !Pred::PairMember(Expr::f(), Expr::v(), values)))?;
                out.extend(self.along(shape, missing)?);
                out.push(self.other(path, extra)?);
            },
            C::Disjoint(path) => {
                out.extend(self.along(shape, hit)?);
                out.push(self.other(path, hit)?);
            },
            C::LessThan(path) | C::LessThanOrEquals(path) => {
                let op = match component {
                    C::LessThan(_) => CmpOp::Lt,
                    _ => CmpOp::LtEq,
                };
                let other = self.components(shape, nodes, values).other(path)?;
                // The other path's values `y` with some value `x` of this one
                // where `x op y` does not hold.
                let ys = self.op(Op::PairJoin {
                    left: other,
                    right: values,
                    pred: !Pred::Compare(Expr::o(), op, Expr::v()),
                })?;
                let ys = self.op(Op::Distinct(ys))?;
                out.extend(self.along(shape, hit)?);
                out.push(self.other(path, ys)?);
            },
            C::Closed(closed) => {
                // The value nodes' own triples, as rows `(v, o, p)`.
                let reached = self.value_set(values)?;
                let own = self.op(Op::Identity(reached))?;
                if let Some(refused) = self.components(shape, nodes, values).closed_rows(closed, own)? {
                    out.push(self.op(Op::RowTriples(refused))?);
                    let hit = self.op(Op::Filter(values, Pred::Member(Expr::v(), refused)))?;
                    out.extend(self.along(shape, hit)?);
                }
            },
            C::Node(s) => {
                out.extend(self.along(shape, hit)?);
                out.extend(self.shape(*s, x, true)?);
            },
            C::Not(s) => {
                out.extend(self.along(shape, hit)?);
                out.extend(self.shape(*s, x, false)?);
            },
            C::And(shapes) => {
                out.extend(self.along(shape, hit)?);
                for s in shapes {
                    if let Some(fails) = self.d.fails(*s, x)? {
                        let fails = self.op(Op::Distinct(fails))?;
                        out.extend(self.shape(*s, fails, true)?);
                    }
                }
            },
            C::Or(shapes) => {
                out.extend(self.along(shape, hit)?);
                for s in shapes {
                    out.extend(self.shape(*s, x, true)?);
                }
            },
            C::Xone(shapes) => {
                // None conforms, or more than one does.
                out.extend(self.along(shape, hit)?);
                let mut none = Vec::new();
                for s in shapes {
                    let fails = self.d.fails(*s, x)?;
                    let conforming = self.keep(x, fails)?;
                    out.extend(self.shape(*s, conforming, false)?);
                    none.push(Pred::member(Expr::f(), fails));
                }
                let none = self.op(Op::Filter(x, Pred::and(none)))?;
                for s in shapes {
                    out.extend(self.shape(*s, none, true)?);
                }
            },
            C::QualifiedValueShape(qvs) => {
                for r in &rows {
                    let failing = self.start(r.rows)?;
                    let pairs = self.op(Op::Restrict {
                        pairs: values,
                        nodes: failing,
                    })?;
                    let min = r.component == ShaclVocab::sh_qualified_min_count_constraint_component();
                    out.extend(self.qualified(shape, qvs, nodes, values, pairs, !min)?);
                }
            },
            other => return Err(outside(shape, other)),
        }
        Ok(out)
    }

    /// The neighbourhood of the pairs of `pairs` whose value a qualified value
    /// shape counts, when `counted`, or does not.
    fn qualified(
        &mut self,
        shape: &'a IRShape,
        qvs: &QualifiedValueShape,
        nodes: RelId,
        values: RelId,
        pairs: RelId,
        counted: bool,
    ) -> Result<Vec<RelId>, DenoteError> {
        let mut c = self.components(shape, nodes, values);
        let pred = c.counted(qvs)?;
        let fails = c.fails(*qvs.shape())?;
        let mut siblings = Vec::new();
        for sibling in qvs.siblings() {
            siblings.push((*sibling, c.fails(*sibling)?));
        }
        let hit = self.op(Op::Filter(pairs, if counted { pred } else { !pred }))?;
        let x = self.value_set(hit)?;
        let mut out = self.along(shape, hit)?;
        if counted {
            // Conforms to the shape, and to none of the siblings.
            out.extend(self.shape(*qvs.shape(), x, false)?);
            for (sibling, _) in siblings {
                out.extend(self.shape(sibling, x, true)?);
            }
        } else {
            // Fails the shape, or conforms to a sibling.
            if let Some(fails) = fails {
                let failing = self.only(x, fails)?;
                out.extend(self.shape(*qvs.shape(), failing, true)?);
            }
            for (sibling, fails) in siblings {
                let conforming = self.keep(x, fails)?;
                out.extend(self.shape(sibling, conforming, false)?);
            }
        }
        Ok(out)
    }
}
