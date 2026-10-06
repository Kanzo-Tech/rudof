//! SHACL validation as relational algebra.
//!
//! SHACL Core validation is a query problem: the focus nodes of a shape, the
//! value nodes of a path, and the results of each constraint component are
//! relations over the data graph. This module states that once. [`denote`]
//! turns a shapes graph ([`IRSchema`]) into a [`Plan`]: a DAG of [`Op`]s, one
//! root per check. Interpretations run the plan and know nothing of SHACL:
//! [`crate::validator::sql`] renders it as SQL over a relational mapping, and
//! [`crate::validator::eval`] evaluates it over an in-memory graph. Both read
//! the same plan, so they cannot disagree about what SHACL means; the W3C
//! suite runs through both and holds their reports equal.
//!
//! The operators are those of a set-based reading of SHACL (Corman, Florenzano,
//! Reutter and Savković, *Validating SHACL constraints over a SPARQL endpoint*,
//! ISWC 2019), shaped after the plan nodes of RDF4J's ShaclSail (select, join,
//! filter, union, distinct, group). The scalar tests ([`Pred`], [`Expr`]) are
//! SPARQL 1.1 filter operators (§17): `isIRI`, `datatype`, `lang`,
//! `langMatches`, `regex`, `strlen`, `sameTerm` and the ordering operators.
//!
//! # Sorts
//!
//! Every relation has a [`Sort`], checked when the plan is built:
//!
//! | sort | columns |
//! |---|---|
//! | [`Sort::Nodes`] | `f` |
//! | [`Sort::Pairs`] | `f`, `v` |
//! | [`Sort::Rows`] | `f`, `v` (absent when the result has no value), `path` (absent unless a row overrides the check's path) |
//! | [`Sort::Triples`] | `f` (subject), `p` (predicate), `v` (object) |
//!
//! A relation is a **bag**: [`Op::Union`] and the projections keep duplicates,
//! [`Op::Distinct`] removes them. The results of a nested property shape repeat
//! once per path that reaches its focus node, as SHACL's report does.
//!
//! # Sharing
//!
//! A plan is an arena. [`PlanBuilder::add`] returns the existing [`RelId`] for
//! an operator already in the plan, so a relation that two checks need (a class
//! extent, a path, the nodes failing a shape) exists once. The SQL
//! interpretation renders it as one CTE; the evaluator computes it once.

pub mod denote;

use crate::ir::ShapeLabelIdx;
use crate::types::Severity;
use rudof_iri::IriS;
use rudof_rdf::SHACLPath;
use rudof_rdf::term::Object;
use rudof_rdf::term::literal::Lang;
use std::collections::HashMap;
use std::fmt::{Display, Formatter};

pub use denote::{DenoteError, denote};

/// A relation of a [`Plan`]: an index into its arena.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RelId(usize);

impl RelId {
    pub fn index(self) -> usize {
        self.0
    }
}

impl Display for RelId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "r{}", self.0)
    }
}

/// The columns of a relation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Sort {
    Nodes,
    Pairs,
    Rows,
    Triples,
}

/// A column of the row a [`Pred`] reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Col {
    /// The focus node (`f`).
    F,
    /// The value node (`v`).
    V,
    /// In [`Op::PairJoin`], the right relation's value.
    O,
}

/// A term-valued expression.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Expr {
    Col(Col),
    Const(Object),
}

impl Expr {
    pub fn f() -> Self {
        Expr::Col(Col::F)
    }
    pub fn v() -> Self {
        Expr::Col(Col::V)
    }
    pub fn o() -> Self {
        Expr::Col(Col::O)
    }
}

/// An ordering or equality operator of SPARQL 1.1 (§17.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CmpOp {
    Lt,
    LtEq,
    Gt,
    GtEq,
}

/// The kind of an RDF term.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Iri,
    Blank,
    Literal,
}

/// A row test. Three-valued like SPARQL's `FILTER`: an incomparable or
/// ill-typed comparison is an error, and a row passes a filter only when its
/// predicate is true.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Pred {
    True,
    And(Vec<Pred>),
    Or(Vec<Pred>),
    Not(Box<Pred>),
    /// Exactly one of the predicates holds.
    ExactlyOne(Vec<Pred>),
    /// `isIRI`, `isBlank`, `isLiteral`: the term is of one of the kinds.
    KindIn(Expr, Vec<Kind>),
    /// The term is a literal whose datatype is one of these, and whose lexical
    /// form lies in that datatype's lexical space (for the XSD datatypes a
    /// processor checks; any other is well-formed).
    Datatype(Expr, Vec<IriS>),
    /// `lang(e) != ""` and `langMatches(lang(e), r)` for one of the ranges.
    LangIn(Expr, Vec<Lang>),
    /// `regex(str(e), pattern, flags)`, false for a blank node.
    Regex(Expr, String, Option<String>),
    /// `strlen(str(e)) op n`, false for a blank node.
    StrLen(Expr, CmpOp, i64),
    /// `a op b` under SPARQL's operator mapping for RDF terms; false when the
    /// terms are incomparable.
    Compare(Expr, CmpOp, Expr),
    /// `sameTerm(a, b)`.
    Same(Expr, Expr),
    /// The term is a focus node of the relation (a semi-join).
    Member(Expr, RelId),
    /// The pair `(a, b)` is in the pair relation.
    PairMember(Expr, Expr, RelId),
}

impl Pred {
    /// The predicate that never holds.
    pub fn never() -> Pred {
        Pred::Or(Vec::new())
    }

    /// `e` is a focus node of `rel`; never, when there is no relation.
    pub fn member(e: Expr, rel: Option<RelId>) -> Pred {
        match rel {
            Some(r) => Pred::Member(e, r),
            None => Pred::never(),
        }
    }

    pub fn not(p: Pred) -> Pred {
        match p {
            Pred::Not(inner) => *inner,
            other => Pred::Not(Box::new(other)),
        }
    }

    pub fn and(ps: Vec<Pred>) -> Pred {
        let ps: Vec<Pred> = ps.into_iter().filter(|p| *p != Pred::True).collect();
        match ps.len() {
            0 => Pred::True,
            1 => ps.into_iter().next().expect("one"),
            _ => Pred::And(ps),
        }
    }

    pub fn or(ps: Vec<Pred>) -> Pred {
        if ps.len() == 1 {
            ps.into_iter().next().expect("one")
        } else {
            Pred::Or(ps)
        }
    }
}

/// A relational operator. Each variant documents its input and output sorts.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Op {
    /// The empty relation of a sort.
    Empty(Sort),
    /// Constant nodes. → Nodes
    Constants(Vec<Object>),
    /// The `(subject, object)` pairs of a predicate, distinct. → Pairs
    Predicate(IriS),
    /// The SHACL instances of a class (SHACL §1.1: `rdf:type` and the
    /// `rdfs:subClassOf*` closure). → Nodes, distinct
    Class(IriS),
    /// Every triple of the data, distinct. → Triples
    Triples,
    /// Every node of the data: the subjects and objects of its triples. → Nodes
    AllNodes,
    /// Bag union of relations of one sort.
    Union(Vec<RelId>),
    /// Duplicates removed.
    Distinct(RelId),
    /// The focus column. Pairs | Rows → Nodes (bag)
    Focus(RelId),
    /// The value column. Pairs → Nodes (bag)
    Values(RelId),
    /// `(n, n)` for each node. Nodes → Pairs
    Identity(RelId),
    /// `(v, f)` for each pair. Pairs → Pairs
    Inverse(RelId),
    /// The pairs whose focus is a node of `nodes` (a semi-join). Pairs, Nodes → Pairs
    Restrict { pairs: RelId, nodes: RelId },
    /// Relational composition `a ∘ b`: `(a.f, b.v)` where `a.v = b.f`, distinct.
    /// Pairs, Pairs → Pairs
    Compose(RelId, RelId),
    /// The transitive closure of `step` from the pairs of `base`: `base`, then
    /// `step` applied until nothing new is reached. Pairs, Pairs → Pairs
    Closure { base: RelId, step: RelId },
    /// The rows that satisfy `pred`. Any sort, preserved.
    Filter(RelId, Pred),
    /// Pairs → Rows: `(f, v, —)`, or `(f, —, —)` when `!with_value`.
    PairRows { pairs: RelId, with_value: bool },
    /// Nodes → Rows: `(n, —, —)`.
    NodeRows(RelId),
    /// Every pair or row of `rel` repeated once per occurrence of its focus
    /// node in the bag `bag`. Pairs | Rows, Nodes → the sort of `rel`
    Repeat { rel: RelId, bag: RelId },
    /// Per focus node of `focus`, the number of pairs of `pairs` with that
    /// focus that satisfy `counted` (0 when none); the focus nodes whose count
    /// `n` satisfies `n op bound`. Nodes, Pairs → Rows `(f, —, —)`
    Count {
        focus: RelId,
        pairs: RelId,
        counted: Pred,
        op: CmpOp,
        bound: i64,
    },
    /// The focus nodes that have more than one value literal with the same
    /// non-empty language tag, compared case-insensitively, once per such tag.
    /// Pairs → Rows `(f, —, —)`
    LangDuplicates(RelId),
    /// For each pair `(f, v)` of `pairs`, each triple `(v, p, o)` of `triples`
    /// whose predicate is not in `allowed`: the row `(f, o, p)`.
    /// Pairs, Triples → Rows
    Outgoing {
        pairs: RelId,
        triples: RelId,
        allowed: Vec<IriS>,
    },
    /// For each pair `(f, v)` of `left` and each pair `(f, o)` of `right` with
    /// the same focus such that `pred` (over `F`, `V`, `O`) holds: `(f, v, —)`.
    /// Pairs, Pairs → Rows
    PairJoin { left: RelId, right: RelId, pred: Pred },
}

impl Op {
    /// The operator's inputs.
    pub fn inputs(&self) -> Vec<RelId> {
        let mut out = match self {
            Op::Empty(_) | Op::Constants(_) | Op::Predicate(_) | Op::Class(_) | Op::Triples | Op::AllNodes => {
                Vec::new()
            },
            Op::Union(rels) => rels.clone(),
            Op::Distinct(r)
            | Op::Focus(r)
            | Op::Values(r)
            | Op::Identity(r)
            | Op::Inverse(r)
            | Op::Filter(r, _)
            | Op::NodeRows(r)
            | Op::LangDuplicates(r) => vec![*r],
            Op::PairRows { pairs, .. } => vec![*pairs],
            Op::Outgoing { pairs, triples, .. } => vec![*pairs, *triples],
            Op::Restrict { pairs, nodes } => vec![*pairs, *nodes],
            Op::Compose(a, b) => vec![*a, *b],
            Op::Closure { base, step } => vec![*base, *step],
            Op::Repeat { rel, bag } => vec![*rel, *bag],
            Op::Count { focus, pairs, .. } => vec![*focus, *pairs],
            Op::PairJoin { left, right, .. } => vec![*left, *right],
        };
        if let Some(p) = self.pred() {
            p.relations(&mut out);
        }
        out
    }

    fn pred(&self) -> Option<&Pred> {
        match self {
            Op::Filter(_, p) | Op::PairJoin { pred: p, .. } | Op::Count { counted: p, .. } => Some(p),
            _ => None,
        }
    }
}

impl Pred {
    /// The relations the predicate reads (by membership).
    pub fn relations(&self, out: &mut Vec<RelId>) {
        match self {
            Pred::And(ps) | Pred::Or(ps) | Pred::ExactlyOne(ps) => ps.iter().for_each(|p| p.relations(out)),
            Pred::Not(p) => p.relations(out),
            Pred::Member(_, r) | Pred::PairMember(_, _, r) => out.push(*r),
            _ => {},
        }
    }
}

/// A check: the rows of one constraint component of one shape, in one context
/// (a targeted shape, or a property shape reached from one).
#[derive(Debug, Clone)]
pub struct Check {
    /// The shape that declares the component: the results' `sh:sourceShape`.
    pub shape: ShapeLabelIdx,
    /// The results' `sh:sourceConstraintComponent`.
    pub component: IriS,
    /// The results' `sh:resultSeverity`.
    pub severity: Severity,
    /// The results' `sh:resultPath`, unless a row overrides it (`sh:closed`).
    pub path: Option<SHACLPath>,
    /// Message parameters: the index of the component in its shape, or the
    /// parameters the check names itself.
    pub parameters: Parameters,
    /// The rows: a relation of sort [`Sort::Rows`].
    pub rows: RelId,
}

/// Where a check's message parameters come from.
#[derive(Debug, Clone)]
pub enum Parameters {
    /// The component at this index in the shape's components.
    Component(usize),
    /// Named by the check (the two of `sh:qualifiedValueShape`).
    Own(Vec<(&'static str, String)>),
}

/// A shapes graph as relational algebra: the operators, and the checks whose
/// rows are the validation results.
#[derive(Debug, Clone, Default)]
pub struct Plan {
    ops: Vec<Op>,
    sorts: Vec<Sort>,
    pub checks: Vec<Check>,
}

impl Plan {
    pub fn op(&self, id: RelId) -> &Op {
        &self.ops[id.0]
    }

    pub fn sort(&self, id: RelId) -> Sort {
        self.sorts[id.0]
    }

    pub fn len(&self) -> usize {
        self.ops.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    /// Every relation, inputs before the relations that read them.
    pub fn ids(&self) -> impl Iterator<Item = RelId> {
        (0..self.ops.len()).map(RelId)
    }

    /// The relations `roots` reach, inputs first.
    pub fn reached(&self, roots: &[RelId]) -> Vec<RelId> {
        let mut seen = vec![false; self.ops.len()];
        let mut pending: Vec<RelId> = roots.to_vec();
        while let Some(id) = pending.pop() {
            if !seen[id.0] {
                seen[id.0] = true;
                pending.extend(self.ops[id.0].inputs());
            }
        }
        (0..self.ops.len()).filter(|i| seen[*i]).map(RelId).collect()
    }
}

/// Builds a [`Plan`], sharing identical operators.
#[derive(Default)]
pub struct PlanBuilder {
    plan: Plan,
    index: HashMap<String, RelId>,
}

/// A plan operator with the wrong input sorts: a bug in [`denote`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("ill-sorted plan: {0}")]
pub struct SortError(pub String);

impl PlanBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// The relation of `op`: an existing one when the plan has it already.
    ///
    /// Operators are keyed by their `Debug` text, which spells terms by their
    /// lexical form: two literals that compare equal by value (`1` and `01`)
    /// are different terms here, as they are in SQL.
    pub fn add(&mut self, op: Op) -> Result<RelId, SortError> {
        let key = format!("{op:?}");
        if let Some(id) = self.index.get(&key) {
            return Ok(*id);
        }
        let sort = self.sort_of(&op)?;
        let id = RelId(self.plan.ops.len());
        self.plan.ops.push(op);
        self.plan.sorts.push(sort);
        self.index.insert(key, id);
        Ok(id)
    }

    pub fn sort(&self, id: RelId) -> Sort {
        self.plan.sorts[id.0]
    }

    pub fn op(&self, id: RelId) -> &Op {
        &self.plan.ops[id.0]
    }

    pub fn check(&mut self, check: Check) {
        self.plan.checks.push(check);
    }

    pub fn finish(self) -> Plan {
        self.plan
    }

    fn expect(&self, id: RelId, sorts: &[Sort], op: &str) -> Result<(), SortError> {
        let sort = self.sort(id);
        if sorts.contains(&sort) {
            Ok(())
        } else {
            Err(SortError(format!("{op} reads {id}, of sort {sort:?}, not {sorts:?}")))
        }
    }

    fn sort_of(&self, op: &Op) -> Result<Sort, SortError> {
        use Sort::*;
        Ok(match op {
            Op::Empty(sort) => *sort,
            Op::Constants(_) | Op::Class(_) | Op::AllNodes => Nodes,
            Op::Predicate(_) => Pairs,
            Op::Triples => Triples,
            Op::Union(rels) => {
                let first = rels
                    .first()
                    .ok_or_else(|| SortError("an empty union".to_owned()))?;
                let sort = self.sort(*first);
                for r in rels {
                    self.expect(*r, &[sort], "union")?;
                }
                sort
            },
            Op::Distinct(r) | Op::Filter(r, _) => self.sort(*r),
            Op::Focus(r) => {
                self.expect(*r, &[Pairs, Rows], "focus")?;
                Nodes
            },
            Op::Values(r) => {
                self.expect(*r, &[Pairs], "values")?;
                Nodes
            },
            Op::Identity(r) => {
                self.expect(*r, &[Nodes], "identity")?;
                Pairs
            },
            Op::Inverse(r) => {
                self.expect(*r, &[Pairs], "inverse")?;
                Pairs
            },
            Op::Restrict { pairs, nodes } => {
                self.expect(*pairs, &[Pairs], "restrict")?;
                self.expect(*nodes, &[Nodes], "restrict")?;
                Pairs
            },
            Op::Compose(a, b) | Op::Closure { base: a, step: b } => {
                self.expect(*a, &[Pairs], "compose")?;
                self.expect(*b, &[Pairs], "compose")?;
                Pairs
            },
            Op::PairRows { pairs, .. } | Op::LangDuplicates(pairs) => {
                self.expect(*pairs, &[Pairs], "rows")?;
                Rows
            },
            Op::Outgoing { pairs, triples, .. } => {
                self.expect(*pairs, &[Pairs], "outgoing")?;
                self.expect(*triples, &[Triples], "outgoing")?;
                Rows
            },
            Op::NodeRows(r) => {
                self.expect(*r, &[Nodes], "node rows")?;
                Rows
            },
            Op::Repeat { rel, bag } => {
                self.expect(*rel, &[Pairs, Rows], "repeat")?;
                self.expect(*bag, &[Nodes], "repeat")?;
                self.sort(*rel)
            },
            Op::Count { focus, pairs, .. } => {
                self.expect(*focus, &[Nodes], "count")?;
                self.expect(*pairs, &[Pairs], "count")?;
                Rows
            },
            Op::PairJoin { left, right, .. } => {
                self.expect(*left, &[Pairs], "pair join")?;
                self.expect(*right, &[Pairs], "pair join")?;
                Rows
            },
        })
    }
}
