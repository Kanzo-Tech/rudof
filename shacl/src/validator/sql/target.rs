//! Targets → the focus relation of a shape.
//!
//! Each target declaration is one node relation and the focus nodes are their
//! union, without duplicates — as the native engine collects them into a set.

use crate::ir::IRShape;
use crate::types::Target;
use crate::validator::sql::SqlCompileError;
use crate::validator::sql::ast::{SelectBuilder, derived, query, union_all_of};
use crate::validator::sql::context::{Ctx, Rel, node_items};
use crate::validator::sql::dialect::SqlDialect;
use crate::validator::sql::mapping::RelationalMapping;
use crate::validator::sql::term::TermExpr;
use rudof_rdf::term::Object;

/// Compiles the targets of a shape into its focus relation.
pub(crate) struct TargetCompiler;

impl TargetCompiler {
    pub(crate) fn focus<M, D>(ctx: &mut Ctx<'_, M, D>, shape: &IRShape) -> Result<Rel, SqlCompileError>
    where
        M: RelationalMapping + ?Sized,
        D: SqlDialect + ?Sized,
    {
        let mut parts = Vec::new();
        for target in shape.targets() {
            parts.push(Self::target(ctx, target)?);
        }
        let bodies = parts
            .iter()
            .map(|rel| {
                SelectBuilder::new(node_items(&TermExpr::columns("t", "f")))
                    .from(rel.from("t"))
                    .into_set_expr()
            })
            .collect();
        Ok(match union_all_of(bodies, true) {
            // The focus nodes are a set, as the native engine collects them.
            Some(body) => {
                let body = SelectBuilder::new(node_items(&TermExpr::columns("t", "f")))
                    .distinct()
                    .from(derived(query(body), "t"))
                    .into_query();
                ctx.add("targets", body)
            },
            None => ctx.empty_nodes(),
        })
    }

    fn target<M, D>(ctx: &mut Ctx<'_, M, D>, target: &Target) -> Result<Rel, SqlCompileError>
    where
        M: RelationalMapping + ?Sized,
        D: SqlDialect + ?Sized,
    {
        Ok(match target {
            Target::Node(node) => {
                if matches!(node, Object::BlankNode(_)) {
                    return Err(SqlCompileError::MalformedTarget(format!(
                        "sh:targetNode {node} is a blank node"
                    )));
                }
                let body = SelectBuilder::new(node_items(&TermExpr::object(node)?)).into_query();
                ctx.add("target_node", body)
            },
            Target::Class(class) | Target::ImplicitClass(class) => match class {
                Object::Iri(iri) => ctx.class_extent(iri),
                // Only an IRI names a class a mapping can hold.
                _ => ctx.empty_nodes(),
            },
            Target::SubjectsOf(predicate) => {
                let edges = ctx.predicate(predicate);
                let body = SelectBuilder::new(node_items(&TermExpr::columns("e", "f")))
                    .distinct()
                    .from(edges.from("e"))
                    .into_query();
                ctx.add("subjects_of", body)
            },
            Target::ObjectsOf(predicate) => {
                let edges = ctx.predicate(predicate);
                let body = SelectBuilder::new(node_items(&TermExpr::columns("e", "v")))
                    .distinct()
                    .from(edges.from("e"))
                    .into_query();
                ctx.add("objects_of", body)
            },
            Target::Where(shape) => {
                return Err(SqlCompileError::Unsupported(format!(
                    "sh:targetWhere {shape} (SHACL 1.2)"
                )));
            },
            Target::WrongNode(_)
            | Target::WrongClass(_)
            | Target::WrongSubjectsOf(_)
            | Target::WrongObjectsOf(_)
            | Target::WrongImplicitClass(_) => {
                return Err(SqlCompileError::MalformedTarget(format!(
                    "{target}: the target value has the wrong term kind"
                )));
            },
        })
    }
}
