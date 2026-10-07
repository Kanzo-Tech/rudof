//! The SQL interpretation of a [`Plan`]: a temporary table per shared
//! relation, then one query with a branch per check.
//!
//! A relation's columns follow its [`Sort`], each term spread over the four
//! columns of [`crate::validator::sql::term`]: `f_*` for the focus, `v_*` for
//! the value, `p` for a triple's predicate and `path` for a row's path
//! override. A relation several consumers read is a `CREATE TEMPORARY TABLE`,
//! written and computed once (rudof#6), and a relation read once is the
//! subquery that reads it; the query is `SELECT <check 0's rows> UNION ALL
//! SELECT <check 1's rows> …` over them.
//!
//! Tables and not CTEs because an engine plans a table from its statistics
//! and a CTE blind: on the Health-RI profile DuckDB turns the `EXISTS` over
//! materialized CTEs into cross products and spills past 20 GB at 395,000
//! triples, where the same relations as tables take 3.5 s and 0.5 GB. One
//! statement also cost a second or more of optimizer time per schema.
//!
//! The algebra's predicates are two-valued (a row passes a filter only when
//! its predicate is true), and SQL's are three-valued: every test that can be
//! `NULL` (an incomparable comparison, a regular expression on a `NULL`) is
//! read through [`known_true`] before anything negates it.

use crate::algebra::{Check, CmpOp, Col, Expr as AExpr, Key, Kind, Op, Plan, Pred, RelId, Sort};
use crate::validator::sql::SqlCompileError;
use crate::validator::sql::ast::{
    SelectBuilder, and, and_all, balanced, boolean, case, col, compare, count_star, cte, cte_ref, derived, eq, exists,
    function, in_list, is_not_null, item, join, known_true, left_join, not, not_eq, null, number, or, or_all, query,
    string, union, union_all_of, with,
};
use crate::validator::sql::dialect::Dialect;
use crate::validator::sql::term::{BLANK, EncodedTerm, IRI, LITERAL, TRIPLE, TermExpr, compare_terms, well_formed_for};
use crate::validator::sql::triples::{PREDICATE_COLUMN, Triples};
use rudof_rdf::vocab::RdfVocab;
use sqlparser::ast::{BinaryOperator, Expr, Query, SelectItem, SetExpr, TableFactor};
use std::cell::Cell;

/// The deepest a statement nests the subqueries of inline relations: a
/// longer chain is cut into tables, as a parser recurses once per level.
const NESTING: usize = 16;

/// The column of a rows relation that overrides the result path (`sh:closed`).
const PATH_COLUMN: &str = "path";

/// The public names of the plan's columns, in order: the index of the check
/// a row belongs to, the focus term, the value term and the path override.
pub const RESULT_COLUMNS: [&str; 10] = [
    "check",
    "focus_kind",
    "focus_value",
    "focus_datatype",
    "focus_lang",
    "value_kind",
    "value_value",
    "value_datatype",
    "value_lang",
    "path",
];

/// The columns of a relation of `sort`, read under `alias`.
fn items(sort: Sort, alias: &str) -> Vec<SelectItem> {
    let f = TermExpr::columns(alias, "f");
    let v = TermExpr::columns(alias, "v");
    match sort {
        Sort::Nodes => f.items("f"),
        Sort::Pairs => pair(&f, &v),
        Sort::Rows => row(&f, &v, col(alias, PATH_COLUMN)),
        Sort::Triples => triple(&f, col(alias, PREDICATE_COLUMN), &v),
    }
}

fn pair(f: &TermExpr, v: &TermExpr) -> Vec<SelectItem> {
    let mut out = f.items("f");
    out.extend(v.items("v"));
    out
}

fn row(f: &TermExpr, v: &TermExpr, path: Expr) -> Vec<SelectItem> {
    let mut out = pair(f, v);
    out.push(item(path, PATH_COLUMN));
    out
}

fn triple(s: &TermExpr, p: Expr, o: &TermExpr) -> Vec<SelectItem> {
    let mut out = s.items("f");
    out.push(item(p, PREDICATE_COLUMN));
    out.extend(o.items("v"));
    out
}

/// A relation of `sort` with no rows.
fn empty(sort: Sort) -> Query {
    let none = TermExpr::constant(&EncodedTerm::default());
    let projection = match sort {
        Sort::Nodes => none.items("f"),
        Sort::Pairs => pair(&none, &none),
        Sort::Rows => row(&none, &TermExpr::null(), null()),
        Sort::Triples => triple(&none, string(""), &none),
    };
    SelectBuilder::new(projection).filter(boolean(false)).into_query()
}

/// The statement of a plan with no checks: the result columns, and no row.
fn no_results() -> SetExpr {
    let projection = RESULT_COLUMNS.iter().map(|c| item(null(), c)).collect();
    SelectBuilder::new(projection).filter(boolean(false)).into_set_expr()
}

fn one() -> Vec<SelectItem> {
    vec![SelectItem::UnnamedExpr(number(1))]
}

fn binary(op: CmpOp) -> BinaryOperator {
    match op {
        CmpOp::Lt => BinaryOperator::Lt,
        CmpOp::LtEq => BinaryOperator::LtEq,
        CmpOp::Gt => BinaryOperator::Gt,
        CmpOp::GtEq => BinaryOperator::GtEq,
    }
}

fn kind(k: Kind) -> &'static str {
    match k {
        Kind::Iri => IRI,
        Kind::Blank => BLANK,
        Kind::Literal => LITERAL,
        Kind::TripleTerm => TRIPLE,
    }
}

/// Renders the relations of one plan over one triples relation, for one dialect.
pub(crate) struct Renderer<'a, D: ?Sized> {
    plan: &'a Plan,
    triples: &'a Triples,
    dialect: &'a D,
    /// Whether each relation is a table of the script, read by name.
    named: Vec<bool>,
    /// The body of each relation read once, until its reader takes it.
    inline: Vec<Cell<Option<Query>>>,
}

impl<'a, D> Renderer<'a, D>
where
    D: Dialect + ?Sized,
{
    pub(crate) fn new(plan: &'a Plan, triples: &'a Triples, dialect: &'a D) -> Self {
        Self {
            plan,
            triples,
            dialect,
            named: vec![false; plan.len()],
            inline: (0..plan.len()).map(|_| Cell::new(None)).collect(),
        }
    }

    /// The tables of the script: every relation `roots` reach that more than
    /// one consumer reads (or that reads itself, a closure) as a temporary
    /// table, inputs first, each with its name. Every other relation is
    /// inline, as the one subquery that reads it; a root is read once, by the
    /// query of [`Self::checks`] or [`Self::fragment`].
    pub(crate) fn tables(&mut self, roots: &[RelId]) -> Result<Vec<(String, Query)>, SqlCompileError> {
        let reached = self.plan.reached(roots);
        let mut readers = vec![0usize; self.plan.len()];
        for id in reached
            .iter()
            .flat_map(|id| self.plan.op(*id).inputs())
            .chain(roots.iter().copied())
        {
            readers[id.index()] += 1;
        }
        // How deep each inline relation nests the subqueries it reads; a
        // relation that would nest deeper than `NESTING` is a table instead.
        let mut depth = vec![0usize; self.plan.len()];
        for id in &reached {
            let named = readers[id.index()] > 1 || matches!(self.plan.op(*id), Op::Closure { .. });
            let nested = 1 + self
                .plan
                .op(*id)
                .inputs()
                .iter()
                .map(|input| depth[input.index()])
                .max()
                .unwrap_or(0);
            self.named[id.index()] = named || nested > NESTING;
            depth[id.index()] = if self.named[id.index()] { 0 } else { nested };
        }
        // Inputs before their readers (the plan's ids are a topological
        // order), each body built once and without recursion.
        let mut tables = Vec::new();
        for id in &reached {
            let body = self.relation(*id)?;
            if self.named[id.index()] {
                // A closure reads itself: the recursive CTE of its own table.
                let body = match self.plan.op(*id) {
                    Op::Closure { .. } => with(
                        vec![cte(&name(*id), body)],
                        true,
                        SelectBuilder::new(items(self.plan.sort(*id), "c"))
                            .from(cte_ref(&name(*id), "c"))
                            .into_query(),
                    ),
                    _ => body,
                };
                tables.push((name(*id), body));
            } else {
                *self.inline[id.index()].get_mut() = Some(body);
            }
        }
        Ok(tables)
    }

    /// The query of the rows of every check under the public
    /// [`RESULT_COLUMNS`], tagged with the check's index, as one `UNION ALL`.
    pub(crate) fn checks(&self, checks: &[Check]) -> Result<Query, SqlCompileError> {
        let mut selects = Vec::with_capacity(checks.len());
        for (index, check) in checks.iter().enumerate() {
            let f = TermExpr::columns("r", "f");
            let v = TermExpr::columns("r", "v");
            let projection = vec![
                item(string(&index.to_string()), RESULT_COLUMNS[0]),
                item(f.kind, RESULT_COLUMNS[1]),
                item(f.lex, RESULT_COLUMNS[2]),
                item(f.datatype, RESULT_COLUMNS[3]),
                item(f.lang, RESULT_COLUMNS[4]),
                item(v.kind, RESULT_COLUMNS[5]),
                item(v.lex, RESULT_COLUMNS[6]),
                item(v.datatype, RESULT_COLUMNS[7]),
                item(v.lang, RESULT_COLUMNS[8]),
                item(col("r", PATH_COLUMN), RESULT_COLUMNS[9]),
            ];
            selects.push(
                SelectBuilder::new(projection)
                    .from(self.from(check.rows, "r")?)
                    .into_set_expr(),
            );
        }
        Ok(query(union_all_of(selects, true).unwrap_or_else(no_results)))
    }

    /// The rows of the triples relation that are triples of `fragment`.
    pub(crate) fn fragment(&self, fragment: RelId) -> Result<Query, SqlCompileError> {
        Ok(self.triples.among(self.from(fragment, "m")?))
    }

    /// The relation `id` read under `alias`: its table when it has one, else its
    /// body as a subquery, handed to its one reader.
    fn from(&self, id: RelId, alias: &str) -> Result<TableFactor, SqlCompileError> {
        Ok(if self.named[id.index()] {
            cte_ref(&name(id), alias)
        } else {
            let body = self.inline[id.index()].take();
            derived(
                body.ok_or_else(|| internal("a relation read once was read twice"))?,
                alias,
            )
        })
    }

    /// The body of the relation `id`.
    fn relation(&self, id: RelId) -> Result<Query, SqlCompileError> {
        let sort = self.plan.sort(id);
        let x = |alias: &str| TermExpr::columns(alias, "f");
        let v = |alias: &str| TermExpr::columns(alias, "v");
        Ok(match self.plan.op(id) {
            Op::Empty(sort) => empty(*sort),
            Op::Constants(nodes) => {
                let mut bodies = Vec::new();
                for node in nodes {
                    bodies.push(SelectBuilder::new(TermExpr::object(node)?.items("f")).into_set_expr());
                }
                match union_all_of(bodies, true) {
                    Some(body) => query(body),
                    None => empty(Sort::Nodes),
                }
            },
            Op::Predicate(iri) => SelectBuilder::new(pair(&TermExpr::columns("m", "s"), &TermExpr::columns("m", "o")))
                .distinct()
                .from(derived(self.triples.predicate(iri), "m"))
                .into_query(),
            Op::Scope => SelectBuilder::new(TermExpr::columns("m", "n").items("f"))
                .distinct()
                .from(derived(
                    self.triples
                        .scope()
                        .ok_or_else(|| SqlCompileError::Internal("a scoped plan with no focus relation".to_owned()))?,
                    "m",
                ))
                .into_query(),
            Op::Class(iri) => SelectBuilder::new(TermExpr::columns("m", "n").items("f"))
                .distinct()
                .from(derived(self.triples.class_extent(iri), "m"))
                .into_query(),
            // An RDF graph is a set (RDF 1.1 Concepts §3) and the relation may
            // be a bag (a view with a triple in two of its rows).
            Op::Triples => SelectBuilder::new(triple(
                &TermExpr::columns("m", "s"),
                col("m", PREDICATE_COLUMN),
                &TermExpr::columns("m", "o"),
            ))
            .distinct()
            .from(derived(self.triples.all(), "m"))
            .into_query(),
            Op::AllNodes => {
                let triples = derived(self.triples.all(), "t");
                let subjects = SelectBuilder::new(TermExpr::columns("t", "s").items("f")).from(triples.clone());
                let objects = SelectBuilder::new(TermExpr::columns("t", "o").items("f")).from(triples);
                query(union(subjects.into_set_expr(), objects.into_set_expr(), false))
            },
            Op::Union(rels) => {
                let bodies = rels
                    .iter()
                    .map(|r| {
                        Ok(SelectBuilder::new(items(sort, "u"))
                            .from(self.from(*r, "u")?)
                            .into_set_expr())
                    })
                    .collect::<Result<Vec<SetExpr>, SqlCompileError>>()?;
                query(union_all_of(bodies, true).ok_or_else(|| internal("an empty union"))?)
            },
            Op::Distinct(r) => SelectBuilder::new(items(sort, "d"))
                .distinct()
                .from(self.from(*r, "d")?)
                .into_query(),
            Op::Focus(r) => SelectBuilder::new(x("a").items("f"))
                .from(self.from(*r, "a")?)
                .into_query(),
            Op::Values(r) => SelectBuilder::new(v("a").items("f"))
                .from(self.from(*r, "a")?)
                .into_query(),
            Op::Identity(r) => SelectBuilder::new(pair(&x("a"), &x("a")))
                .from(self.from(*r, "a")?)
                .into_query(),
            Op::Inverse(r) => SelectBuilder::new(pair(&v("a"), &x("a")))
                .from(self.from(*r, "a")?)
                .into_query(),
            Op::Restrict { pairs, nodes } => SelectBuilder::new(items(Sort::Pairs, "a"))
                .from(self.from(*pairs, "a")?)
                .filter(exists(
                    SelectBuilder::new(one())
                        .from(self.from(*nodes, "n")?)
                        .filter(x("n").same(&x("a")))
                        .into_query(),
                ))
                .into_query(),
            Op::Compose(a, b) => SelectBuilder::new(pair(&x("a"), &v("b")))
                .from(self.from(*a, "a")?)
                .join(join(self.from(*b, "b")?, v("a").same(&x("b"))))
                .into_query(),
            Op::Closure { base, step } => {
                if !self.dialect.supports_recursive_cte() {
                    return Err(SqlCompileError::Unsupported(format!(
                        "sh:zeroOrMorePath and sh:oneOrMorePath on {}, which has no WITH RECURSIVE",
                        self.dialect.name()
                    )));
                }
                // `UNION`, not `UNION ALL`: the recursion stops on cycles.
                let seed = SelectBuilder::new(items(Sort::Pairs, "b")).from(self.from(*base, "b")?);
                let more = SelectBuilder::new(pair(&x("c"), &v("s")))
                    .from(self.from(id, "c")?)
                    .join(join(self.from(*step, "s")?, v("c").same(&x("s"))));
                query(union(seed.into_set_expr(), more.into_set_expr(), false))
            },
            Op::Filter(r, pred) => SelectBuilder::new(items(sort, "a"))
                .from(self.from(*r, "a")?)
                .filter(self.pred(pred, &Scope::new("a"))?)
                .into_query(),
            Op::PairRows { pairs, with_value } => {
                let value = if *with_value { v("a") } else { TermExpr::null() };
                SelectBuilder::new(row(&x("a"), &value, null()))
                    .from(self.from(*pairs, "a")?)
                    .into_query()
            },
            Op::RowPairs(r) => SelectBuilder::new(pair(&x("a"), &v("a")))
                .from(self.from(*r, "a")?)
                .filter(is_not_null(v("a").kind))
                .into_query(),
            Op::RowTriples(r) => SelectBuilder::new(triple(&x("a"), col("a", PATH_COLUMN), &v("a")))
                .from(self.from(*r, "a")?)
                .filter(and(is_not_null(v("a").kind), is_not_null(col("a", PATH_COLUMN))))
                .into_query(),
            Op::Arcs {
                pairs,
                predicate,
                inverse,
            } => {
                let (s, o) = if *inverse { (v("a"), x("a")) } else { (x("a"), v("a")) };
                SelectBuilder::new(triple(&s, string(predicate.as_str()), &o))
                    .from(self.from(*pairs, "a")?)
                    .into_query()
            },
            Op::NodeRows(r) => SelectBuilder::new(row(&x("a"), &TermExpr::null(), null()))
                .from(self.from(*r, "a")?)
                .into_query(),
            Op::Repeat { rel, bag } => SelectBuilder::new(items(sort, "a"))
                .from(self.from(*rel, "a")?)
                .join(join(self.from(*bag, "b")?, x("b").same(&x("a"))))
                .into_query(),
            Op::Count {
                focus,
                pairs,
                counted,
                op,
                bound,
            } => {
                let f = x("a");
                let mut projection = f.items("f");
                projection.push(item(count_star(), "n"));
                let counts = SelectBuilder::new(projection)
                    .from(self.from(*pairs, "a")?)
                    .filter(self.pred(counted, &Scope::new("a"))?)
                    .group_by(vec![f.kind, f.lex, f.datatype, f.lang])
                    .into_query();
                let n = function("COALESCE", vec![col("c", "n"), number(0)]);
                SelectBuilder::new(row(&x("F"), &TermExpr::null(), null()))
                    .from(self.from(*focus, "F")?)
                    .join(left_join(derived(counts, "c"), x("c").same(&x("F"))))
                    .filter(compare(n, binary(*op), number(*bound)))
                    .into_query()
            },
            Op::Duplicates { pairs, key } => {
                // One row per focus node and key held by more than one value.
                let (f, value) = (x("a"), v("a"));
                let mut group = vec![f.kind.clone(), f.lex.clone(), f.datatype.clone(), f.lang.clone()];
                let held = match key {
                    Key::Lang => {
                        group.push(function("LOWER", vec![value.lang.clone()]));
                        and(value.is_kind(LITERAL), not_eq(value.lang, string("")))
                    },
                    Key::Term => {
                        group.extend([value.kind, value.lex, value.datatype, value.lang]);
                        boolean(true)
                    },
                };
                SelectBuilder::new(row(&f, &TermExpr::null(), null()))
                    .from(self.from(*pairs, "a")?)
                    .filter(held)
                    .group_by(group)
                    .having(compare(count_star(), BinaryOperator::Gt, number(1)))
                    .into_query()
            },
            Op::Outgoing {
                pairs,
                triples,
                allowed,
                by_type,
            } => {
                let p = col("t", PREDICATE_COLUMN);
                let mut permitted = in_list(p.clone(), allowed.iter().map(|a| string(a.as_str())).collect());
                // A property a class of the value node permits: read from the
                // data's `rdf:type`, so the triples are read once.
                if !by_type.is_empty() {
                    let types = self.triples.predicate(&RdfVocab::rdf_type());
                    let class = TermExpr::columns("ty", "o");
                    let by_class = or_all(by_type.iter().map(|(class_iri, properties)| {
                        and(
                            class.same(&TermExpr::constant(&[
                                IRI.to_owned(),
                                class_iri.as_str().to_owned(),
                                String::new(),
                                String::new(),
                            ])),
                            in_list(p.clone(), properties.iter().map(|q| string(q.as_str())).collect()),
                        )
                    }));
                    permitted = or(
                        permitted,
                        exists(
                            SelectBuilder::new(one())
                                .from(derived(types, "ty"))
                                .filter(and(TermExpr::columns("ty", "s").same(&v("a")), by_class))
                                .into_query(),
                        ),
                    );
                }
                SelectBuilder::new(row(&x("a"), &v("t"), p.clone()))
                    .from(self.from(*pairs, "a")?)
                    .join(join(self.from(*triples, "t")?, x("t").same(&v("a"))))
                    .filter(not(permitted))
                    .into_query()
            },
            Op::PairJoin { left, right, pred } => {
                let scope = Scope {
                    row: "a",
                    other: Some("o"),
                };
                SelectBuilder::new(pair(&x("a"), &v("a")))
                    .from(self.from(*left, "a")?)
                    .join(join(self.from(*right, "o")?, x("o").same(&x("a"))))
                    .filter(self.pred(pred, &scope)?)
                    .into_query()
            },
        })
    }

    fn expr(&self, e: &AExpr, scope: &Scope) -> Result<TermExpr, SqlCompileError> {
        Ok(match e {
            AExpr::Col(Col::F) => TermExpr::columns(scope.row, "f"),
            AExpr::Col(Col::V) => TermExpr::columns(scope.row, "v"),
            AExpr::Col(Col::O) => TermExpr::columns(
                scope
                    .other
                    .ok_or_else(|| internal("the column O outside a pair join"))?,
                "v",
            ),
            AExpr::Const(object) => TermExpr::object(object)?,
            AExpr::Triple(s, p, o) => TermExpr::triple(&self.expr(s, scope)?, p, &self.expr(o, scope)?),
        })
    }

    /// The predicate as a two-valued SQL condition.
    fn pred(&self, pred: &Pred, scope: &Scope) -> Result<Expr, SqlCompileError> {
        let preds = |ps: &[Pred]| ps.iter().map(|p| self.pred(p, scope)).collect::<Result<Vec<_>, _>>();
        Ok(match pred {
            Pred::True => boolean(true),
            Pred::And(ps) => and_all(preds(ps)?),
            Pred::Or(ps) => or_all(preds(ps)?),
            Pred::Not(p) => not(self.pred(p, scope)?),
            Pred::ExactlyOne(ps) => {
                let ones = preds(ps)?
                    .into_iter()
                    .map(|p| case(vec![(p, number(1))], number(0)))
                    .collect();
                let count = balanced(ones, &|a, b| compare(a, BinaryOperator::Plus, b)).unwrap_or_else(|| number(0));
                eq(count, number(1))
            },
            Pred::KindIn(e, kinds) => in_list(
                self.expr(e, scope)?.kind,
                kinds.iter().map(|k| string(kind(*k))).collect(),
            ),
            Pred::Datatype(e, datatypes) => {
                let t = self.expr(e, scope)?;
                let names: Vec<String> = datatypes.iter().map(|d| d.as_str().to_owned()).collect();
                and_all([
                    t.is_kind(LITERAL),
                    in_list(t.datatype.clone(), names.iter().map(|d| string(d)).collect()),
                    well_formed_for(self.dialect, &t, &names)?,
                ])
            },
            Pred::LangIn(e, langs) => {
                let t = self.expr(e, scope)?;
                let tag = function("LOWER", vec![t.lang.clone()]);
                let any = or_all(langs.iter().map(|l| {
                    let l = l.to_string().to_lowercase();
                    or(
                        eq(tag.clone(), string(&l)),
                        Expr::Like {
                            negated: false,
                            any: false,
                            expr: Box::new(tag.clone()),
                            pattern: Box::new(string(&format!("{l}-%"))),
                            escape_char: None,
                        },
                    )
                }));
                and_all([t.is_kind(LITERAL), not_eq(t.lang, string("")), any])
            },
            Pred::Regex(e, pattern, flags) => {
                let t = self.expr(e, scope)?;
                let matches = self.dialect.regex_match(t.lex.clone(), pattern, flags.as_deref())?;
                and(t.is_not_kind(BLANK), known_true(matches))
            },
            Pred::StrLen(e, op, n) => {
                let t = self.expr(e, scope)?;
                let holds = compare(self.dialect.char_length(t.lex.clone()), binary(*op), number(*n));
                and(t.is_not_kind(BLANK), known_true(holds))
            },
            Pred::Compare(a, op, b) => known_true(compare_terms(
                self.dialect,
                &self.expr(a, scope)?,
                binary(*op),
                &self.expr(b, scope)?,
            )?),
            Pred::Same(a, b) => self.expr(a, scope)?.same(&self.expr(b, scope)?),
            Pred::Member(e, r) => exists(
                SelectBuilder::new(one())
                    .from(self.from(*r, "m")?)
                    .filter(TermExpr::columns("m", "f").same(&self.expr(e, scope)?))
                    .into_query(),
            ),
            Pred::PairMember(a, b, r) => exists(
                SelectBuilder::new(one())
                    .from(self.from(*r, "m")?)
                    .filter(and(
                        TermExpr::columns("m", "f").same(&self.expr(a, scope)?),
                        TermExpr::columns("m", "v").same(&self.expr(b, scope)?),
                    ))
                    .into_query(),
            ),
        })
    }
}

/// The aliases a predicate's columns are read under.
struct Scope {
    /// The row's alias: `F` and `V`.
    row: &'static str,
    /// The other relation's alias in a pair join: `O`.
    other: Option<&'static str>,
}

impl Scope {
    fn new(row: &'static str) -> Self {
        Self { row, other: None }
    }
}

/// The CTE name of a relation.
/// The table of a shared relation, under a prefix of its own so that it
/// does not shadow a host table.
fn name(id: RelId) -> String {
    format!("shacl_{id}")
}

fn internal(message: &str) -> SqlCompileError {
    SqlCompileError::Internal(message.to_owned())
}
