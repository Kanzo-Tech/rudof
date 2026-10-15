//! A mapping as an RDF graph: the mapping graph of RML-Core §3.2.
//!
//! [`parse`] reads a [`Mapping`](crate::Mapping) from any
//! [`NeighsRDF`](rudof_rdf::NeighsRDF) graph and [`write`] builds one into any
//! [`BuildRDF`](rudof_rdf::BuildRDF) graph, as `shacl`'s `ShaclParser` and
//! `ShaclWriter` do for shapes. [`parse_turtle`] and [`write_turtle`] are the
//! two over rudof's in-memory graph, the Turtle syntax being the one a
//! conforming processor SHOULD accept (§3.3).
//!
//! `parse(write(m)) == m.canonical()` for every valid mapping, and
//! `write(parse(g))` is `g` for a graph that uses the constant shortcuts of
//! §8.1.1 and carries no documentation triples beyond the mapping's
//! annotations.

mod parse;
mod write;

pub use parse::{parse, parse_turtle};
pub use write::{write, write_turtle};
