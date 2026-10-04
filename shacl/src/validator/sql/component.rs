//! Constraint components → relations of failing `(focus, value)` rows.
//!
//! [`ComponentCompiler`] implements [`IRComponentVisitor`] arm by arm — every
//! arm, so a component added to the IR does not compile here until someone
//! writes its SQL. Its input is a shape's focus relation `F` and value relation
//! `VP` (the focus → value pairs: the identity for a node shape, the path for a
//! property shape), and each arm yields the rows the native checker would emit
//! a result for: same focus, same value (or none), same multiplicity. The
//! shape-based arms ask [`ShapeCompiler::fails`] which value nodes do not
//! conform to the shapes they name.

use crate::ir::components::{BasicSparql, Closed, If, Pattern, QualifiedValueShape};
use crate::ir::visitor::IRComponentVisitor;
use crate::ir::{IRShape, ShapeLabelIdx};
use crate::types::NodeKind;
use crate::validator::sql::SqlCompileError;
use crate::validator::sql::ast::{
    SelectBuilder, and, and_all, balanced, case, col, compare, count_star, eq, function, in_list, item, known_true,
    left_join, not, not_eq, not_exists, number, or, or_all, query, string, union_all_of,
};
use crate::validator::sql::context::{Ctx, Rel, member, no_path, node_items, rows_items};
use crate::validator::sql::dialect::SqlDialect;
use crate::validator::sql::mapping::{PREDICATE_COLUMN, RelationalMapping};
use crate::validator::sql::shape::ShapeCompiler;
use crate::validator::sql::term::{
    BLANK, EncodedTerm, IRI, LITERAL, TermExpr, compare_terms, encode_object, well_formed_for,
};
use rudof_iri::IriS;
use rudof_rdf::term::Object;
use rudof_rdf::term::literal::{ConcreteLiteral, Lang};
use rudof_rdf::vocab::ShaclVocab;
use sqlparser::ast::{BinaryOperator, Expr};

/// The rows of one constraint component.
#[derive(Debug, Clone)]
pub(crate) struct ComponentRows {
    /// The `sh:sourceConstraintComponent` of the rows.
    pub(crate) component: IriS,
    /// The rows relation (`f_*`, `v_*`, `path`).
    pub(crate) rows: Rel,
    /// Message parameters, when the component names its own (the two of
    /// `sh:qualifiedValueShape`); otherwise those of the IR component.
    pub(crate) parameters: Option<Vec<(&'static str, String)>>,
}

/// Compiles the components of one shape against its focus and value relations.
pub(crate) struct ComponentCompiler<'c, 'a, M: ?Sized, D: ?Sized> {
    pub(crate) ctx: &'c mut Ctx<'a, M, D>,
    pub(crate) shape: &'c IRShape,
    /// The focus nodes (distinct).
    pub(crate) focus: Rel,
    /// The focus → value pairs (distinct).
    pub(crate) values: Rel,
    /// The distinct value nodes, built on first use.
    value_set: Option<Rel>,
    /// The component being compiled.
    component: IriS,
}

/// `vp.v` / `vp.f`: the terms of a values row under alias `vp`.
fn vp_value() -> TermExpr {
    TermExpr::columns("vp", "v")
}

fn vp_focus() -> TermExpr {
    TermExpr::columns("vp", "f")
}

impl<'c, 'a, M, D> ComponentCompiler<'c, 'a, M, D>
where
    M: RelationalMapping + ?Sized,
    D: SqlDialect + ?Sized,
{
    pub(crate) fn new(ctx: &'c mut Ctx<'a, M, D>, shape: &'c IRShape, focus: Rel, values: Rel) -> Self {
        Self {
            ctx,
            shape,
            focus,
            values,
            value_set: None,
            component: IriS::new_unchecked(""),
        }
    }

    /// Compiles one component; an empty vector when it can never fail.
    pub(crate) fn compile(
        &mut self,
        component: &crate::ir::IRComponent,
    ) -> Result<Vec<ComponentRows>, SqlCompileError> {
        self.component = IriS::from(component);
        component.accept(self)
    }

    fn one(&self, rows: Rel) -> Vec<ComponentRows> {
        vec![ComponentRows {
            component: self.component.clone(),
            rows,
            parameters: None,
        }]
    }

    /// The distinct value nodes, as a node relation.
    fn value_set(&mut self) -> Rel {
        if let Some(rel) = &self.value_set {
            return rel.clone();
        }
        let body = SelectBuilder::new(node_items(&vp_value()))
            .distinct()
            .from(self.values.from("vp"))
            .into_query();
        let rel = self.ctx.add("values", body);
        self.value_set = Some(rel.clone());
        rel
    }

    /// Value rows (`focus`, `value`) whose value violates: `violates(vp.v)`.
    fn value_rows(&mut self, violates: Expr) -> Vec<ComponentRows> {
        let body = SelectBuilder::new(rows_items(&vp_focus(), &vp_value(), no_path()))
            .from(self.values.from("vp"))
            .filter(violates)
            .into_query();
        let rel = self.ctx.add("rows", body);
        self.one(rel)
    }

    /// Focus rows, without a value, for the focus nodes where `violates(f)`.
    fn focus_rows(&mut self, from: SelectBuilder) -> Vec<ComponentRows> {
        let rel = self.ctx.add("rows", from.into_query());
        self.one(rel)
    }

    /// The nodes of the value set that fail `shape`.
    fn fails(&mut self, shape: ShapeLabelIdx) -> Result<Option<Rel>, SqlCompileError> {
        let candidates = self.value_set();
        ShapeCompiler::fails(self.ctx, shape, &candidates)
    }

    /// A range component: the value must be a literal and `value op bound`.
    fn range(&mut self, bound: &ConcreteLiteral, op: BinaryOperator) -> Result<Vec<ComponentRows>, SqlCompileError> {
        let bound = TermExpr::object(&Object::Literal(bound.clone()))?;
        let holds = and(
            vp_value().is_kind(LITERAL),
            known_true(compare_terms(self.ctx.dialect, &vp_value(), op, &bound)?),
        );
        Ok(self.value_rows(not(holds)))
    }

    fn length(&mut self, len: isize, op: BinaryOperator) -> Vec<ComponentRows> {
        let bound = number(i64::try_from(len).unwrap_or(i64::MAX));
        let holds = compare(self.ctx.dialect.char_length(vp_value().lex), op, bound);
        self.value_rows(or(vp_value().is_kind(BLANK), not(known_true(holds))))
    }

    /// The other predicate's values for each focus node, as `(f, v)` pairs.
    fn other(&mut self, predicate: &IriS) -> Rel {
        self.ctx.predicate(predicate)
    }

    /// `sh:lessThan` / `sh:lessThanOrEquals`: one row per value and other value
    /// that are not ordered by `op`, as the native checker emits them.
    fn ordered(&mut self, predicate: &IriS, op: BinaryOperator) -> Result<Vec<ComponentRows>, SqlCompileError> {
        let other = self.other(predicate);
        let o = TermExpr::columns("o", "v");
        let holds = compare_terms(self.ctx.dialect, &vp_value(), op, &o)?;
        let body = SelectBuilder::new(rows_items(&vp_focus(), &vp_value(), no_path()))
            .from(self.values.from("vp"))
            .join(crate::validator::sql::ast::join(
                other.from("o"),
                TermExpr::columns("o", "f").same(&vp_focus()),
            ))
            .filter(not(known_true(holds)))
            .into_query();
        let rel = self.ctx.add("rows", body);
        Ok(self.one(rel))
    }

    /// Value rows `vp` for which `pairs(o)` holds against the other predicate's
    /// `(f, v)` pairs `o` (when `present`) or does not (when not).
    fn against_other(&mut self, predicate: &IriS, present: bool) -> Vec<ComponentRows> {
        let other = self.other(predicate);
        let probe = SelectBuilder::new(vec![sqlparser::ast::SelectItem::UnnamedExpr(number(1))])
            .from(other.from("o"))
            .filter(and(
                TermExpr::columns("o", "f").same(&vp_focus()),
                TermExpr::columns("o", "v").same(&vp_value()),
            ))
            .into_query();
        let test = if present {
            crate::validator::sql::ast::exists(probe)
        } else {
            not_exists(probe)
        };
        self.value_rows(and(vp_focus().is_not_kind(LITERAL), test))
    }

    /// Counts per focus node of the value nodes satisfying `counted`, joined
    /// back to every focus node (`0` for none): `(F, n)` rows filtered by `keep(n)`.
    fn per_focus_count(&mut self, counted: Expr, keep: impl Fn(Expr) -> Expr) -> Vec<ComponentRows> {
        let f = TermExpr::columns("vp", "f");
        let mut items = f.items("f");
        items.push(item(count_star(), "n"));
        let counts = SelectBuilder::new(items)
            .from(self.values.from("vp"))
            .filter(counted)
            .group_by(vec![f.kind, f.lex, f.datatype, f.lang])
            .into_query();
        let counts = self.ctx.add("counts", counts);
        let focus = TermExpr::columns("F", "f");
        let n = function("COALESCE", vec![col("c", "n"), number(0)]);
        let select = SelectBuilder::new(rows_items(&focus, &TermExpr::null(), no_path()))
            .from(self.focus.from("F"))
            .join(left_join(counts.from("c"), TermExpr::columns("c", "f").same(&focus)))
            .filter(keep(n));
        self.focus_rows(select)
    }
}

impl<M, D> IRComponentVisitor for ComponentCompiler<'_, '_, M, D>
where
    M: RelationalMapping + ?Sized,
    D: SqlDialect + ?Sized,
{
    type Output = Vec<ComponentRows>;
    type Error = SqlCompileError;

    /// Never reached: every arm below is overridden, so the IR's components and
    /// the SQL ones cannot drift apart silently.
    fn default_component(&mut self) -> Result<Self::Output, Self::Error> {
        Err(SqlCompileError::Unsupported(format!(
            "the component {}",
            self.component
        )))
    }

    // --- value type ---------------------------------------------------------

    fn visit_class(&mut self, class: &Object) -> Result<Self::Output, Self::Error> {
        let extent = match class {
            Object::Iri(iri) => Some(self.ctx.class_extent(iri)),
            _ => None,
        };
        Ok(self.value_rows(not(member(&vp_value(), extent.as_ref()))))
    }

    fn visit_datatype(&mut self, datatypes: &[IriS]) -> Result<Self::Output, Self::Error> {
        let names: Vec<String> = datatypes.iter().map(|d| d.as_str().to_owned()).collect();
        let v = vp_value();
        let holds = and_all([
            v.is_kind(LITERAL),
            in_list(v.datatype.clone(), names.iter().map(|d| string(d)).collect()),
            well_formed_for(self.ctx.dialect, &v, &names)?,
        ]);
        Ok(self.value_rows(not(holds)))
    }

    fn visit_node_kind(&mut self, node_kind: &NodeKind) -> Result<Self::Output, Self::Error> {
        let kinds: &[&str] = match node_kind {
            NodeKind::Iri => &[IRI],
            NodeKind::Lit => &[LITERAL],
            NodeKind::BNode => &[BLANK],
            NodeKind::BNodeOrIri => &[BLANK, IRI],
            NodeKind::BNodeOrLit => &[BLANK, LITERAL],
            NodeKind::IriOrLit => &[IRI, LITERAL],
        };
        let holds = in_list(vp_value().kind, kinds.iter().map(|k| string(k)).collect());
        Ok(self.value_rows(not(holds)))
    }

    // --- cardinality ----------------------------------------------------------

    fn visit_min_count(&mut self, count: isize) -> Result<Self::Output, Self::Error> {
        if count <= 0 {
            return Ok(Vec::new());
        }
        let bound = number(i64::try_from(count).unwrap_or(i64::MAX));
        Ok(self.per_focus_count(crate::validator::sql::ast::boolean(true), |n| {
            compare(n, BinaryOperator::Lt, bound.clone())
        }))
    }

    fn visit_max_count(&mut self, count: isize) -> Result<Self::Output, Self::Error> {
        let bound = number(i64::try_from(count).unwrap_or(i64::MAX));
        Ok(self.per_focus_count(crate::validator::sql::ast::boolean(true), |n| {
            compare(n, BinaryOperator::Gt, bound.clone())
        }))
    }

    // --- value range ----------------------------------------------------------

    fn visit_min_exclusive(&mut self, lit: &ConcreteLiteral) -> Result<Self::Output, Self::Error> {
        self.range(lit, BinaryOperator::Gt)
    }

    fn visit_max_exclusive(&mut self, lit: &ConcreteLiteral) -> Result<Self::Output, Self::Error> {
        self.range(lit, BinaryOperator::Lt)
    }

    fn visit_min_inclusive(&mut self, lit: &ConcreteLiteral) -> Result<Self::Output, Self::Error> {
        self.range(lit, BinaryOperator::GtEq)
    }

    fn visit_max_inclusive(&mut self, lit: &ConcreteLiteral) -> Result<Self::Output, Self::Error> {
        self.range(lit, BinaryOperator::LtEq)
    }

    // --- string ---------------------------------------------------------------

    fn visit_min_length(&mut self, len: isize) -> Result<Self::Output, Self::Error> {
        Ok(self.length(len, BinaryOperator::GtEq))
    }

    fn visit_max_length(&mut self, len: isize) -> Result<Self::Output, Self::Error> {
        Ok(self.length(len, BinaryOperator::LtEq))
    }

    fn visit_pattern(&mut self, pattern: &Pattern) -> Result<Self::Output, Self::Error> {
        let matches =
            self.ctx
                .dialect
                .regex_match(vp_value().lex, pattern.pattern(), pattern.flags().map(String::as_str))?;
        Ok(self.value_rows(or(vp_value().is_kind(BLANK), not(known_true(matches)))))
    }

    fn visit_unique_lang(&mut self, unique: bool) -> Result<Self::Output, Self::Error> {
        if !unique {
            return Ok(Vec::new());
        }
        // One result per focus node and language held by more than one value.
        let f = vp_focus();
        let lang = function("LOWER", vec![vp_value().lang]);
        let body = SelectBuilder::new(rows_items(&f, &TermExpr::null(), no_path()))
            .from(self.values.from("vp"))
            .filter(and(vp_value().is_kind(LITERAL), not_eq(vp_value().lang, string(""))))
            .group_by(vec![
                f.kind.clone(),
                f.lex.clone(),
                f.datatype.clone(),
                f.lang.clone(),
                lang,
            ])
            .having(compare(count_star(), BinaryOperator::Gt, number(1)))
            .into_query();
        let rel = self.ctx.add("rows", body);
        Ok(self.one(rel))
    }

    fn visit_language_in(&mut self, langs: &[Lang]) -> Result<Self::Output, Self::Error> {
        let v = vp_value();
        let tag = function("LOWER", vec![v.lang.clone()]);
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
        let holds = and_all([v.is_kind(LITERAL), not_eq(v.lang.clone(), string("")), any]);
        Ok(self.value_rows(not(holds)))
    }

    // --- property pair ----------------------------------------------------------

    fn visit_equals(&mut self, iri: &IriS) -> Result<Self::Output, Self::Error> {
        // Values the other predicate lacks, then other values the path lacks.
        let missing = self.against_other(iri, false);
        let other = self.other(iri);
        let o_f = TermExpr::columns("o", "f");
        let o_v = TermExpr::columns("o", "v");
        let extra = SelectBuilder::new(rows_items(&o_f, &o_v, no_path()))
            .from(other.from("o"))
            .join(crate::validator::sql::ast::join(
                self.focus.from("F"),
                TermExpr::columns("F", "f").same(&o_f),
            ))
            .filter(o_f.is_not_kind(LITERAL))
            .filter(not_exists(
                SelectBuilder::new(vec![sqlparser::ast::SelectItem::UnnamedExpr(number(1))])
                    .from(self.values.from("vp"))
                    .filter(and(vp_focus().same(&o_f), vp_value().same(&o_v)))
                    .into_query(),
            ));
        let mut bodies = Vec::new();
        for part in &missing {
            bodies.push(
                SelectBuilder::new(rows_items(
                    &TermExpr::columns("m", "f"),
                    &TermExpr::columns("m", "v"),
                    col("m", "path"),
                ))
                .from(part.rows.from("m"))
                .into_set_expr(),
            );
        }
        bodies.push(extra.into_set_expr());
        let body = union_all_of(bodies, true)
            .map(query)
            .ok_or_else(|| SqlCompileError::Internal("sh:equals compiled to no relation".to_owned()))?;
        let rel = self.ctx.add("rows", body);
        Ok(self.one(rel))
    }

    fn visit_disjoint(&mut self, iri: &IriS) -> Result<Self::Output, Self::Error> {
        Ok(self.against_other(iri, true))
    }

    fn visit_less_than(&mut self, iri: &IriS) -> Result<Self::Output, Self::Error> {
        self.ordered(iri, BinaryOperator::Lt)
    }

    fn visit_less_than_or_equals(&mut self, iri: &IriS) -> Result<Self::Output, Self::Error> {
        self.ordered(iri, BinaryOperator::LtEq)
    }

    // --- logical ------------------------------------------------------------------

    fn visit_or(&mut self, shapes: &[ShapeLabelIdx]) -> Result<Self::Output, Self::Error> {
        let mut fails_all = Vec::new();
        for shape in shapes {
            let fails = self.fails(*shape)?;
            fails_all.push(member(&vp_value(), fails.as_ref()));
        }
        Ok(self.value_rows(and_all(fails_all)))
    }

    fn visit_and(&mut self, shapes: &[ShapeLabelIdx]) -> Result<Self::Output, Self::Error> {
        let mut fails_any = Vec::new();
        for shape in shapes {
            let fails = self.fails(*shape)?;
            fails_any.push(member(&vp_value(), fails.as_ref()));
        }
        Ok(self.value_rows(or_all(fails_any)))
    }

    fn visit_not(&mut self, shape: ShapeLabelIdx) -> Result<Self::Output, Self::Error> {
        let fails = self.fails(shape)?;
        Ok(self.value_rows(not(member(&vp_value(), fails.as_ref()))))
    }

    fn visit_xone(&mut self, shapes: &[ShapeLabelIdx]) -> Result<Self::Output, Self::Error> {
        let mut conforming = Vec::new();
        for shape in shapes {
            let fails = self.fails(*shape)?;
            conforming.push(case(vec![(member(&vp_value(), fails.as_ref()), number(0))], number(1)));
        }
        let count = balanced(conforming, &|a, b| compare(a, BinaryOperator::Plus, b)).unwrap_or_else(|| number(0));
        Ok(self.value_rows(not_eq(count, number(1))))
    }

    fn visit_if(&mut self, if_: &If) -> Result<Self::Output, Self::Error> {
        let cond = self.fails(*if_.cond())?;
        let conforms_cond = not(member(&vp_value(), cond.as_ref()));
        let mut violations = Vec::new();
        if let Some(then) = if_.then() {
            let then = self.fails(*then)?;
            violations.push(and(conforms_cond.clone(), member(&vp_value(), then.as_ref())));
        }
        if let Some(els) = if_.els() {
            let els = self.fails(*els)?;
            violations.push(and(not(conforms_cond), member(&vp_value(), els.as_ref())));
        }
        if violations.is_empty() {
            return Ok(Vec::new());
        }
        Ok(self.value_rows(or_all(violations)))
    }

    // --- shape-based ----------------------------------------------------------------

    fn visit_node(&mut self, shape: ShapeLabelIdx) -> Result<Self::Output, Self::Error> {
        let fails = self.fails(shape)?;
        if fails.is_none() {
            return Ok(Vec::new());
        }
        Ok(self.value_rows(member(&vp_value(), fails.as_ref())))
    }

    fn visit_qualified_value_shape(&mut self, qvs: &QualifiedValueShape) -> Result<Self::Output, Self::Error> {
        // A value counts when it conforms to the shape and, for
        // sh:qualifiedValueShapesDisjoint, to none of the sibling shapes.
        let fails = self.fails(*qvs.shape())?;
        let mut counted = vec![not(member(&vp_value(), fails.as_ref()))];
        for sibling in qvs.siblings() {
            let sibling_fails = self.fails(*sibling)?;
            counted.push(member(&vp_value(), sibling_fails.as_ref()));
        }
        let counted = and_all(counted);
        let mut out = Vec::new();
        if let Some(min) = qvs.qualified_min_count() {
            let bound = number(i64::try_from(min).unwrap_or(i64::MAX));
            let mut rows = self.per_focus_count(counted.clone(), |n| compare(n, BinaryOperator::Lt, bound.clone()));
            for r in &mut rows {
                r.component = ShaclVocab::sh_qualified_min_count_constraint_component();
                r.parameters = Some(vec![("qualifiedMinCount", min.to_string())]);
            }
            out.extend(rows);
        }
        if let Some(max) = qvs.qualified_max_count() {
            let bound = number(i64::try_from(max).unwrap_or(i64::MAX));
            let mut rows = self.per_focus_count(counted, |n| compare(n, BinaryOperator::Gt, bound.clone()));
            for r in &mut rows {
                r.component = ShaclVocab::sh_qualified_max_count_constraint_component();
                r.parameters = Some(vec![("qualifiedMaxCount", max.to_string())]);
            }
            out.extend(rows);
        }
        Ok(out)
    }

    fn visit_closed(&mut self, closed: &Closed) -> Result<Self::Output, Self::Error> {
        if !closed.is_closed() {
            return Ok(Vec::new());
        }
        // SHACL §4.8.1: every triple of a value node (the focus node itself
        // for a node shape) whose predicate is neither a property of the shape
        // nor ignored, with the predicate as the result path and the object as
        // the value.
        let mut allowed: Vec<String> = self
            .shape
            .allowed_properties()
            .iter()
            .map(|p| p.as_str().to_owned())
            .collect();
        allowed.sort();
        let triples = self.ctx.triples();
        let p = col("t", PREDICATE_COLUMN);
        let body = SelectBuilder::new(rows_items(&vp_focus(), &TermExpr::columns("t", "v"), p.clone()))
            .from(self.values.from("vp"))
            .join(crate::validator::sql::ast::join(
                triples.from("t"),
                TermExpr::columns("t", "f").same(&vp_value()),
            ))
            .filter(not(in_list(p, allowed.iter().map(|a| string(a)).collect())))
            .into_query();
        let rel = self.ctx.add("rows", body);
        Ok(self.one(rel))
    }

    fn visit_has_value(&mut self, value: &Object) -> Result<Self::Output, Self::Error> {
        let value = TermExpr::object(value)?;
        let focus = TermExpr::columns("F", "f");
        let select = SelectBuilder::new(rows_items(&focus, &TermExpr::null(), no_path()))
            .from(self.focus.from("F"))
            .filter(not_exists(
                SelectBuilder::new(vec![sqlparser::ast::SelectItem::UnnamedExpr(number(1))])
                    .from(self.values.from("vp"))
                    .filter(and(vp_focus().same(&focus), vp_value().same(&value)))
                    .into_query(),
            ));
        Ok(self.focus_rows(select))
    }

    fn visit_in(&mut self, values: &[Object]) -> Result<Self::Output, Self::Error> {
        let encoded = values
            .iter()
            .map(encode_object)
            .collect::<Result<Vec<EncodedTerm>, _>>()?;
        Ok(self.value_rows(not(vp_value().in_terms(&encoded))))
    }

    // --- status / SPARQL ----------------------------------------------------------------

    fn visit_deactivated(&mut self, _deactivated: bool) -> Result<Self::Output, Self::Error> {
        // A deactivated shape is never compiled; on an active one the
        // component itself raises nothing.
        Ok(Vec::new())
    }

    fn visit_basic_sparql(&mut self, _sparql: &BasicSparql) -> Result<Self::Output, Self::Error> {
        Err(SqlCompileError::Unsupported(
            "sh:sparql (SHACL-SPARQL is outside SHACL Core)".to_owned(),
        ))
    }
}
