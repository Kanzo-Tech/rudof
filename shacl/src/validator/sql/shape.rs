//! Shapes → their checks, and set-based conformance.
//!
//! Two questions are asked of a shape:
//!
//! - **Its results** for a focus relation ([`ShapeCompiler::emit`]): the rows
//!   of each of its components, then — recursively — those of its property
//!   shapes, whose focus nodes are this shape's value nodes. As in the native
//!   engine, a property shape nested under a property shape is evaluated once
//!   per path that reaches a node, so its focus relation is a *bag*; its rows
//!   are computed on the distinct nodes and joined back to the bag, which
//!   repeats each result as often as the native engine does.
//! - **Which candidates fail it** ([`ShapeCompiler::fails`]): `fails(S)(C)` is
//!   the set of nodes of `C` that S reports any result for, of any severity —
//!   the native notion of conformance that `sh:node`, `sh:and`, `sh:or`,
//!   `sh:xone`, `sh:not` and the qualified value shapes test. It unions the
//!   focus nodes of every component's rows with the nodes whose values fail a
//!   nested property shape. A deactivated shape fails nothing.
//!
//! The schema's dependency graph is acyclic (recursive schemas are refused
//! before compilation), so `fails` recurses into strictly lower strata and
//! terminates; each `(shape, candidates)` pair is compiled once per context.

use crate::ir::{IRShape, ShapeLabelIdx};
use crate::validator::sql::SqlCompileError;
use crate::validator::sql::ast::{SelectBuilder, join, query, union_all_of};
use crate::validator::sql::component::{ComponentCompiler, ComponentRows};
use crate::validator::sql::context::{Ctx, Rel, node_items, rows_items};
use crate::validator::sql::dialect::SqlDialect;
use crate::validator::sql::mapping::RelationalMapping;
use crate::validator::sql::path::{PathCompiler, Start};
use crate::validator::sql::term::TermExpr;

/// A check before its query is assembled.
pub(crate) struct Pending {
    pub(crate) shape: ShapeLabelIdx,
    /// The index of the component in the shape's components.
    pub(crate) component: usize,
    pub(crate) rows: ComponentRows,
}

/// Compiles shapes: their results, and which nodes fail them.
pub(crate) struct ShapeCompiler;

impl ShapeCompiler {
    /// The focus → value pairs of `shape` for the (distinct) focus nodes `focus`.
    fn values<M, D>(ctx: &mut Ctx<'_, M, D>, shape: &IRShape, focus: &Rel) -> Result<Rel, SqlCompileError>
    where
        M: RelationalMapping + ?Sized,
        D: SqlDialect + ?Sized,
    {
        let start = Start::Nodes(focus.clone());
        match shape.path() {
            None => Ok(PathCompiler::identity(ctx, &start)),
            Some(path) => PathCompiler::pairs(ctx, path, &start),
        }
    }

    fn shape<'s, M, D>(ctx: &Ctx<'s, M, D>, idx: ShapeLabelIdx) -> Result<&'s IRShape, SqlCompileError>
    where
        M: RelationalMapping + ?Sized,
        D: SqlDialect + ?Sized,
    {
        ctx.schema
            .get_shape_from_idx(&idx)
            .ok_or_else(|| SqlCompileError::Internal(format!("shape {idx} is not in the schema")))
    }

    /// The rows of every component of `shape`, for the distinct focus nodes
    /// `focus` with values `values`.
    fn component_rows<M, D>(
        ctx: &mut Ctx<'_, M, D>,
        shape: &IRShape,
        focus: &Rel,
        values: &Rel,
    ) -> Result<Vec<(usize, ComponentRows)>, SqlCompileError>
    where
        M: RelationalMapping + ?Sized,
        D: SqlDialect + ?Sized,
    {
        let mut compiler = ComponentCompiler::new(ctx, shape, focus.clone(), values.clone());
        let mut out = Vec::new();
        for (index, component) in shape.components().iter().enumerate() {
            out.extend(compiler.compile(component)?.into_iter().map(|rows| (index, rows)));
        }
        Ok(out)
    }

    /// The nodes of `candidates` (a distinct node relation) that do not
    /// conform to the shape `idx`; `None` when none can fail.
    pub(crate) fn fails<M, D>(
        ctx: &mut Ctx<'_, M, D>,
        idx: ShapeLabelIdx,
        candidates: &Rel,
    ) -> Result<Option<Rel>, SqlCompileError>
    where
        M: RelationalMapping + ?Sized,
        D: SqlDialect + ?Sized,
    {
        let key = format!("fails {idx} {}", candidates.name());
        if let Some(memo) = ctx.memo.get(&key) {
            return Ok(memo.clone());
        }
        let shape = Self::shape(ctx, idx)?;
        if shape.deactivated() {
            ctx.memo.insert(key, None);
            return Ok(None);
        }
        let values = Self::values(ctx, shape, candidates)?;
        let mut failing = Vec::new();
        for (_, rows) in Self::component_rows(ctx, shape, candidates, &values)? {
            failing.push(rows.rows);
        }
        // A nested property shape takes this shape's value nodes as focus.
        let nested_candidates = match shape {
            IRShape::NodeShape(_) => candidates.clone(),
            IRShape::PropertyShape(_) => {
                let body = SelectBuilder::new(node_items(&TermExpr::columns("vp", "v")))
                    .distinct()
                    .from(values.from("vp"))
                    .into_query();
                ctx.add("values", body)
            },
        };
        for nested in shape.property_shapes() {
            let Some(nested_fails) = Self::fails(ctx, *nested, &nested_candidates)? else {
                continue;
            };
            let body = match shape {
                IRShape::NodeShape(_) => SelectBuilder::new(rows_items(
                    &TermExpr::columns("x", "f"),
                    &TermExpr::null(),
                    crate::validator::sql::context::no_path(),
                ))
                .from(nested_fails.from("x")),
                IRShape::PropertyShape(_) => SelectBuilder::new(rows_items(
                    &TermExpr::columns("vp", "f"),
                    &TermExpr::null(),
                    crate::validator::sql::context::no_path(),
                ))
                .from(values.from("vp"))
                .join(join(
                    nested_fails.from("x"),
                    TermExpr::columns("x", "f").same(&TermExpr::columns("vp", "v")),
                )),
            };
            failing.push(ctx.add("nested", body.into_query()));
        }
        let bodies: Vec<_> = failing
            .iter()
            .map(|rows| {
                SelectBuilder::new(node_items(&TermExpr::columns("r", "f")))
                    .from(rows.from("r"))
                    .into_set_expr()
            })
            .collect();
        let out = union_all_of(bodies, false).map(|body| ctx.add("fails", query(body)));
        ctx.memo.insert(key, out.clone());
        Ok(out)
    }

    /// The checks of `shape` for the focus relation `focus` (a bag when `bag`).
    pub(crate) fn emit<M, D>(
        ctx: &mut Ctx<'_, M, D>,
        idx: ShapeLabelIdx,
        focus: &Rel,
        bag: bool,
        out: &mut Vec<Pending>,
    ) -> Result<(), SqlCompileError>
    where
        M: RelationalMapping + ?Sized,
        D: SqlDialect + ?Sized,
    {
        let shape = Self::shape(ctx, idx)?;
        if shape.deactivated() {
            return Ok(());
        }
        let distinct = if bag { ctx.distinct_nodes(focus) } else { focus.clone() };
        let values = Self::values(ctx, shape, &distinct)?;
        for (component, mut rows) in Self::component_rows(ctx, shape, &distinct, &values)? {
            if bag {
                // One copy of each result per occurrence of its focus node.
                let r_f = TermExpr::columns("r", "f");
                let body = SelectBuilder::new(rows_items(
                    &r_f,
                    &TermExpr::columns("r", "v"),
                    crate::validator::sql::ast::col("r", crate::validator::sql::context::PATH_COLUMN),
                ))
                .from(rows.rows.from("r"))
                .join(join(focus.from("b"), TermExpr::columns("b", "f").same(&r_f)))
                .into_query();
                rows.rows = ctx.add("repeated", body);
            }
            out.push(Pending {
                shape: idx,
                component,
                rows,
            });
        }
        for nested in shape.property_shapes() {
            let (nested_focus, nested_bag) = match shape {
                IRShape::NodeShape(_) => (focus.clone(), bag),
                IRShape::PropertyShape(_) => {
                    let body = SelectBuilder::new(node_items(&TermExpr::columns("vp", "v")))
                        .from(focus.from("b"))
                        .join(join(
                            values.from("vp"),
                            TermExpr::columns("vp", "f").same(&TermExpr::columns("b", "f")),
                        ))
                        .into_query();
                    (ctx.add("nested_focus", body), true)
                },
            };
            Self::emit(ctx, *nested, &nested_focus, nested_bag, out)?;
        }
        Ok(())
    }
}
