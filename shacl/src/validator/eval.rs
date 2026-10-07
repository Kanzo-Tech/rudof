//! The in-memory interpretation of a [`Plan`]: every relation computed once,
//! set-at-a-time, over a graph the process holds.
//!
//! It reads the graph through [`NeighsRDF`] and nothing else: the triples of
//! a predicate, and every triple. Terms stay `oxrdf::Term`s, whose equality is
//! `sameTerm` (`"1"^^xsd:integer` and `"01"^^xsd:integer` are two terms), as in
//! SQL; they become [`Object`]s only where a test reads their value
//! (comparisons, datatypes, languages) and in the report.

use crate::algebra::{
    self, Check, CmpOp, Col, Expr, Key, Kind, Op, Plan, Pred, RelId, Unchecked, denote, denote_shape,
};
use crate::error::ValidationError;
use crate::ir::{IRSchema, ShapeLabelIdx};
use crate::validator::report::{ValidationReport, ValidationResult};
use oxrdf::{NamedNode, NamedOrBlankNode, Term, Triple};
use rudof_iri::IriS;
use rudof_rdf::NeighsRDF;
use rudof_rdf::term::Object;
use rudof_rdf::term::Triple as _;
use rudof_rdf::term::literal::ConcreteLiteral;
use rudof_rdf::utils::RDFRegex;
use rudof_rdf::vocab::{RdfVocab, RdfsVocab};
use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};

/// A row of a check: its focus node, its value (when the result has one) and
/// the predicate that overrides the check's path (`sh:closed`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Row {
    pub focus: Term,
    pub value: Option<Term>,
    pub path: Option<NamedNode>,
}

/// Why a plan could not be evaluated over a graph.
#[derive(Debug, Clone, thiserror::Error)]
#[error("evaluating {relation}: {message}")]
pub struct EvalError {
    pub relation: String,
    pub message: String,
}

/// A relation's value, by sort.
#[derive(Debug, Clone)]
enum Rel {
    Nodes(Vec<Term>),
    Pairs(Vec<(Term, Term)>),
    Rows(Vec<Row>),
    Triples(Vec<(Term, NamedNode, Term)>),
}

impl Rel {
    /// The focus column, whatever the sort.
    fn focus(&self) -> Vec<&Term> {
        match self {
            Rel::Nodes(ns) => ns.iter().collect(),
            Rel::Pairs(ps) => ps.iter().map(|(f, _)| f).collect(),
            Rel::Rows(rs) => rs.iter().map(|r| &r.focus).collect(),
            Rel::Triples(ts) => ts.iter().map(|(s, _, _)| s).collect(),
        }
    }
}

fn distinct<T: Clone + Eq + std::hash::Hash>(items: &[T]) -> Vec<T> {
    let mut seen = HashSet::new();
    items.iter().filter(|i| seen.insert((*i).clone())).cloned().collect()
}

/// The term as a value, for the tests that read one. A literal the reader
/// cannot represent has no value, and every test of it fails.
fn object(term: &Term) -> Option<Object> {
    Object::try_from(term.clone()).ok()
}

fn kind_of(term: &Term) -> Kind {
    match term {
        Term::NamedNode(_) => Kind::Iri,
        Term::BlankNode(_) => Kind::Blank,
        Term::Literal(_) => Kind::Literal,
        Term::Triple(_) => Kind::TripleTerm,
    }
}

/// The text `str()` reads: an IRI's, or a literal's lexical form.
fn text(term: &Term) -> Option<&str> {
    match term {
        Term::NamedNode(n) => Some(n.as_str()),
        Term::Literal(l) => Some(l.value()),
        _ => None,
    }
}

fn holds(ordering: Ordering, op: CmpOp) -> bool {
    match op {
        CmpOp::Lt => ordering.is_lt(),
        CmpOp::LtEq => ordering.is_le(),
        CmpOp::Gt => ordering.is_gt(),
        CmpOp::GtEq => ordering.is_ge(),
    }
}

/// The columns a predicate reads.
struct Scope<'r> {
    f: &'r Term,
    v: Option<&'r Term>,
    o: Option<&'r Term>,
}

/// Validates `store` against `schema`: the plan of the shapes graph, evaluated
/// over the graph, as a report.
pub fn validate<S>(schema: &IRSchema, store: &S) -> Result<ValidationReport, ValidationError>
where
    S: NeighsRDF<Term = Term>,
{
    report(schema, &denote(schema, false)?, store, None)
}

/// Validates `store` against `schema` with the focus nodes of every shape
/// restricted to `focus`: its targets among them, against the whole graph.
pub fn validate_scoped<S>(schema: &IRSchema, store: &S, focus: &[Object]) -> Result<ValidationReport, ValidationError>
where
    S: NeighsRDF<Term = Term>,
{
    let focus: Vec<Term> = focus.iter().map(|n| Term::from(n.clone())).collect();
    report(schema, &denote(schema, true)?, store, Some(&focus))
}

/// Validates `store` against one shape and its property shapes: `focus` when
/// given, otherwise the shape's own targets.
pub fn validate_shape<S>(
    schema: &IRSchema,
    store: &S,
    shape: ShapeLabelIdx,
    focus: Option<&Object>,
) -> Result<ValidationReport, ValidationError>
where
    S: NeighsRDF<Term = Term>,
{
    report(schema, &denote_shape(schema, shape, focus)?, store, None)
}

/// The report of `plan`'s rows over `store`.
fn report<S>(
    schema: &IRSchema,
    plan: &Plan,
    store: &S,
    scope: Option<&[Term]>,
) -> Result<ValidationReport, ValidationError>
where
    S: NeighsRDF<Term = Term>,
{
    let rows = evaluate(plan, store, scope)?;
    let mut results = Vec::new();
    for (check, rows) in plan.checks.iter().zip(rows) {
        let err = |message: String| EvalError {
            relation: check.rows.to_string(),
            message,
        };
        for row in rows {
            let as_object = |t: Term| Object::try_from(t).map_err(|e| err(e.to_string()));
            let focus = as_object(row.focus)?;
            let value = row.value.map(as_object).transpose()?;
            let path = row.path.map(|p| IriS::new_unchecked(p.as_str()));
            results.push(ValidationResult::of(schema, check, focus, value, path).map_err(err)?);
        }
    }
    let mut prefixes = schema.prefix_map().clone();
    if let Some(data) = store.prefixmap() {
        prefixes.merge(data);
    }
    Ok(ValidationReport::new()
        .with_results(results)
        .with_unchecked(plan.unchecked.clone())
        .with_prefixmap(prefixes))
}

/// Evaluates every relation the checks of `plan` reach over `store`, with
/// `scope` as [`Op::Scope`]; the rows of `plan.checks[i]` are the `i`-th
/// vector.
pub fn evaluate<S>(plan: &Plan, store: &S, scope: Option<&[Term]>) -> Result<Vec<Vec<Row>>, EvalError>
where
    S: NeighsRDF<Term = Term>,
{
    let mut eval = Evaluator::new(plan, store, scope);
    for check in &plan.checks {
        eval.force(check.rows)?;
    }
    plan.checks.iter().map(|c: &Check| eval.rows(c.rows)).collect()
}

/// A Shape Fragment: the triples that make the conforming focus nodes conform,
/// and the shapes it was not computed for.
#[derive(Debug, Clone)]
pub struct Fragment {
    pub triples: Vec<Triple>,
    pub unchecked: Vec<Unchecked>,
}

/// The Shape Fragment of `store` under `schema` ([`crate::algebra::fragment`]),
/// with every shape's focus nodes restricted to `focus` when given.
pub fn fragment<S>(schema: &IRSchema, store: &S, focus: Option<&[Object]>) -> Result<Fragment, ValidationError>
where
    S: NeighsRDF<Term = Term>,
{
    let plan = algebra::fragment(schema, focus.is_some())?;
    let focus: Option<Vec<Term>> = focus.map(|nodes| nodes.iter().map(|n| Term::from(n.clone())).collect());
    let err = |message: &str| EvalError {
        relation: "the fragment".to_owned(),
        message: message.to_owned(),
    };
    let root = plan.fragment.ok_or_else(|| err("a plan without a fragment"))?;
    let mut eval = Evaluator::new(&plan, store, focus.as_deref());
    eval.force(root)?;
    let Ok(Rel::Triples(triples)) = eval.rel(root) else {
        return Err(err("not a triples relation").into());
    };
    let triples = triples
        .iter()
        .filter_map(|(s, p, o)| {
            let subject = match s {
                Term::NamedNode(n) => NamedOrBlankNode::NamedNode(n.clone()),
                Term::BlankNode(b) => NamedOrBlankNode::BlankNode(b.clone()),
                _ => return None,
            };
            Some(Triple::new(subject, p.clone(), o.clone()))
        })
        .collect();
    Ok(Fragment {
        triples,
        unchecked: plan.unchecked,
    })
}

struct Evaluator<'a, S> {
    plan: &'a Plan,
    store: &'a S,
    scope: Option<&'a [Term]>,
    rels: Vec<Option<Rel>>,
    /// The focus nodes of a relation, as a set, for [`Pred::Member`].
    members: HashMap<RelId, HashSet<Term>>,
    /// The pairs of a relation, as a set, for [`Pred::PairMember`].
    pairs: HashMap<RelId, HashSet<(Term, Term)>>,
    regexes: HashMap<(String, Option<String>), RDFRegex>,
}

impl<'a, S: NeighsRDF<Term = Term>> Evaluator<'a, S> {
    fn new(plan: &'a Plan, store: &'a S, scope: Option<&'a [Term]>) -> Self {
        Self {
            plan,
            store,
            scope,
            rels: vec![None; plan.len()],
            members: HashMap::new(),
            pairs: HashMap::new(),
            regexes: HashMap::new(),
        }
    }

    /// Computes `id`, and first what it reads, on demand: a relation no check
    /// reaches is never computed. The arcs of a predicate from a set of nodes
    /// are looked up node by node ([`Self::arcs`]), so a path read from a few
    /// focus nodes never reads the whole predicate.
    fn force(&mut self, id: RelId) -> Result<(), EvalError> {
        if self.rels[id.index()].is_some() {
            return Ok(());
        }
        let plan = self.plan;
        let error = |message: String| EvalError {
            relation: format!("{id} = {:?}", plan.op(id)),
            message,
        };
        let rel = match self.arcs_of(id) {
            Some((predicate, inverse, nodes)) => {
                self.force(nodes)?;
                self.arcs(&predicate, inverse, nodes).map_err(error)?
            },
            None => {
                for input in plan.op(id).inputs() {
                    self.force(input)?;
                }
                self.relation(id).map_err(error)?
            },
        };
        self.rels[id.index()] = Some(rel);
        Ok(())
    }

    /// `Restrict` of a predicate, or of its inverse, to a node relation.
    fn arcs_of(&self, id: RelId) -> Option<(NamedNode, bool, RelId)> {
        let Op::Restrict { pairs, nodes } = self.plan.op(id) else {
            return None;
        };
        match self.plan.op(*pairs) {
            Op::Predicate(iri) => Some((NamedNode::new_unchecked(iri.as_str()), false, *nodes)),
            Op::Inverse(edges) => match self.plan.op(*edges) {
                Op::Predicate(iri) => Some((NamedNode::new_unchecked(iri.as_str()), true, *nodes)),
                _ => None,
            },
            _ => None,
        }
    }

    /// The arcs of `predicate` (inverted when `inverse`) from each node of
    /// `nodes`, distinct.
    fn arcs(&self, predicate: &NamedNode, inverse: bool, nodes: RelId) -> Result<Rel, String> {
        let iri: S::IRI = IriS::new_unchecked(predicate.as_str()).into();
        let mut out = Vec::new();
        for node in distinct(self.nodes(nodes)?) {
            if inverse {
                let triples = self
                    .store
                    .triples_with_predicate_object(&iri, &node)
                    .map_err(|e| e.to_string())?;
                for t in triples {
                    let (s, _, _) = t.into_components();
                    out.push((node.clone(), S::subject_as_term(&s)));
                }
            } else if let Ok(subject) = S::term_as_subject(&node) {
                let triples = self
                    .store
                    .triples_with_subject_predicate(&subject, &iri)
                    .map_err(|e| e.to_string())?;
                for t in triples {
                    let (_, _, o) = t.into_components();
                    out.push((node.clone(), o));
                }
            }
        }
        Ok(Rel::Pairs(distinct(&out)))
    }

    fn rel(&self, id: RelId) -> Result<&Rel, String> {
        self.rels[id.index()]
            .as_ref()
            .ok_or_else(|| format!("{id} is read before it is computed"))
    }

    fn nodes(&self, id: RelId) -> Result<&[Term], String> {
        match self.rel(id)? {
            Rel::Nodes(ns) => Ok(ns),
            other => Err(format!("{id} is not a node relation: {other:?}")),
        }
    }

    fn pairs_of(&self, id: RelId) -> Result<&[(Term, Term)], String> {
        match self.rel(id)? {
            Rel::Pairs(ps) => Ok(ps),
            other => Err(format!("{id} is not a pair relation: {other:?}")),
        }
    }

    fn row_list(&self, id: RelId) -> Result<&[Row], String> {
        match self.rel(id)? {
            Rel::Rows(rs) => Ok(rs),
            other => Err(format!("{id} is not a rows relation: {other:?}")),
        }
    }

    fn rows(&self, id: RelId) -> Result<Vec<Row>, EvalError> {
        match self.rel(id) {
            Ok(Rel::Rows(rs)) => Ok(rs.clone()),
            Ok(_) | Err(_) => Err(EvalError {
                relation: id.to_string(),
                message: "a check's rows are not a rows relation".to_owned(),
            }),
        }
    }

    /// The `(subject, object)` pairs of `predicate` in the graph, distinct.
    fn predicate(&self, predicate: &NamedNode) -> Result<Vec<(Term, Term)>, String> {
        let iri: S::IRI = IriS::new_unchecked(predicate.as_str()).into();
        let pairs: Vec<(Term, Term)> = self
            .store
            .triples_with_predicate(&iri)
            .map_err(|e| e.to_string())?
            .map(|t| {
                let (s, _, o) = t.into_components();
                (S::subject_as_term(&s), o)
            })
            .collect();
        Ok(distinct(&pairs))
    }

    fn triples(&self) -> Result<Vec<(Term, NamedNode, Term)>, String> {
        let triples: Vec<_> = self
            .store
            .triples()
            .map_err(|e| e.to_string())?
            .map(|t| {
                let (s, p, o) = t.into_components();
                let p: IriS = p.into();
                (S::subject_as_term(&s), NamedNode::new_unchecked(p.as_str()), o)
            })
            .collect();
        Ok(distinct(&triples))
    }

    /// The SHACL instances of `class` (SHACL §1.1): the subjects of
    /// `rdf:type C` for `class` and every class below it by `rdfs:subClassOf*`.
    fn class(&self, class: &NamedNode) -> Result<Vec<Term>, String> {
        let sub_class_of = self.predicate(&NamedNode::new_unchecked(RdfsVocab::rdfs_subclass_of_str().as_str()))?;
        let mut classes: HashSet<Term> = HashSet::from([Term::from(class.clone())]);
        let mut frontier: Vec<Term> = classes.iter().cloned().collect();
        while let Some(c) = frontier.pop() {
            for (sub, sup) in &sub_class_of {
                if *sup == c && classes.insert(sub.clone()) {
                    frontier.push(sub.clone());
                }
            }
        }
        let types = self.predicate(&NamedNode::new_unchecked(RdfVocab::rdf_type().as_str()))?;
        let instances: Vec<Term> = types
            .into_iter()
            .filter(|(_, c)| classes.contains(c))
            .map(|(n, _)| n)
            .collect();
        Ok(distinct(&instances))
    }

    /// Builds the sets the predicate's membership tests read.
    fn index(&mut self, pred: &Pred) -> Result<(), String> {
        let mut rels = Vec::new();
        collect_members(pred, &mut rels);
        for (rel, pair) in rels {
            if pair && !self.pairs.contains_key(&rel) {
                let set = self.pairs_of(rel)?.iter().cloned().collect();
                self.pairs.insert(rel, set);
            }
            if !pair && !self.members.contains_key(&rel) {
                let set = self.rel(rel)?.focus().into_iter().cloned().collect();
                self.members.insert(rel, set);
            }
        }
        let mut patterns = Vec::new();
        collect_patterns(pred, &mut patterns);
        for (pattern, flags) in patterns {
            if let Entry::Vacant(slot) = self.regexes.entry((pattern.clone(), flags.clone())) {
                slot.insert(RDFRegex::new(&pattern, flags.as_deref()).map_err(|e| e.to_string())?);
            }
        }
        Ok(())
    }

    fn expr<'r>(e: &Expr, scope: &Scope<'r>, constants: &'r HashMap<String, Term>) -> Option<Cow<'r, Term>> {
        match e {
            Expr::Col(Col::F) => Some(Cow::Borrowed(scope.f)),
            Expr::Col(Col::V) => scope.v.map(Cow::Borrowed),
            Expr::Col(Col::O) => scope.o.map(Cow::Borrowed),
            Expr::Const(c) => constants.get(&format!("{c:?}")).map(Cow::Borrowed),
            Expr::Triple(s, p, o) => {
                let subject = match Self::expr(s, scope, constants)?.into_owned() {
                    Term::NamedNode(n) => NamedOrBlankNode::NamedNode(n),
                    Term::BlankNode(b) => NamedOrBlankNode::BlankNode(b),
                    _ => return None,
                };
                let object = Self::expr(o, scope, constants)?.into_owned();
                let triple = Triple::new(subject, NamedNode::new_unchecked(p.as_str()), object);
                Some(Cow::Owned(Term::Triple(Box::new(triple))))
            },
        }
    }

    /// Whether `pred` is true of the row in `scope`.
    fn test(&self, pred: &Pred, scope: &Scope<'_>, constants: &HashMap<String, Term>) -> bool {
        let term = |e: &Expr| Self::expr(e, scope, constants);
        let text = |t: Cow<'_, Term>| text(&t).map(str::to_owned);
        match pred {
            Pred::True => true,
            Pred::And(ps) => ps.iter().all(|p| self.test(p, scope, constants)),
            Pred::Or(ps) => ps.iter().any(|p| self.test(p, scope, constants)),
            Pred::Not(p) => !self.test(p, scope, constants),
            Pred::ExactlyOne(ps) => ps.iter().filter(|p| self.test(p, scope, constants)).count() == 1,
            Pred::KindIn(e, kinds) => term(e).is_some_and(|t| kinds.contains(&kind_of(&t))),
            Pred::Datatype(e, datatypes) => match term(e).and_then(|t| object(&t)) {
                Some(Object::Literal(ConcreteLiteral::WrongDatatypeLiteral { .. })) | None => false,
                Some(Object::Literal(lit)) => lit
                    .datatype()
                    .get_iri()
                    .is_ok_and(|d| datatypes.iter().any(|x| x.as_str() == d.as_str())),
                Some(_) => false,
            },
            Pred::LangIn(e, ranges) => match term(e).as_deref() {
                Some(Term::Literal(l)) => l.language().is_some_and(|tag| {
                    let tag = tag.to_lowercase();
                    ranges.iter().any(|r| {
                        let r = r.to_string().to_lowercase();
                        tag == r || tag.starts_with(&format!("{r}-"))
                    })
                }),
                _ => false,
            },
            Pred::Regex(e, pattern, flags) => term(e).and_then(text).is_some_and(|s| {
                self.regexes
                    .get(&(pattern.clone(), flags.clone()))
                    .is_some_and(|r| r.is_match(&s))
            }),
            Pred::StrLen(e, op, n) => term(e)
                .and_then(text)
                .is_some_and(|s| holds((s.chars().count() as i64).cmp(n), *op)),
            Pred::Compare(a, op, b) => match (term(a).and_then(|t| object(&t)), term(b).and_then(|t| object(&t))) {
                (Some(a), Some(b)) => a.sparql_compare(&b).is_some_and(|o| holds(o, *op)),
                _ => false,
            },
            Pred::Same(a, b) => matches!((term(a), term(b)), (Some(a), Some(b)) if a == b),
            Pred::Member(e, rel) => term(e).is_some_and(|t| self.members.get(rel).is_some_and(|s| s.contains(&*t))),
            Pred::PairMember(a, b, rel) => match (term(a), term(b)) {
                (Some(a), Some(b)) => self
                    .pairs
                    .get(rel)
                    .is_some_and(|s| s.contains(&(a.into_owned(), b.into_owned()))),
                _ => false,
            },
        }
    }

    fn relation(&mut self, id: RelId) -> Result<Rel, String> {
        let op = self.plan.op(id);
        if let Some(pred) = op_pred(op) {
            self.index(pred)?;
        }
        let constants = op_pred(op).map(constants).transpose()?.unwrap_or_default();
        Ok(match op {
            Op::Empty(sort) => match sort {
                crate::algebra::Sort::Nodes => Rel::Nodes(Vec::new()),
                crate::algebra::Sort::Pairs => Rel::Pairs(Vec::new()),
                crate::algebra::Sort::Rows => Rel::Rows(Vec::new()),
                crate::algebra::Sort::Triples => Rel::Triples(Vec::new()),
            },
            Op::Constants(nodes) => Rel::Nodes(nodes.iter().map(|n| Term::from(n.clone())).collect()),
            Op::Predicate(iri) => Rel::Pairs(self.predicate(&NamedNode::new_unchecked(iri.as_str()))?),
            Op::Class(iri) => Rel::Nodes(self.class(&NamedNode::new_unchecked(iri.as_str()))?),
            Op::Triples => Rel::Triples(self.triples()?),
            Op::Scope => Rel::Nodes(distinct(self.scope.ok_or("a scoped plan evaluated without a scope")?)),
            Op::AllNodes => {
                let triples = self.triples()?;
                let nodes: Vec<Term> = triples.iter().flat_map(|(s, _, o)| [s.clone(), o.clone()]).collect();
                Rel::Nodes(distinct(&nodes))
            },
            Op::Union(rels) => {
                let mut parts = rels.iter().map(|r| self.rel(*r).cloned());
                let mut acc = parts.next().ok_or("an empty union")??;
                for part in parts {
                    match (&mut acc, part?) {
                        (Rel::Nodes(a), Rel::Nodes(b)) => a.extend(b),
                        (Rel::Pairs(a), Rel::Pairs(b)) => a.extend(b),
                        (Rel::Rows(a), Rel::Rows(b)) => a.extend(b),
                        (Rel::Triples(a), Rel::Triples(b)) => a.extend(b),
                        _ => return Err("a union of relations of different sorts".to_owned()),
                    }
                }
                acc
            },
            Op::Distinct(r) => match self.rel(*r)? {
                Rel::Nodes(ns) => Rel::Nodes(distinct(ns)),
                Rel::Pairs(ps) => Rel::Pairs(distinct(ps)),
                Rel::Rows(rs) => Rel::Rows(distinct(rs)),
                Rel::Triples(ts) => Rel::Triples(distinct(ts)),
            },
            Op::Focus(r) => Rel::Nodes(self.rel(*r)?.focus().into_iter().cloned().collect()),
            Op::Values(r) => Rel::Nodes(self.pairs_of(*r)?.iter().map(|(_, v)| v.clone()).collect()),
            Op::Identity(r) => Rel::Pairs(self.nodes(*r)?.iter().map(|n| (n.clone(), n.clone())).collect()),
            Op::Inverse(r) => Rel::Pairs(self.pairs_of(*r)?.iter().map(|(f, v)| (v.clone(), f.clone())).collect()),
            Op::Restrict { pairs, nodes } => {
                let nodes: HashSet<&Term> = self.nodes(*nodes)?.iter().collect();
                Rel::Pairs(
                    self.pairs_of(*pairs)?
                        .iter()
                        .filter(|(f, _)| nodes.contains(f))
                        .cloned()
                        .collect(),
                )
            },
            Op::Compose(a, b) => {
                let next = by_focus(self.pairs_of(*b)?);
                let mut out = Vec::new();
                for (f, v) in self.pairs_of(*a)? {
                    for w in next.get(v).into_iter().flatten() {
                        out.push((f.clone(), (*w).clone()));
                    }
                }
                Rel::Pairs(out)
            },
            Op::Closure { base, step } => {
                let step = by_focus(self.pairs_of(*step)?);
                let mut seen: HashSet<(Term, Term)> = HashSet::new();
                let mut out = Vec::new();
                let mut frontier: Vec<(Term, Term)> = self.pairs_of(*base)?.to_vec();
                while let Some(pair) = frontier.pop() {
                    if seen.insert(pair.clone()) {
                        for w in step.get(&pair.1).into_iter().flatten() {
                            frontier.push((pair.0.clone(), (*w).clone()));
                        }
                        out.push(pair);
                    }
                }
                Rel::Pairs(out)
            },
            Op::Filter(r, pred) => match self.rel(*r)? {
                Rel::Nodes(ns) => Rel::Nodes(
                    ns.iter()
                        .filter(|n| self.test(pred, &Scope { f: n, v: None, o: None }, &constants))
                        .cloned()
                        .collect(),
                ),
                Rel::Pairs(ps) => Rel::Pairs(
                    ps.iter()
                        .filter(|(f, v)| self.test(pred, &Scope { f, v: Some(v), o: None }, &constants))
                        .cloned()
                        .collect(),
                ),
                Rel::Rows(rs) => Rel::Rows(
                    rs.iter()
                        .filter(|r| {
                            self.test(
                                pred,
                                &Scope {
                                    f: &r.focus,
                                    v: r.value.as_ref(),
                                    o: None,
                                },
                                &constants,
                            )
                        })
                        .cloned()
                        .collect(),
                ),
                Rel::Triples(_) => return Err("a filter over triples".to_owned()),
            },
            Op::PairRows { pairs, with_value } => Rel::Rows(
                self.pairs_of(*pairs)?
                    .iter()
                    .map(|(f, v)| Row {
                        focus: f.clone(),
                        value: with_value.then(|| v.clone()),
                        path: None,
                    })
                    .collect(),
            ),
            Op::RowPairs(r) => Rel::Pairs(
                self.row_list(*r)?
                    .iter()
                    .filter_map(|r| Some((r.focus.clone(), r.value.clone()?)))
                    .collect(),
            ),
            Op::RowTriples(r) => Rel::Triples(
                self.row_list(*r)?
                    .iter()
                    .filter_map(|r| Some((r.focus.clone(), r.path.clone()?, r.value.clone()?)))
                    .collect(),
            ),
            Op::Arcs {
                pairs,
                predicate,
                inverse,
            } => {
                let p = NamedNode::new_unchecked(predicate.as_str());
                Rel::Triples(
                    self.pairs_of(*pairs)?
                        .iter()
                        .map(|(f, v)| match inverse {
                            false => (f.clone(), p.clone(), v.clone()),
                            true => (v.clone(), p.clone(), f.clone()),
                        })
                        .collect(),
                )
            },
            Op::NodeRows(r) => Rel::Rows(
                self.nodes(*r)?
                    .iter()
                    .map(|n| Row {
                        focus: n.clone(),
                        value: None,
                        path: None,
                    })
                    .collect(),
            ),
            Op::Repeat { rel, bag } => {
                let mut occurrences: HashMap<&Term, usize> = HashMap::new();
                for n in self.nodes(*bag)? {
                    *occurrences.entry(n).or_default() += 1;
                }
                let times = |f: &Term| occurrences.get(f).copied().unwrap_or(0);
                match self.rel(*rel)? {
                    Rel::Pairs(ps) => Rel::Pairs(
                        ps.iter()
                            .flat_map(|p| std::iter::repeat_n(p.clone(), times(&p.0)))
                            .collect(),
                    ),
                    Rel::Rows(rs) => Rel::Rows(
                        rs.iter()
                            .flat_map(|r| std::iter::repeat_n(r.clone(), times(&r.focus)))
                            .collect(),
                    ),
                    _ => return Err("a repeat of nodes or triples".to_owned()),
                }
            },
            Op::Count {
                focus,
                pairs,
                counted,
                op,
                bound,
            } => {
                let mut counts: HashMap<&Term, i64> = HashMap::new();
                for (f, v) in self.pairs_of(*pairs)? {
                    let scope = Scope { f, v: Some(v), o: None };
                    if self.test(counted, &scope, &constants) {
                        *counts.entry(f).or_default() += 1;
                    }
                }
                Rel::Rows(
                    self.nodes(*focus)?
                        .iter()
                        .filter(|n| holds(counts.get(n).copied().unwrap_or(0).cmp(bound), *op))
                        .map(|n| Row {
                            focus: n.clone(),
                            value: None,
                            path: None,
                        })
                        .collect(),
                )
            },
            Op::Duplicates { pairs, key } => {
                let mut counts: HashMap<(&Term, Cow<'_, Term>), usize> = HashMap::new();
                for (f, v) in self.pairs_of(*pairs)? {
                    let key = match key {
                        Key::Term => Some(Cow::Borrowed(v)),
                        Key::Lang => lang_key(v).map(Cow::Owned),
                    };
                    if let Some(key) = key {
                        *counts.entry((f, key)).or_default() += 1;
                    }
                }
                Rel::Rows(
                    counts
                        .into_iter()
                        .filter(|(_, n)| *n > 1)
                        .map(|((f, _), _)| Row {
                            focus: f.clone(),
                            value: None,
                            path: None,
                        })
                        .collect(),
                )
            },
            Op::Outgoing {
                pairs,
                triples,
                allowed,
                by_type,
            } => {
                let mut out_of: HashMap<&Term, Vec<(&NamedNode, &Term)>> = HashMap::new();
                match self.rel(*triples)? {
                    Rel::Triples(ts) => {
                        for (s, p, o) in ts {
                            out_of.entry(s).or_default().push((p, o));
                        }
                    },
                    _ => return Err("outgoing arcs of a relation that is not triples".to_owned()),
                }
                let allowed: HashSet<&str> = allowed.iter().map(|a| a.as_str()).collect();
                let by_type: HashMap<&str, HashSet<&str>> = by_type
                    .iter()
                    .map(|(t, ps)| (t.as_str(), ps.iter().map(|p| p.as_str()).collect()))
                    .collect();
                let rdf_type = RdfVocab::rdf_type();
                let mut out = Vec::new();
                for (f, v) in self.pairs_of(*pairs)? {
                    let arcs = out_of.get(v).map(Vec::as_slice).unwrap_or_default();
                    let permitted: HashSet<&str> = arcs
                        .iter()
                        .filter(|(p, _)| p.as_str() == rdf_type.as_str())
                        .filter_map(|(_, t)| match t {
                            Term::NamedNode(t) => by_type.get(t.as_str()),
                            _ => None,
                        })
                        .flatten()
                        .copied()
                        .collect();
                    for (p, o) in arcs {
                        if !allowed.contains(p.as_str()) && !permitted.contains(p.as_str()) {
                            out.push(Row {
                                focus: f.clone(),
                                value: Some((*o).clone()),
                                path: Some((*p).clone()),
                            });
                        }
                    }
                }
                Rel::Rows(out)
            },
            Op::PairJoin { left, right, pred } => {
                let others = by_focus(self.pairs_of(*right)?);
                let mut out = Vec::new();
                for (f, v) in self.pairs_of(*left)? {
                    for o in others.get(f).into_iter().flatten() {
                        let scope = Scope {
                            f,
                            v: Some(v),
                            o: Some(o),
                        };
                        if self.test(pred, &scope, &constants) {
                            out.push((f.clone(), v.clone()));
                        }
                    }
                }
                Rel::Pairs(out)
            },
        })
    }
}

/// What [`Key::Lang`] compares a value by: its language tag, lower-cased,
/// and its base direction, as a term; `None` for a value without a tag.
fn lang_key(term: &Term) -> Option<Term> {
    let Term::Literal(l) = term else { return None };
    let tag = l.language()?.to_lowercase();
    let direction = l.direction().map(|d| format!("--{d}")).unwrap_or_default();
    Some(Term::Literal(oxrdf::Literal::new_simple_literal(format!(
        "{tag}{direction}"
    ))))
}

/// The values of each focus node of a pair relation.
fn by_focus(pairs: &[(Term, Term)]) -> HashMap<&Term, Vec<&Term>> {
    let mut out: HashMap<&Term, Vec<&Term>> = HashMap::new();
    for (f, v) in pairs {
        out.entry(f).or_default().push(v);
    }
    out
}

fn op_pred(op: &Op) -> Option<&Pred> {
    match op {
        Op::Filter(_, p) | Op::PairJoin { pred: p, .. } | Op::Count { counted: p, .. } => Some(p),
        _ => None,
    }
}

/// The relations a predicate tests membership in; `true` for pair membership.
fn collect_members(pred: &Pred, out: &mut Vec<(RelId, bool)>) {
    match pred {
        Pred::And(ps) | Pred::Or(ps) | Pred::ExactlyOne(ps) => ps.iter().for_each(|p| collect_members(p, out)),
        Pred::Not(p) => collect_members(p, out),
        Pred::Member(_, r) => out.push((*r, false)),
        Pred::PairMember(_, _, r) => out.push((*r, true)),
        _ => {},
    }
}

fn collect_patterns(pred: &Pred, out: &mut Vec<(String, Option<String>)>) {
    match pred {
        Pred::And(ps) | Pred::Or(ps) | Pred::ExactlyOne(ps) => ps.iter().for_each(|p| collect_patterns(p, out)),
        Pred::Not(p) => collect_patterns(p, out),
        Pred::Regex(_, pattern, flags) => out.push((pattern.clone(), flags.clone())),
        _ => {},
    }
}

/// The constants of a predicate, as terms, keyed by their spelling: an
/// [`Object`]'s own equality is by value, and `"1"` and `"01"` are two terms.
fn constants(pred: &Pred) -> Result<HashMap<String, Term>, String> {
    fn push(e: &Expr, out: &mut Vec<Object>) {
        match e {
            Expr::Const(c) => out.push(c.clone()),
            Expr::Triple(s, _, o) => {
                push(s, out);
                push(o, out);
            },
            Expr::Col(_) => {},
        }
    }
    fn walk(pred: &Pred, out: &mut Vec<Object>) {
        match pred {
            Pred::And(ps) | Pred::Or(ps) | Pred::ExactlyOne(ps) => ps.iter().for_each(|p| walk(p, out)),
            Pred::Not(p) => walk(p, out),
            Pred::KindIn(e, _)
            | Pred::Datatype(e, _)
            | Pred::LangIn(e, _)
            | Pred::Regex(e, _, _)
            | Pred::StrLen(e, _, _)
            | Pred::Member(e, _) => push(e, out),
            Pred::Compare(a, _, b) | Pred::Same(a, b) | Pred::PairMember(a, b, _) => {
                push(a, out);
                push(b, out);
            },
            Pred::True => {},
        }
    }
    let mut objects = Vec::new();
    walk(pred, &mut objects);
    Ok(objects.into_iter().map(|o| (format!("{o:?}"), Term::from(o))).collect())
}
