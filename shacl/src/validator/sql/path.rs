//! Property paths → `(focus, value)` relations.
//!
//! A path is evaluated from a set of start nodes, because a zero-length path
//! relates a node to itself whether or not the data mentions it. Inverses are
//! pushed down to the predicates first (`^(a/b) = ^b/^a`, `^(p*) = (^p)*`, …),
//! so only a predicate is ever inverted. Then:
//!
//! - a predicate is its edge relation, semi-joined with the start nodes;
//! - a sequence joins each step's values to the next step's starts;
//! - an alternative is the `UNION` of its members;
//! - `p?` adds the identity on the start nodes;
//! - `p+` is a `WITH RECURSIVE` closure: `p` from the start nodes, then `p`
//!   from every node reached, until nothing new is reached (`UNION`, so the
//!   recursion terminates on cycles). The step relation is `p` from every node
//!   of the data, so a zero-length part of `p` holds for all of them;
//! - `p*` is `p+` plus the identity.
//!
//! Every pair relation is distinct: the value nodes of a focus node are a set.

use crate::validator::sql::SqlCompileError;
use crate::validator::sql::ast::{SelectBuilder, join, query, union, union_all_of};
use crate::validator::sql::context::{Ctx, Rel, node_items, pair_items};
use crate::validator::sql::dialect::SqlDialect;
use crate::validator::sql::mapping::RelationalMapping;
use crate::validator::sql::term::TermExpr;
use rudof_iri::IriS;
use rudof_rdf::SHACLPath;

/// Where a path starts.
#[derive(Debug, Clone)]
pub(crate) enum Start {
    /// The nodes of a node relation.
    Nodes(Rel),
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

/// Compiles a [`SHACLPath`] into a pair relation.
pub(crate) struct PathCompiler;

impl PathCompiler {
    /// The `(focus, value)` pairs of `path` from `start`.
    pub(crate) fn pairs<M, D>(ctx: &mut Ctx<'_, M, D>, path: &SHACLPath, start: &Start) -> Result<Rel, SqlCompileError>
    where
        M: RelationalMapping + ?Sized,
        D: SqlDialect + ?Sized,
    {
        Self::compile(ctx, &normalise(path, false), start)
    }

    /// The identity pairs on the start nodes.
    pub(crate) fn identity<M, D>(ctx: &mut Ctx<'_, M, D>, start: &Start) -> Rel
    where
        M: RelationalMapping + ?Sized,
        D: SqlDialect + ?Sized,
    {
        let nodes = match start {
            Start::Nodes(rel) => rel.clone(),
            Start::All => ctx.nodes(),
        };
        let t = TermExpr::columns("n", "f");
        let body = SelectBuilder::new(pair_items(&t, &t))
            .distinct()
            .from(nodes.from("n"))
            .into_query();
        ctx.add("identity", body)
    }

    fn compile<M, D>(ctx: &mut Ctx<'_, M, D>, path: &Normal, start: &Start) -> Result<Rel, SqlCompileError>
    where
        M: RelationalMapping + ?Sized,
        D: SqlDialect + ?Sized,
    {
        match path {
            Normal::Predicate { iri, inverse } => Ok(Self::predicate(ctx, iri, *inverse, start)),
            Normal::Sequence(steps) => {
                let Some((first, rest)) = steps.split_first() else {
                    return Ok(Self::identity(ctx, start));
                };
                let mut acc = Self::compile(ctx, first, start)?;
                for step in rest {
                    let reached = SelectBuilder::new(node_items(&TermExpr::columns("a", "v")))
                        .distinct()
                        .from(acc.from("a"))
                        .into_query();
                    let reached = ctx.add("reached", reached);
                    let next = Self::compile(ctx, step, &Start::Nodes(reached))?;
                    let body =
                        SelectBuilder::new(pair_items(&TermExpr::columns("a", "f"), &TermExpr::columns("b", "v")))
                            .distinct()
                            .from(acc.from("a"))
                            .join(join(
                                next.from("b"),
                                TermExpr::columns("a", "v").same(&TermExpr::columns("b", "f")),
                            ))
                            .into_query();
                    acc = ctx.add("sequence", body);
                }
                Ok(acc)
            },
            Normal::Alternative(members) => {
                let mut rels = Vec::new();
                for member in members {
                    rels.push(Self::compile(ctx, member, start)?);
                }
                Ok(Self::union_of(ctx, &rels))
            },
            Normal::ZeroOrOne(inner) => {
                let identity = Self::identity(ctx, start);
                let once = Self::compile(ctx, inner, start)?;
                Ok(Self::union_of(ctx, &[identity, once]))
            },
            Normal::OneOrMore(inner) => Self::closure(ctx, inner, start),
            Normal::ZeroOrMore(inner) => {
                let identity = Self::identity(ctx, start);
                let more = Self::closure(ctx, inner, start)?;
                Ok(Self::union_of(ctx, &[identity, more]))
            },
        }
    }

    fn predicate<M, D>(ctx: &mut Ctx<'_, M, D>, iri: &IriS, inverse: bool, start: &Start) -> Rel
    where
        M: RelationalMapping + ?Sized,
        D: SqlDialect + ?Sized,
    {
        let edges = ctx.predicate(iri);
        let (from, to) = if inverse { ("v", "f") } else { ("f", "v") };
        let f = TermExpr::columns("e", from);
        let v = TermExpr::columns("e", to);
        match start {
            Start::All if !inverse => edges,
            Start::All => {
                let body = SelectBuilder::new(pair_items(&f, &v))
                    .from(edges.from("e"))
                    .into_query();
                ctx.add("inverse", body)
            },
            Start::Nodes(nodes) => {
                let body = SelectBuilder::new(pair_items(&f, &v))
                    .distinct()
                    .from(edges.from("e"))
                    .join(join(nodes.from("s"), f.same(&TermExpr::columns("s", "f"))))
                    .into_query();
                ctx.add("step", body)
            },
        }
    }

    fn closure<M, D>(ctx: &mut Ctx<'_, M, D>, inner: &Normal, start: &Start) -> Result<Rel, SqlCompileError>
    where
        M: RelationalMapping + ?Sized,
        D: SqlDialect + ?Sized,
    {
        if !ctx.dialect.supports_recursive_cte() {
            return Err(SqlCompileError::Unsupported(format!(
                "sh:zeroOrMorePath and sh:oneOrMorePath on {}, which has no WITH RECURSIVE",
                ctx.dialect.name()
            )));
        }
        let base = Self::compile(ctx, inner, start)?;
        let step = Self::compile(ctx, inner, &Start::All)?;
        let name = ctx.reserve("closure");
        let seed = SelectBuilder::new(pair_items(&TermExpr::columns("b", "f"), &TermExpr::columns("b", "v")))
            .from(base.from("b"));
        let more = SelectBuilder::new(pair_items(&TermExpr::columns("c", "f"), &TermExpr::columns("s", "v")))
            .from(crate::validator::sql::ast::cte_ref(&name, "c"))
            .join(join(
                step.from("s"),
                TermExpr::columns("c", "v").same(&TermExpr::columns("s", "f")),
            ));
        let body = query(union(seed.into_set_expr(), more.into_set_expr(), false));
        Ok(ctx.define(name, body, true))
    }

    fn union_of<M, D>(ctx: &mut Ctx<'_, M, D>, rels: &[Rel]) -> Rel
    where
        M: RelationalMapping + ?Sized,
        D: SqlDialect + ?Sized,
    {
        let bodies = rels
            .iter()
            .map(|r| {
                SelectBuilder::new(pair_items(&TermExpr::columns("u", "f"), &TermExpr::columns("u", "v")))
                    .from(r.from("u"))
                    .into_set_expr()
            })
            .collect();
        match union_all_of(bodies, false) {
            Some(body) => ctx.add("union", query(body)),
            None => ctx.empty_pairs(),
        }
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
