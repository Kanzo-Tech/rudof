//! The CTEs one check's query is built from.
//!
//! Every relation the compilers produce is a named CTE in a [`Ctx`], and a
//! [`Rel`] is its name. A check's query is `WITH <the CTEs it reaches> SELECT …
//! FROM <its rows>`: [`Ctx::finish`] keeps only the CTEs the result transitively
//! references, found by walking the AST, so each check is a self-contained
//! query and none carries another's relations.
//!
//! Column conventions inside a context: a node relation has the term columns
//! `f_*`; a pair relation (focus → value) `f_*` and `v_*`; a rows relation
//! `f_*`, `v_*` (all `NULL` when the result has no value) and `path`.

use crate::ir::IRSchema;
use crate::validator::sql::SqlCompileError;
use crate::validator::sql::ast::{SelectBuilder, boolean, col, cte, cte_ref, derived, exists, item, null, query, with};
use crate::validator::sql::dialect::SqlDialect;
use crate::validator::sql::mapping::{PREDICATE_COLUMN, RelationalMapping};
use crate::validator::sql::term::{EncodedTerm, TermExpr};
use rudof_iri::IriS;
use sqlparser::ast::{Cte, Expr, ObjectName, ObjectNamePart, Query, SelectItem, visit_relations};
use std::collections::{BTreeSet, HashMap};
use std::ops::ControlFlow;

/// The column of a rows relation that overrides the result path (`sh:closed`).
pub(crate) const PATH_COLUMN: &str = "path";

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

/// A relation in a [`Ctx`]: the name of its CTE.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct Rel(String);

impl Rel {
    pub(crate) fn name(&self) -> &str {
        &self.0
    }

    /// The relation in `FROM`, under `alias`.
    pub(crate) fn from(&self, alias: &str) -> sqlparser::ast::TableFactor {
        cte_ref(&self.0, alias)
    }
}

struct Entry {
    name: String,
    cte: Cte,
    deps: Vec<usize>,
}

/// The compiler's state for one check family: its CTEs, and the context it
/// compiles against.
pub(crate) struct Ctx<'a, M: ?Sized, D: ?Sized> {
    pub(crate) mapping: &'a M,
    pub(crate) dialect: &'a D,
    pub(crate) schema: &'a IRSchema,
    entries: Vec<Entry>,
    index: HashMap<String, usize>,
    recursive: BTreeSet<usize>,
    pub(crate) memo: HashMap<String, Option<Rel>>,
}

/// The projection of a node relation's term under `prefix`.
pub(crate) fn node_items(t: &TermExpr) -> Vec<SelectItem> {
    t.items("f")
}

/// The projection of a pair relation.
pub(crate) fn pair_items(f: &TermExpr, v: &TermExpr) -> Vec<SelectItem> {
    let mut items = f.items("f");
    items.extend(v.items("v"));
    items
}

/// The projection of a rows relation.
pub(crate) fn rows_items(f: &TermExpr, v: &TermExpr, path: Expr) -> Vec<SelectItem> {
    let mut items = pair_items(f, v);
    items.push(item(path, PATH_COLUMN));
    items
}

/// `EXISTS (SELECT 1 FROM rel x WHERE x.f = term)`; `FALSE` for a relation
/// known to be empty.
pub(crate) fn member(term: &TermExpr, rel: Option<&Rel>) -> Expr {
    match rel {
        None => boolean(false),
        Some(rel) => exists(
            SelectBuilder::new(vec![SelectItem::UnnamedExpr(crate::validator::sql::ast::number(1))])
                .from(rel.from("m"))
                .filter(TermExpr::columns("m", "f").same(term))
                .into_query(),
        ),
    }
}

impl<'a, M: RelationalMapping + ?Sized, D: SqlDialect + ?Sized> Ctx<'a, M, D> {
    pub(crate) fn new(schema: &'a IRSchema, mapping: &'a M, dialect: &'a D) -> Self {
        Self {
            mapping,
            dialect,
            schema,
            entries: Vec::new(),
            index: HashMap::new(),
            recursive: BTreeSet::new(),
            memo: HashMap::new(),
        }
    }

    /// A fresh CTE name with a readable hint.
    pub(crate) fn reserve(&self, hint: &str) -> String {
        format!("{hint}_{}", self.entries.len())
    }

    fn deps_of(&self, name: &str, body: &Query) -> Vec<usize> {
        let mut deps = Vec::new();
        let _ = visit_relations(body, |object: &ObjectName| {
            if let [ObjectNamePart::Identifier(ident)] = object.0.as_slice()
                && ident.value != name
                && let Some(i) = self.index.get(&ident.value)
            {
                deps.push(*i);
            }
            ControlFlow::<()>::Continue(())
        });
        deps.sort_unstable();
        deps.dedup();
        deps
    }

    /// Defines the CTE `name`; `recursive` when its body refers to itself.
    pub(crate) fn define(&mut self, name: String, body: Query, recursive: bool) -> Rel {
        let deps = self.deps_of(&name, &body);
        let position = self.entries.len();
        if recursive {
            self.recursive.insert(position);
        }
        self.index.insert(name.clone(), position);
        self.entries.push(Entry {
            cte: cte(&name, body),
            name: name.clone(),
            deps,
        });
        Rel(name)
    }

    /// A new CTE for `body`.
    pub(crate) fn add(&mut self, hint: &str, body: Query) -> Rel {
        let name = self.reserve(hint);
        self.define(name, body, false)
    }

    /// An empty node relation (`f_*`).
    pub(crate) fn empty_nodes(&mut self) -> Rel {
        let body = SelectBuilder::new(node_items(&TermExpr::constant(&EncodedTerm::default())))
            .filter(boolean(false))
            .into_query();
        self.add("empty", body)
    }

    /// An empty pair relation.
    pub(crate) fn empty_pairs(&mut self) -> Rel {
        let t = TermExpr::constant(&EncodedTerm::default());
        let body = SelectBuilder::new(pair_items(&t, &t))
            .filter(boolean(false))
            .into_query();
        self.add("empty", body)
    }

    /// The SHACL instances of `class`, as a node relation.
    pub(crate) fn class_extent(&mut self, class: &IriS) -> Rel {
        let key = format!("class {class}");
        if let Some(Some(rel)) = self.memo.get(&key) {
            return rel.clone();
        }
        let rel = match self.mapping.class_extent(class) {
            Some(extent) => {
                let body = SelectBuilder::new(node_items(&TermExpr::columns("m", "n")))
                    .distinct()
                    .from(derived(extent.0, "m"))
                    .into_query();
                self.add("class", body)
            },
            None => self.empty_nodes(),
        };
        self.memo.insert(key, Some(rel.clone()));
        rel
    }

    /// The triples of `predicate`, as a pair relation (subject → object).
    pub(crate) fn predicate(&mut self, predicate: &IriS) -> Rel {
        let key = format!("predicate {predicate}");
        if let Some(Some(rel)) = self.memo.get(&key) {
            return rel.clone();
        }
        let rel = match self.mapping.predicate(predicate) {
            Some(edges) => {
                let body = SelectBuilder::new(pair_items(&TermExpr::columns("m", "s"), &TermExpr::columns("m", "o")))
                    .distinct()
                    .from(derived(edges.0, "m"))
                    .into_query();
                self.add("pred", body)
            },
            None => self.empty_pairs(),
        };
        self.memo.insert(key, Some(rel.clone()));
        rel
    }

    /// Every triple: `f_*` (subject), `p`, `v_*` (object).
    pub(crate) fn triples(&mut self) -> Rel {
        let key = "triples".to_owned();
        if let Some(Some(rel)) = self.memo.get(&key) {
            return rel.clone();
        }
        let mut items = TermExpr::columns("m", "s").items("f");
        items.push(item(col("m", PREDICATE_COLUMN), PREDICATE_COLUMN));
        items.extend(TermExpr::columns("m", "o").items("v"));
        let body = SelectBuilder::new(items)
            .from(derived(self.mapping.triples(), "m"))
            .into_query();
        let rel = self.add("triples", body);
        self.memo.insert(key, Some(rel.clone()));
        rel
    }

    /// Every node of the data: the subjects and objects of its triples.
    pub(crate) fn nodes(&mut self) -> Rel {
        let key = "nodes".to_owned();
        if let Some(Some(rel)) = self.memo.get(&key) {
            return rel.clone();
        }
        let triples = self.triples();
        let subjects = SelectBuilder::new(node_items(&TermExpr::columns("t", "f"))).from(triples.from("t"));
        let objects = SelectBuilder::new(node_items(&TermExpr::columns("t", "v"))).from(triples.from("t"));
        let body = query(crate::validator::sql::ast::union(
            subjects.into_set_expr(),
            objects.into_set_expr(),
            false,
        ));
        let rel = self.add("nodes", body);
        self.memo.insert(key, Some(rel.clone()));
        rel
    }

    /// `SELECT DISTINCT f FROM rel` for a node relation that may hold duplicates.
    pub(crate) fn distinct_nodes(&mut self, rel: &Rel) -> Rel {
        let body = SelectBuilder::new(node_items(&TermExpr::columns("b", "f")))
            .distinct()
            .from(rel.from("b"))
            .into_query();
        self.add("focus", body)
    }

    /// The query of a check: the CTEs `rows` reaches, then its rows under the
    /// public [`RESULT_COLUMNS`].
    pub(crate) fn finish(&self, rows: &Rel) -> Result<Query, SqlCompileError> {
        let root = *self
            .index
            .get(rows.name())
            .ok_or_else(|| SqlCompileError::Internal(format!("unknown relation {}", rows.name())))?;
        let mut needed = BTreeSet::new();
        let mut pending = vec![root];
        while let Some(i) = pending.pop() {
            if needed.insert(i) {
                pending.extend(self.entries[i].deps.iter().copied());
            }
        }
        let recursive = needed.iter().any(|i| self.recursive.contains(i));
        let ctes = needed.iter().map(|i| self.entries[*i].cte.clone()).collect();
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
        let body = SelectBuilder::new(projection).from(rows.from("r")).into_query();
        let _ = &self.entries[root].name;
        Ok(with(ctes, recursive, body))
    }
}

/// `NULL` in the path column.
pub(crate) fn no_path() -> Expr {
    null()
}
