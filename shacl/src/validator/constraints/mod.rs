mod core;
#[cfg(feature = "sparql")]
mod sparql;
mod test;

use crate::error::ValidationError;
use crate::ir::components::{
    And, Closed, Datatype, Deactivated, HasValue, In, LanguageIn, MaxCount, MinCount, Node, Not, Or,
    QualifiedValueShape, UniqueLang, Xone,
};
use crate::ir::{IRComponent, IRSchema, IRShape};
use crate::types::MessageMap;
use crate::validator::engine::Engine;
use crate::validator::iteration::IterationStrategy;
#[cfg(feature = "sparql")]
use crate::validator::iteration::ValueNodeIteration;
use crate::validator::nodes::ValueNodes;
use crate::validator::report::ValidationResult;
#[cfg(feature = "sparql")]
use rudof_rdf::rdf_core::query::QueryRDF;
use rudof_rdf::rdf_core::term::Object;
use rudof_rdf::rdf_core::{NeighsRDF, Rdf, SHACLPath};
use std::fmt::Debug;

/// Components whose native and SPARQL logic are identical implement this single
/// trait; the [`impl_validators_via_validate!`] macro derives both
/// [`NativeValidator`] and [`BasicSparqlValidator`] from it.
///
/// The engine flows as a **generic `E`**, never `&mut dyn Engine`, so the
/// shape-based components (`sh:node`, `sh:or`, …) recurse statically.
pub trait Validator<RDF: NeighsRDF + Debug> {
    fn validate<E: Engine<RDF>>(
        &self,
        component: &IRComponent,
        shape: &IRShape,
        store: &RDF,
        engine: &mut E,
        value_nodes: &ValueNodes<RDF>,
        source_shape: Option<&IRShape>,
        maybe_path: Option<&SHACLPath>,
        shapes_graph: &IRSchema,
    ) -> Result<Vec<ValidationResult>, ValidationError>;
}

/// Native validation of a single component. The method is **generic over the
/// engine** (so it is not object-safe — and it never needs to be, since the
/// [`validate_native`] dispatcher calls it on the concrete component value).
pub trait NativeValidator<RDF: NeighsRDF> {
    fn validate_native<E: Engine<RDF>>(
        &self,
        component: &IRComponent,
        shape: &IRShape,
        store: &RDF,
        engine: &mut E,
        value_nodes: &ValueNodes<RDF>,
        source_shape: Option<&IRShape>,
        maybe_path: Option<&SHACLPath>,
        shapes_graph: &IRSchema,
    ) -> Result<Vec<ValidationResult>, ValidationError>;
}

#[cfg(feature = "sparql")]
pub trait BasicSparqlValidator<RDF: QueryRDF + Debug> {
    fn validate_sparql(
        &self,
        component: &IRComponent,
        shape: &IRShape,
        store: &RDF,
        value_nodes: &ValueNodes<RDF>,
        source_shape: Option<&IRShape>,
        maybe_path: Option<&SHACLPath>,
        shapes_graph: &IRSchema,
    ) -> Result<Vec<ValidationResult>, ValidationError>;
}

macro_rules! impl_validators_via_validate {
    ($ty:ty) => {
        impl<S> crate::validator::constraints::NativeValidator<S> for $ty
        where
            S: rudof_rdf::rdf_core::NeighsRDF + std::fmt::Debug + 'static,
        {
            fn validate_native<E: crate::validator::engine::Engine<S>>(
                &self,
                component: &crate::ir::IRComponent,
                shape: &crate::ir::IRShape,
                store: &S,
                engine: &mut E,
                value_nodes: &crate::validator::nodes::ValueNodes<S>,
                source_shape: Option<&crate::ir::IRShape>,
                maybe_path: Option<&rudof_rdf::rdf_core::SHACLPath>,
                shapes_graph: &crate::ir::IRSchema,
            ) -> Result<Vec<crate::validator::report::ValidationResult>, crate::validator::error::ValidationError> {
                self.validate::<E>(
                    component,
                    shape,
                    store,
                    engine,
                    value_nodes,
                    source_shape,
                    maybe_path,
                    shapes_graph,
                )
            }
        }

        #[cfg(feature = "sparql")]
        impl<S> crate::validator::constraints::BasicSparqlValidator<S> for $ty
        where
            S: rudof_rdf::rdf_core::query::QueryRDF + rudof_rdf::rdf_core::NeighsRDF + std::fmt::Debug + 'static,
        {
            fn validate_sparql(
                &self,
                component: &crate::ir::IRComponent,
                shape: &crate::ir::IRShape,
                store: &S,
                value_nodes: &crate::validator::nodes::ValueNodes<S>,
                source_shape: Option<&crate::ir::IRShape>,
                maybe_path: Option<&rudof_rdf::rdf_core::SHACLPath>,
                shapes_graph: &crate::ir::IRSchema,
            ) -> Result<Vec<crate::validator::report::ValidationResult>, crate::validator::error::ValidationError> {
                self.validate::<crate::validator::engine::SparqlEngine>(
                    component,
                    shape,
                    store,
                    &mut crate::validator::engine::SparqlEngine::new(),
                    value_nodes,
                    source_shape,
                    maybe_path,
                    shapes_graph,
                )
            }
        }
    };
}

// Components whose native and sparql logic coincide.
impl_validators_via_validate!(MinCount);
impl_validators_via_validate!(MaxCount);
impl_validators_via_validate!(Or);
impl_validators_via_validate!(And);
impl_validators_via_validate!(Not);
impl_validators_via_validate!(Xone);
impl_validators_via_validate!(Deactivated);
impl_validators_via_validate!(Closed);
impl_validators_via_validate!(In);
impl_validators_via_validate!(HasValue);
impl_validators_via_validate!(Node);
impl_validators_via_validate!(QualifiedValueShape);
impl_validators_via_validate!(LanguageIn);
impl_validators_via_validate!(UniqueLang);
impl_validators_via_validate!(Datatype);

// ---------------------------------------------------------------------------
// Static dispatch — the `IRComponent` enum match *is* the dispatch. A single
// component list drives both the native and (cfg-gated) sparql dispatchers from
// one place, so they cannot desync. No `&dyn NativeValidator`, no `ValidatorDeref`.
// ---------------------------------------------------------------------------

macro_rules! gen_native_dispatch {
    ($($V:ident),+ $(,)?) => {
        /// Native constraint dispatch: monomorphises a concrete
        /// `validate_native::<E>` per component — no trait object is created.
        pub(crate) fn validate_native<S: NeighsRDF + Debug + 'static, E: Engine<S>>(
            component: &IRComponent,
            shape: &IRShape,
            store: &S,
            engine: &mut E,
            value_nodes: &ValueNodes<S>,
            source_shape: Option<&IRShape>,
            maybe_path: Option<&SHACLPath>,
            shapes_graph: &IRSchema,
        ) -> Result<Vec<ValidationResult>, ValidationError> {
            match component {
                $(
                    IRComponent::$V(c) => NativeValidator::validate_native::<E>(
                        c, component, shape, store, engine, value_nodes, source_shape, maybe_path, shapes_graph,
                    ),
                )+
            }
        }
    };
}

#[cfg(feature = "sparql")]
macro_rules! gen_sparql_dispatch {
    ($($V:ident),+ $(,)?) => {
        /// SPARQL constraint dispatch (mirror of [`validate_native`], same
        /// component list). SPARQL needs no recursion engine.
        pub(crate) fn validate_sparql<S: QueryRDF + NeighsRDF + Debug + 'static>(
            component: &IRComponent,
            shape: &IRShape,
            store: &S,
            value_nodes: &ValueNodes<S>,
            source_shape: Option<&IRShape>,
            maybe_path: Option<&SHACLPath>,
            shapes_graph: &IRSchema,
        ) -> Result<Vec<ValidationResult>, ValidationError> {
            match component {
                $(
                    IRComponent::$V(c) => BasicSparqlValidator::validate_sparql(
                        c, component, shape, store, value_nodes, source_shape, maybe_path, shapes_graph,
                    ),
                )+
            }
        }
    };
}

gen_native_dispatch!(
    Class, Datatype, NodeKind, MinCount, MaxCount, MinExclusive, MaxExclusive, MinInclusive, MaxInclusive, MinLength,
    MaxLength, Pattern, UniqueLang, LanguageIn, Equals, Disjoint, LessThan, LessThanOrEquals, Or, And, Not, Xone, Node,
    HasValue, In, QualifiedValueShape, Closed, Deactivated, BasicSparql,
);

#[cfg(feature = "sparql")]
gen_sparql_dispatch!(
    Class, Datatype, NodeKind, MinCount, MaxCount, MinExclusive, MaxExclusive, MinInclusive, MaxInclusive, MinLength,
    MaxLength, Pattern, UniqueLang, LanguageIn, Equals, Disjoint, LessThan, LessThanOrEquals, Or, And, Not, Xone, Node,
    HasValue, In, QualifiedValueShape, Closed, Deactivated, BasicSparql,
);

// ---------------------------------------------------------------------------
// Shared evaluation skeletons for the per-value / per-focus components.
// ---------------------------------------------------------------------------

fn apply<S: Rdf, I: IterationStrategy<S>>(
    component: &IRComponent,
    shape: &IRShape,
    value_nodes: &ValueNodes<S>,
    strategy: I,
    evaluator: impl Fn(&I::Item) -> Result<bool, ValidationError>,
    msg: &str,
    maybe_path: Option<&SHACLPath>,
) -> Result<Vec<ValidationResult>, ValidationError> {
    let mut results = Vec::new();
    for (focus_node, item) in strategy.iterate(value_nodes) {
        let Ok(focus) = S::term_as_object(focus_node) else {
            continue;
        };
        // Propagate evaluator errors as a typed ValidationError (no silent drop).
        if evaluator(item)? {
            let component = Object::iri(component.into());
            let value = strategy.to_object(item);
            let mut msg = MessageMap::from(msg);
            if let Some(m) = shape.message() {
                msg = msg.merge(m.to_owned(), true);
            }
            results.push(
                ValidationResult::new(focus, component, shape.severity().clone())
                    .with_source(Some(shape.id().clone()))
                    .with_message(msg)
                    .with_path(maybe_path.cloned())
                    .with_value(value),
            );
        }
    }
    Ok(results)
}

fn apply_with_focus<S: Rdf, I: IterationStrategy<S>>(
    component: &IRComponent,
    shape: &IRShape,
    value_nodes: &ValueNodes<S>,
    strategy: I,
    evaluator: impl Fn(&S::Term, &I::Item) -> Result<bool, ValidationError>,
    msg: &str,
    maybe_path: Option<&SHACLPath>,
) -> Result<Vec<ValidationResult>, ValidationError> {
    let mut results = Vec::new();
    for (focus_node, item) in strategy.iterate(value_nodes) {
        let Ok(focus) = S::term_as_object(focus_node) else {
            continue;
        };
        // Propagate evaluator errors instead of dropping them.
        if evaluator(focus_node, item)? {
            let component = Object::iri(component.into());
            let value = strategy.to_object(item);
            results.push(
                ValidationResult::new(focus, component, shape.severity().clone())
                    .with_source(Some(shape.id().clone()))
                    .with_message(MessageMap::from(msg))
                    .with_path(maybe_path.cloned())
                    .with_value(value),
            );
        }
    }
    Ok(results)
}

/// Validate with a boolean evaluator. If the evaluator returns true, it means there is a violation
fn validate_with<S: Rdf, I: IterationStrategy<S>>(
    component: &IRComponent,
    shape: &IRShape,
    value_nodes: &ValueNodes<S>,
    strategy: I,
    evaluator: impl Fn(&I::Item) -> bool,
    msg: &str,
    maybe_path: Option<&SHACLPath>,
) -> Result<Vec<ValidationResult>, ValidationError> {
    apply(
        component,
        shape,
        value_nodes,
        strategy,
        |item| Ok(evaluator(item)),
        msg,
        maybe_path,
    )
}

/// Validate with a boolean evaluator. If the evaluator returns true, it means that there is a violation
fn validate_with_focus<S: Rdf, I: IterationStrategy<S>>(
    component: &IRComponent,
    shape: &IRShape,
    value_nodes: &ValueNodes<S>,
    strategy: I,
    evaluator: impl Fn(&S::Term, &I::Item) -> bool,
    msg: &str,
    maybe_path: Option<&SHACLPath>,
) -> Result<Vec<ValidationResult>, ValidationError> {
    apply_with_focus(
        component,
        shape,
        value_nodes,
        strategy,
        |f, i| Ok(evaluator(f, i)),
        msg,
        maybe_path,
    )
}

#[cfg(feature = "sparql")]
fn validate_ask_with<S: QueryRDF>(
    component: &IRComponent,
    shape: &IRShape,
    store: &S,
    value_nodes: &ValueNodes<S>,
    eval_query: impl Fn(&S::Term) -> String,
    msg: &str,
    maybe_path: Option<&SHACLPath>,
) -> Result<Vec<ValidationResult>, ValidationError> {
    apply(
        component,
        shape,
        value_nodes,
        ValueNodeIteration,
        |vn| match store.query_ask(&eval_query(vn)) {
            Ok(ask) => Ok(!ask),
            Err(err) => Err(ValidationError::ask_query_error::<S>(err)),
        },
        msg,
        maybe_path,
    )
}
