//! The SQL interpretation of a [`Plan`]: each relation a CTE, each check one
//! query over the CTEs it reaches.
//!
//! A relation's columns follow its [`Sort`], each term spread over the four
//! columns of [`crate::validator::sql::term`]: `f_*` for the focus, `v_*` for
//! the value, `p` for a triple's predicate and `path` for a row's path
//! override. A check's query is `WITH <the CTEs its rows reach> SELECT … FROM
//! <its rows>`, so each check is a self-contained query.
//!
//! The algebra's predicates are two-valued (a row passes a filter only when
//! its predicate is true), and SQL's are three-valued: every test that can be
//! `NULL` (an incomparable comparison, a regular expression on a `NULL`) is
//! read through [`known_true`] before anything negates it.

use crate::algebra::{CmpOp, Col, Expr as AExpr, Kind, Op, Plan, Pred, RelId, Sort};
use crate::validator::sql::SqlCompileError;
use crate::validator::sql::ast::{
    SelectBuilder, and, and_all, balanced, boolean, case, col, compare, count_star, cte, cte_ref, derived, eq, exists,
    function, in_list, item, join, known_true, left_join, not, not_eq, null, number, or, or_all, query, string, union,
    union_all_of, with,
};
use crate::validator::sql::dialect::SqlDialect;
use crate::validator::sql::mapping::{PREDICATE_COLUMN, RelationalMapping};
use crate::validator::sql::term::{BLANK, EncodedTerm, IRI, LITERAL, TermExpr, compare_terms, well_formed_for};
use sqlparser::ast::{BinaryOperator, Cte, Expr, Query, SelectItem, SetExpr};

/// The column of a rows relation that overrides the result path (`sh:closed`).
const PATH_COLUMN: &str = "path";

/// The public names of a check's columns, in order.
pub const RESULT_COLUMNS: [&str; 9] = [
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
    }
}

/// Renders the relations of one plan for one mapping and dialect.
pub(crate) struct Renderer<'a, M: ?Sized, D: ?Sized> {
    plan: &'a Plan,
    mapping: &'a M,
    dialect: &'a D,
    /// The CTE of each relation, rendered once.
    ctes: Vec<Option<Cte>>,
}

impl<'a, M, D> Renderer<'a, M, D>
where
    M: RelationalMapping + ?Sized,
    D: SqlDialect + ?Sized,
{
    pub(crate) fn new(plan: &'a Plan, mapping: &'a M, dialect: &'a D) -> Self {
        Self {
            plan,
            mapping,
            dialect,
            ctes: vec![None; plan.len()],
        }
    }

    /// The query of the check whose rows are `rows`: the CTEs it reaches,
    /// then its rows under the public [`RESULT_COLUMNS`].
    pub(crate) fn check(&mut self, rows: RelId) -> Result<Query, SqlCompileError> {
        let reached = self.plan.reached(&[rows]);
        let mut ctes = Vec::with_capacity(reached.len());
        for id in &reached {
            if self.ctes[id.index()].is_none() {
                let body = self.relation(*id)?;
                self.ctes[id.index()] = Some(cte(&name(*id), body));
            }
            ctes.push(self.ctes[id.index()].clone().expect("rendered"));
        }
        let recursive = reached.iter().any(|id| matches!(self.plan.op(*id), Op::Closure { .. }));
        let f = TermExpr::columns("r", "f");
        let v = TermExpr::columns("r", "v");
        let projection = vec![
            item(f.kind, RESULT_COLUMNS[0]),
            item(f.lex, RESULT_COLUMNS[1]),
            item(f.datatype, RESULT_COLUMNS[2]),
            item(f.lang, RESULT_COLUMNS[3]),
            item(v.kind, RESULT_COLUMNS[4]),
            item(v.lex, RESULT_COLUMNS[5]),
            item(v.datatype, RESULT_COLUMNS[6]),
            item(v.lang, RESULT_COLUMNS[7]),
            item(col("r", PATH_COLUMN), RESULT_COLUMNS[8]),
        ];
        let body = SelectBuilder::new(projection).from(from(rows, "r")).into_query();
        Ok(with(ctes, recursive, body))
    }

    /// The body of the CTE of `id`.
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
            Op::Predicate(iri) => match self.mapping.predicate(iri) {
                Some(edges) => SelectBuilder::new(pair(&TermExpr::columns("m", "s"), &TermExpr::columns("m", "o")))
                    .distinct()
                    .from(derived(edges.0, "m"))
                    .into_query(),
                None => empty(Sort::Pairs),
            },
            Op::Class(iri) => match self.mapping.class_extent(iri) {
                Some(extent) => SelectBuilder::new(TermExpr::columns("m", "n").items("f"))
                    .distinct()
                    .from(derived(extent.0, "m"))
                    .into_query(),
                None => empty(Sort::Nodes),
            },
            // An RDF graph is a set (RDF 1.1 Concepts §3) and a mapping may
            // answer a bag (one row per source row, a triple in two rules).
            Op::Triples => SelectBuilder::new(triple(
                &TermExpr::columns("m", "s"),
                col("m", PREDICATE_COLUMN),
                &TermExpr::columns("m", "o"),
            ))
            .distinct()
            .from(derived(self.mapping.triples(), "m"))
            .into_query(),
            Op::AllNodes => {
                let triples = derived(self.mapping.triples(), "t");
                let subjects = SelectBuilder::new(TermExpr::columns("t", "s").items("f")).from(triples.clone());
                let objects = SelectBuilder::new(TermExpr::columns("t", "o").items("f")).from(triples);
                query(union(subjects.into_set_expr(), objects.into_set_expr(), false))
            },
            Op::Union(rels) => {
                let bodies: Vec<SetExpr> = rels
                    .iter()
                    .map(|r| SelectBuilder::new(items(sort, "u")).from(from(*r, "u")).into_set_expr())
                    .collect();
                query(union_all_of(bodies, true).ok_or_else(|| internal("an empty union"))?)
            },
            Op::Distinct(r) => SelectBuilder::new(items(sort, "d"))
                .distinct()
                .from(from(*r, "d"))
                .into_query(),
            Op::Focus(r) => SelectBuilder::new(x("a").items("f")).from(from(*r, "a")).into_query(),
            Op::Values(r) => SelectBuilder::new(v("a").items("f")).from(from(*r, "a")).into_query(),
            Op::Identity(r) => SelectBuilder::new(pair(&x("a"), &x("a")))
                .from(from(*r, "a"))
                .into_query(),
            Op::Inverse(r) => SelectBuilder::new(pair(&v("a"), &x("a")))
                .from(from(*r, "a"))
                .into_query(),
            Op::Restrict { pairs, nodes } => SelectBuilder::new(items(Sort::Pairs, "a"))
                .from(from(*pairs, "a"))
                .filter(exists(
                    SelectBuilder::new(one())
                        .from(from(*nodes, "n"))
                        .filter(x("n").same(&x("a")))
                        .into_query(),
                ))
                .into_query(),
            Op::Compose(a, b) => SelectBuilder::new(pair(&x("a"), &v("b")))
                .distinct()
                .from(from(*a, "a"))
                .join(join(from(*b, "b"), v("a").same(&x("b"))))
                .into_query(),
            Op::Closure { base, step } => {
                if !self.dialect.supports_recursive_cte() {
                    return Err(SqlCompileError::Unsupported(format!(
                        "sh:zeroOrMorePath and sh:oneOrMorePath on {}, which has no WITH RECURSIVE",
                        self.dialect.name()
                    )));
                }
                // `UNION`, not `UNION ALL`: the recursion stops on cycles.
                let seed = SelectBuilder::new(items(Sort::Pairs, "b")).from(from(*base, "b"));
                let more = SelectBuilder::new(pair(&x("c"), &v("s")))
                    .from(from(id, "c"))
                    .join(join(from(*step, "s"), v("c").same(&x("s"))));
                query(union(seed.into_set_expr(), more.into_set_expr(), false))
            },
            Op::Filter(r, pred) => SelectBuilder::new(items(sort, "a"))
                .from(from(*r, "a"))
                .filter(self.pred(pred, &Scope::new("a"))?)
                .into_query(),
            Op::PairRows { pairs, with_value } => {
                let value = if *with_value { v("a") } else { TermExpr::null() };
                SelectBuilder::new(row(&x("a"), &value, null()))
                    .from(from(*pairs, "a"))
                    .into_query()
            },
            Op::NodeRows(r) => SelectBuilder::new(row(&x("a"), &TermExpr::null(), null()))
                .from(from(*r, "a"))
                .into_query(),
            Op::Repeat { rel, bag } => SelectBuilder::new(items(sort, "a"))
                .from(from(*rel, "a"))
                .join(join(from(*bag, "b"), x("b").same(&x("a"))))
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
                    .from(from(*pairs, "a"))
                    .filter(self.pred(counted, &Scope::new("a"))?)
                    .group_by(vec![f.kind, f.lex, f.datatype, f.lang])
                    .into_query();
                let n = function("COALESCE", vec![col("c", "n"), number(0)]);
                SelectBuilder::new(row(&x("F"), &TermExpr::null(), null()))
                    .from(from(*focus, "F"))
                    .join(left_join(derived(counts, "c"), x("c").same(&x("F"))))
                    .filter(compare(n, binary(*op), number(*bound)))
                    .into_query()
            },
            Op::LangDuplicates(pairs) => {
                // One row per focus node and language held by more than one value.
                let f = x("a");
                let lang = function("LOWER", vec![v("a").lang]);
                SelectBuilder::new(row(&f, &TermExpr::null(), null()))
                    .from(from(*pairs, "a"))
                    .filter(and(v("a").is_kind(LITERAL), not_eq(v("a").lang, string(""))))
                    .group_by(vec![
                        f.kind.clone(),
                        f.lex.clone(),
                        f.datatype.clone(),
                        f.lang.clone(),
                        lang,
                    ])
                    .having(compare(count_star(), BinaryOperator::Gt, number(1)))
                    .into_query()
            },
            Op::Outgoing {
                pairs,
                triples,
                allowed,
            } => {
                let p = col("t", PREDICATE_COLUMN);
                SelectBuilder::new(row(&x("a"), &v("t"), p.clone()))
                    .from(from(*pairs, "a"))
                    .join(join(from(*triples, "t"), x("t").same(&v("a"))))
                    .filter(not(in_list(p, allowed.iter().map(|a| string(a.as_str())).collect())))
                    .into_query()
            },
            Op::PairJoin { left, right, pred } => {
                let scope = Scope {
                    row: "a",
                    other: Some("o"),
                };
                SelectBuilder::new(row(&x("a"), &v("a"), null()))
                    .from(from(*left, "a"))
                    .join(join(from(*right, "o"), x("o").same(&x("a"))))
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
                    .from(from(*r, "m"))
                    .filter(TermExpr::columns("m", "f").same(&self.expr(e, scope)?))
                    .into_query(),
            ),
            Pred::PairMember(a, b, r) => exists(
                SelectBuilder::new(one())
                    .from(from(*r, "m"))
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
fn name(id: RelId) -> String {
    id.to_string()
}

fn from(id: RelId, alias: &str) -> sqlparser::ast::TableFactor {
    cte_ref(&name(id), alias)
}

fn internal(message: &str) -> SqlCompileError {
    SqlCompileError::Internal(message.to_owned())
}
