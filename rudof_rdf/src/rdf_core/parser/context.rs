//! Parser-owned focus cursor and parsing context.
//!
//! The RDF graph is *data* (queried through `&self`); the parse *focus* is traversal
//! state and belongs to the traverser, not to the graph. This module externalizes the
//! focus out of the RDF layer: [`RdfFocus`] is a small cursor over a graph term, and
//! [`ParseCtx`] bundles a borrowed graph (`&R`) with a mutably-borrowed focus cursor.
//!
//! This removes any need for a graph-internal focus (the deleted `FocusRDF` trait) and,
//! with it, the `Arc<Graph>` clone-to-fork machinery: backtracking combinators save and
//! restore a single small term via the cursor, never the graph.

use crate::rdf_core::{
    NeighsRDF, RDFError, SHACLPath,
    parser::rdf_node_parser::{RDFNodeParse, constructors::ShaclPathParser},
};
use prefixmap::PrefixMap;

/// A parser-owned cursor tracking the current focus node plus a save/restore stack
/// used by backtracking combinators (`Or`, `optional`, `then`).
///
/// Only a single [`NeighsRDF::Term`](crate::rdf_core::Rdf::Term) is ever cloned on
/// save/restore — never the graph.
pub struct RdfFocus<R: NeighsRDF> {
    /// The current focus node, if any.
    current: Option<R::Term>,
    /// Save/restore stack for backtracking combinators.
    saved: Vec<Option<R::Term>>,
}

impl<R: NeighsRDF> Default for RdfFocus<R> {
    fn default() -> Self {
        Self::empty()
    }
}

impl<R: NeighsRDF> RdfFocus<R> {
    /// Creates a cursor positioned at `start`.
    pub fn new(start: R::Term) -> Self {
        Self {
            current: Some(start),
            saved: Vec::new(),
        }
    }

    /// Creates a cursor with no current focus.
    pub fn empty() -> Self {
        Self {
            current: None,
            saved: Vec::new(),
        }
    }

    /// Returns the current focus node, if set.
    pub fn get(&self) -> Option<&R::Term> {
        self.current.as_ref()
    }

    /// Sets the current focus node.
    pub fn set(&mut self, term: R::Term) {
        self.current = Some(term);
    }

    /// Pushes the current focus onto the save stack (for backtracking).
    pub fn save(&mut self) {
        self.saved.push(self.current.clone());
    }

    /// Restores the most recently saved focus, rewinding a backtracking attempt.
    pub fn restore(&mut self) {
        self.current = self.saved.pop().flatten();
    }

    /// Commits the most recently saved focus, discarding it without rewinding.
    pub fn commit(&mut self) {
        self.saved.pop();
    }
}

/// Execution context for a parse: a borrowed, immutable graph plus a mutable focus cursor.
///
/// The graph is shared (`&'a R`) and never mutated by parsing; only the focus cursor moves.
pub struct ParseCtx<'a, R: NeighsRDF> {
    graph: &'a R,
    focus: &'a mut RdfFocus<R>,
}

impl<'a, R: NeighsRDF> ParseCtx<'a, R> {
    /// Creates a parsing context over a borrowed graph and focus cursor.
    pub fn new(graph: &'a R, focus: &'a mut RdfFocus<R>) -> Self {
        Self { graph, focus }
    }

    /// Returns the borrowed graph.
    pub fn graph(&self) -> &'a R {
        self.graph
    }

    /// Returns a shared reference to the focus cursor.
    pub fn focus(&self) -> &RdfFocus<R> {
        self.focus
    }

    /// Returns a mutable reference to the focus cursor.
    pub fn focus_mut(&mut self) -> &mut RdfFocus<R> {
        self.focus
    }

    /// Returns the prefix map of the underlying graph.
    pub fn prefixmap(&self) -> Option<PrefixMap> {
        self.graph.prefixmap()
    }

    // ---- `FocusRDF`-compatible surface (keeps parser bodies unchanged) ----

    /// Returns the current focus node, if set.
    pub fn get_focus(&self) -> Option<&R::Term> {
        self.focus.get()
    }

    /// Sets the current focus node.
    pub fn set_focus(&mut self, focus: &R::Term) {
        self.focus.set(focus.clone());
    }

    /// Returns the current focus node, failing if none is set.
    pub fn get_focus_as_term(&self) -> Result<&R::Term, RDFError> {
        self.focus.get().ok_or(RDFError::NoFocusNodeError)
    }

    /// Returns the current focus as a subject, failing if unset or not a subject.
    pub fn get_focus_as_subject(&self) -> Result<R::Subject, RDFError> {
        match self.focus.get() {
            None => Err(RDFError::NoFocusNodeError),
            Some(term) => R::term_as_subject(term).map_err(|_| RDFError::ExpectedSubjectError {
                node: format!("{term}"),
                context: "get_focus_as_subject".to_string(),
            }),
        }
    }

    /// Parses a SHACL path from a subject-predicate pair.
    pub fn get_path_for(&mut self, subject: &R::Term, predicate: &R::IRI) -> Result<Option<SHACLPath>, RDFError> {
        match self.graph.objects_for(subject, predicate)?.into_iter().next() {
            Some(term) => {
                let path = ShaclPathParser::new(term.clone()).parse_focused(self).map_err(|e| {
                    RDFError::InvalidSHACLPathError {
                        node: format!("{term}"),
                        error: Box::new(e),
                    }
                })?;
                Ok(Some(path))
            },
            None => Ok(None),
        }
    }
}
