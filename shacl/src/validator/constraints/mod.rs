mod core;
#[cfg(feature = "sparql")]
mod sparql;
mod test;

use crate::error::ValidationError;
use crate::ir::{IRComponent, IRSchema, IRShape};
use crate::types::MessageMap;
use crate::validator::engine::Engine;
#[cfg(feature = "sparql")]
use crate::validator::engine::SparqlEngine;
use crate::validator::iteration::IterationStrategy;
#[cfg(feature = "sparql")]
use crate::validator::iteration::ValueNodeIteration;
use crate::validator::nodes::ValueNodes;
use crate::validator::report::ValidationResult;
#[cfg(feature = "sparql")]
use rudof_rdf::rdf_core::query::QueryRDF;
use rudof_rdf::rdf_core::term::Object;
use rudof_rdf::rdf_core::{NeighsRDF, SHACLPath};
use std::fmt::Debug;

/// Outcome of checking a single item against a constraint component.
pub(crate) enum Check {
    /// The item satisfies the constraint.
    Hold,
    /// The item violates the constraint — emit a validation result.
    Violate,
}

/// Read-mostly context threaded into [`ConstraintComponent::check`].
///
/// The engine flows as a **generic `E`** (never `&mut dyn Engine`): shape-based
/// components (`sh:node`) recurse statically through it, pure components leave
/// it untouched. The full surface (engine, shapes, path, …) is exposed so any
/// `check` can use it; today only `store` is read by a per-item `check`, the
/// recursing components overriding `validate_native` instead.
#[allow(dead_code)]
pub(crate) struct CheckCtx<'a, S: NeighsRDF, E: Engine<S>> {
    pub store: &'a S,
    pub engine: &'a mut E,
    pub source_shape: Option<&'a IRShape>,
    pub shape: &'a IRShape,
    pub maybe_path: Option<&'a SHACLPath>,
    pub shapes_graph: &'a IRSchema,
}

/// A SHACL constraint component evaluated through a fixed **template method**.
///
/// Regular components override only the three hooks — [`strategy`], [`check`],
/// [`message`] — and inherit the iterate→check→emit→collect
/// [`validate_native`](ConstraintComponent::validate_native) body. Components
/// with bespoke shapes (whole-set scans, multi-result emits, engine recursion)
/// override `validate_native` directly and leave the hooks at their defaults.
///
/// [`strategy`]: ConstraintComponent::strategy
/// [`check`]: ConstraintComponent::check
/// [`message`]: ConstraintComponent::message
pub(crate) trait ConstraintComponent<S: NeighsRDF + Debug> {
    /// How `value_nodes` is iterated (per focus node, or per value node).
    type Strategy: IterationStrategy<S>;

    fn strategy(&self) -> Self::Strategy;

    /// The overridable per-item hook. Pure components ignore `cx.engine`.
    fn check<E: Engine<S>>(
        &self,
        _item: &<Self::Strategy as IterationStrategy<S>>::Item,
        _cx: &mut CheckCtx<'_, S, E>,
    ) -> Result<Check, ValidationError> {
        Ok(Check::Hold)
    }

    /// `sh:resultMessage` text for a violation produced by the template.
    fn message(&self, _schema: &IRSchema) -> String {
        String::new()
    }

    /// Skip the whole component without iterating (e.g. `sh:minCount 0`).
    fn short_circuit(&self) -> bool {
        false
    }

    /// TEMPLATE METHOD — generic over the engine, never a trait object.
    fn validate_native<E: Engine<S>>(
        &self,
        component: &IRComponent,
        shape: &IRShape,
        store: &S,
        engine: &mut E,
        value_nodes: &ValueNodes<S>,
        source_shape: Option<&IRShape>,
        maybe_path: Option<&SHACLPath>,
        shapes_graph: &IRSchema,
    ) -> Result<Vec<ValidationResult>, ValidationError> {
        if self.short_circuit() {
            return Ok(Vec::new());
        }
        let strategy = self.strategy();
        let msg = self.message(shapes_graph);
        let mut cx = CheckCtx {
            store,
            engine,
            source_shape,
            shape,
            maybe_path,
            shapes_graph,
        };
        let mut results = Vec::new();
        for (focus_node, item) in strategy.iterate(value_nodes) {
            let Ok(focus) = S::term_as_object(focus_node) else {
                continue;
            };
            // Real evaluator errors propagate (no silent drop).
            if let Check::Violate = self.check(item, &mut cx)? {
                let component_obj = Object::iri(component.into());
                let value = strategy.to_object(item);
                let mut message = MessageMap::from(msg.as_str());
                if let Some(m) = shape.message() {
                    message = message.merge(m.to_owned(), true);
                }
                results.push(
                    ValidationResult::new(focus, component_obj, shape.severity().clone())
                        .with_source(Some(shape.id().clone()))
                        .with_message(message)
                        .with_path(maybe_path.cloned())
                        .with_value(value),
                );
            }
        }
        Ok(results)
    }

    /// SPARQL evaluation. The default runs the native template under a
    /// [`SparqlEngine`] — correct for components whose native and SPARQL logic
    /// coincide. ASK-query components override this with their query.
    #[cfg(feature = "sparql")]
    fn validate_sparql(
        &self,
        component: &IRComponent,
        shape: &IRShape,
        store: &S,
        value_nodes: &ValueNodes<S>,
        source_shape: Option<&IRShape>,
        maybe_path: Option<&SHACLPath>,
        shapes_graph: &IRSchema,
    ) -> Result<Vec<ValidationResult>, ValidationError>
    where
        S: QueryRDF,
    {
        self.validate_native::<SparqlEngine>(
            component,
            shape,
            store,
            &mut SparqlEngine::new(),
            value_nodes,
            source_shape,
            maybe_path,
            shapes_graph,
        )
    }
}

// ---------------------------------------------------------------------------
// Static dispatch — the `IRComponent` enum match *is* the dispatch. Each arm
// reconstructs the lightweight checker from the inlined component payload (or
// borrows the kept compiled struct) and calls its template method. Both the
// native and the sparql dispatch list every variant exhaustively (no wildcard),
// so the compiler forbids them from desyncing. No `&dyn`, no `ValidatorDeref`.
// ---------------------------------------------------------------------------

/// Native constraint dispatch: monomorphises a concrete `validate_native::<E>`
/// per component — no trait object is created.
pub(crate) fn validate_native<S: NeighsRDF + Debug, E: Engine<S>>(
    component: &IRComponent,
    shape: &IRShape,
    store: &S,
    engine: &mut E,
    value_nodes: &ValueNodes<S>,
    source_shape: Option<&IRShape>,
    maybe_path: Option<&SHACLPath>,
    schema: &IRSchema,
) -> Result<Vec<ValidationResult>, ValidationError> {
    macro_rules! run {
        ($checker:expr) => {
            $checker.validate_native(component, shape, store, engine, value_nodes, source_shape, maybe_path, schema)
        };
    }
    match component {
        IRComponent::Class(c) => run!(core::value::Class(c)),
        IRComponent::Datatype(c) => run!(core::value::Datatype(c)),
        IRComponent::NodeKind(c) => run!(core::value::Nodekind(c)),
        IRComponent::MinCount(c) => run!(core::cardinality::MinCount(*c)),
        IRComponent::MaxCount(c) => run!(core::cardinality::MaxCount(*c)),
        IRComponent::MinExclusive(c) => run!(core::value_range::MinExclusive(c)),
        IRComponent::MaxExclusive(c) => run!(core::value_range::MaxExclusive(c)),
        IRComponent::MinInclusive(c) => run!(core::value_range::MinInclusive(c)),
        IRComponent::MaxInclusive(c) => run!(core::value_range::MaxInclusive(c)),
        IRComponent::MinLength(c) => run!(core::string_based::MinLength(*c)),
        IRComponent::MaxLength(c) => run!(core::string_based::MaxLength(*c)),
        IRComponent::Pattern(c) => run!(c),
        IRComponent::UniqueLang(c) => run!(core::string_based::UniqueLang(*c)),
        IRComponent::LanguageIn(c) => run!(core::string_based::LanguageIn(c)),
        IRComponent::Equals(c) => run!(core::property_pair::Equals(c)),
        IRComponent::Disjoint(c) => run!(core::property_pair::Disjoint(c)),
        IRComponent::LessThan(c) => run!(core::property_pair::LessThan(c)),
        IRComponent::LessThanOrEquals(c) => run!(core::property_pair::LessThanOrEquals(c)),
        IRComponent::Or(c) => run!(c),
        IRComponent::And(c) => run!(c),
        IRComponent::Not(c) => run!(c),
        IRComponent::Xone(c) => run!(c),
        IRComponent::Node(c) => run!(c),
        IRComponent::HasValue(c) => run!(core::other::HasValue(c)),
        IRComponent::In(c) => run!(core::other::In(c)),
        IRComponent::QualifiedValueShape(c) => run!(c),
        IRComponent::Closed(c) => run!(c),
        IRComponent::Deactivated(_) => run!(core::non_shape::Deactivated),
        IRComponent::BasicSparql(c) => run!(c),
    }
}

/// SPARQL constraint dispatch (mirror of [`validate_native`], same variants).
#[cfg(feature = "sparql")]
pub(crate) fn validate_sparql<S: QueryRDF + NeighsRDF + Debug>(
    component: &IRComponent,
    shape: &IRShape,
    store: &S,
    value_nodes: &ValueNodes<S>,
    source_shape: Option<&IRShape>,
    maybe_path: Option<&SHACLPath>,
    schema: &IRSchema,
) -> Result<Vec<ValidationResult>, ValidationError> {
    macro_rules! run {
        ($checker:expr) => {
            $checker.validate_sparql(component, shape, store, value_nodes, source_shape, maybe_path, schema)
        };
    }
    match component {
        IRComponent::Class(c) => run!(core::value::Class(c)),
        IRComponent::Datatype(c) => run!(core::value::Datatype(c)),
        IRComponent::NodeKind(c) => run!(core::value::Nodekind(c)),
        IRComponent::MinCount(c) => run!(core::cardinality::MinCount(*c)),
        IRComponent::MaxCount(c) => run!(core::cardinality::MaxCount(*c)),
        IRComponent::MinExclusive(c) => run!(core::value_range::MinExclusive(c)),
        IRComponent::MaxExclusive(c) => run!(core::value_range::MaxExclusive(c)),
        IRComponent::MinInclusive(c) => run!(core::value_range::MinInclusive(c)),
        IRComponent::MaxInclusive(c) => run!(core::value_range::MaxInclusive(c)),
        IRComponent::MinLength(c) => run!(core::string_based::MinLength(*c)),
        IRComponent::MaxLength(c) => run!(core::string_based::MaxLength(*c)),
        IRComponent::Pattern(c) => run!(c),
        IRComponent::UniqueLang(c) => run!(core::string_based::UniqueLang(*c)),
        IRComponent::LanguageIn(c) => run!(core::string_based::LanguageIn(c)),
        IRComponent::Equals(c) => run!(core::property_pair::Equals(c)),
        IRComponent::Disjoint(c) => run!(core::property_pair::Disjoint(c)),
        IRComponent::LessThan(c) => run!(core::property_pair::LessThan(c)),
        IRComponent::LessThanOrEquals(c) => run!(core::property_pair::LessThanOrEquals(c)),
        IRComponent::Or(c) => run!(c),
        IRComponent::And(c) => run!(c),
        IRComponent::Not(c) => run!(c),
        IRComponent::Xone(c) => run!(c),
        IRComponent::Node(c) => run!(c),
        IRComponent::HasValue(c) => run!(core::other::HasValue(c)),
        IRComponent::In(c) => run!(core::other::In(c)),
        IRComponent::QualifiedValueShape(c) => run!(c),
        IRComponent::Closed(c) => run!(c),
        IRComponent::Deactivated(_) => run!(core::non_shape::Deactivated),
        IRComponent::BasicSparql(c) => run!(c),
    }
}

/// Shared ASK skeleton for the SPARQL value-range / string / class / node-kind
/// components: iterate value nodes, run the per-node ASK, emit a violation when
/// it fails. Mirrors the native template's emit (message merged with the
/// shape's `sh:message`).
#[cfg(feature = "sparql")]
pub(crate) fn sparql_ask<S: QueryRDF + NeighsRDF + Debug>(
    component: &IRComponent,
    shape: &IRShape,
    store: &S,
    value_nodes: &ValueNodes<S>,
    eval_query: impl Fn(&S::Term) -> String,
    msg: &str,
    maybe_path: Option<&SHACLPath>,
) -> Result<Vec<ValidationResult>, ValidationError> {
    let strategy = ValueNodeIteration;
    let mut results = Vec::new();
    for (focus_node, item) in strategy.iterate(value_nodes) {
        let Ok(focus) = S::term_as_object(focus_node) else {
            continue;
        };
        let violates = match store.query_ask(&eval_query(item)) {
            Ok(ask) => !ask,
            Err(err) => return Err(ValidationError::ask_query_error::<S>(err)),
        };
        if violates {
            let component_obj = Object::iri(component.into());
            let value = S::term_as_object(item).ok();
            let mut message = MessageMap::from(msg);
            if let Some(m) = shape.message() {
                message = message.merge(m.to_owned(), true);
            }
            results.push(
                ValidationResult::new(focus, component_obj, shape.severity().clone())
                    .with_source(Some(shape.id().clone()))
                    .with_message(message)
                    .with_path(maybe_path.cloned())
                    .with_value(value),
            );
        }
    }
    Ok(results)
}
