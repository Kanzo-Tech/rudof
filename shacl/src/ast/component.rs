use crate::ast::ComponentVisitor;
use crate::types::{MessageMap, NodeKind, Value};
use itertools::Itertools;
use prefixmap::{IriRef, PrefixMap};
use rudof_iri::IriS;
use rudof_rdf::term::Object;
use rudof_rdf::term::literal::{ConcreteLiteral, Lang};
use rudof_rdf::vocab::ShaclVocab;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::convert::Infallible;
use std::fmt::{Display, Formatter};

// TODO - For node expr only derive Debug (maybe)
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub enum ASTComponent {
    Class(Object),
    Datatype(IriRef),
    NodeKind(NodeKind),
    MinCount(isize),
    MaxCount(isize),
    MinExclusive(ConcreteLiteral),
    MaxExclusive(ConcreteLiteral),
    MinInclusive(ConcreteLiteral),
    MaxInclusive(ConcreteLiteral),
    MinLength(isize),
    MaxLength(isize),
    Pattern {
        pattern: String,
        flags: Option<String>,
    },
    UniqueLang(bool),
    LanguageIn(Vec<Lang>),
    Equals(IriRef),
    Disjoint(IriRef),
    LessThan(IriRef),
    LessThanOrEquals(IriRef),
    Or(Vec<Object>),
    And(Vec<Object>),
    Not(Object),
    Xone(Vec<Object>),
    Closed {
        is_closed: bool,
        ignored_properties: HashSet<IriS>,
    },
    Node(Object),
    HasValue(Value),
    In(Vec<Value>),
    QualifiedValueShape {
        shape: Object,
        q_min_count: Option<isize>,
        q_max_count: Option<isize>,
        disjoint: Option<bool>,
        siblings: Vec<Object>,
    },
    Deactivated(bool), // TODO - Replace with node expr
    BasicSparql {
        select: String,
        message: Option<MessageMap>,
        deactivated: Option<bool>,
        prefixes: Option<PrefixMap>,
    },
}

/// Writes the canonical textual form of a component. The variant dispatch is
/// owned by [`ASTComponent::accept`]; this visitor only renders.
struct DisplayVisitor<'a, 'b> {
    f: &'a mut Formatter<'b>,
}

impl ComponentVisitor for DisplayVisitor<'_, '_> {
    type Output = ();
    type Error = std::fmt::Error;

    fn default_component(&mut self) -> Result<(), std::fmt::Error> {
        Ok(())
    }

    fn visit_class(&mut self, class: &Object) -> Result<(), std::fmt::Error> {
        write!(self.f, "class({class})")
    }
    fn visit_datatype(&mut self, iri: &IriRef) -> Result<(), std::fmt::Error> {
        write!(self.f, "datatype({iri})")
    }
    fn visit_node_kind(&mut self, node: &NodeKind) -> Result<(), std::fmt::Error> {
        write!(self.f, "nodeKind({node})")
    }
    fn visit_min_count(&mut self, qty: isize) -> Result<(), std::fmt::Error> {
        write!(self.f, "minCount({qty})")
    }
    fn visit_max_count(&mut self, qty: isize) -> Result<(), std::fmt::Error> {
        write!(self.f, "maxCount({qty})")
    }
    fn visit_min_exclusive(&mut self, lit: &ConcreteLiteral) -> Result<(), std::fmt::Error> {
        write!(self.f, "minExclusive({lit})")
    }
    fn visit_max_exclusive(&mut self, lit: &ConcreteLiteral) -> Result<(), std::fmt::Error> {
        write!(self.f, "maxExclusive({lit})")
    }
    fn visit_min_inclusive(&mut self, lit: &ConcreteLiteral) -> Result<(), std::fmt::Error> {
        write!(self.f, "minInclusive({lit})")
    }
    fn visit_max_inclusive(&mut self, lit: &ConcreteLiteral) -> Result<(), std::fmt::Error> {
        write!(self.f, "maxInclusive({lit})")
    }
    fn visit_min_length(&mut self, len: isize) -> Result<(), std::fmt::Error> {
        write!(self.f, "minLength({len})")
    }
    fn visit_max_length(&mut self, len: isize) -> Result<(), std::fmt::Error> {
        write!(self.f, "maxLength({len})")
    }
    fn visit_pattern(&mut self, pattern: &str, flags: Option<&str>) -> Result<(), std::fmt::Error> {
        match flags {
            None => write!(self.f, "pattern({pattern})"),
            Some(flags) => write!(self.f, "pattern({pattern}, {flags})"),
        }
    }
    fn visit_unique_lang(&mut self, unique: bool) -> Result<(), std::fmt::Error> {
        write!(self.f, "uniqueLang({unique})")
    }
    fn visit_language_in(&mut self, langs: &[Lang]) -> Result<(), std::fmt::Error> {
        let str = langs.iter().map(|s| s.to_string()).join(", ");
        write!(self.f, "languageIn[{str}]")
    }
    fn visit_equals(&mut self, iri: &IriRef) -> Result<(), std::fmt::Error> {
        write!(self.f, "equals({iri})")
    }
    fn visit_disjoint(&mut self, iri: &IriRef) -> Result<(), std::fmt::Error> {
        write!(self.f, "disjoint({iri})")
    }
    fn visit_less_than(&mut self, iri: &IriRef) -> Result<(), std::fmt::Error> {
        write!(self.f, "lessThan({iri})")
    }
    fn visit_less_than_or_equals(&mut self, iri: &IriRef) -> Result<(), std::fmt::Error> {
        write!(self.f, "lessThanOrEquals({iri})")
    }
    fn visit_or(&mut self, shapes: &[Object]) -> Result<(), std::fmt::Error> {
        let str = shapes.iter().map(|s| s.to_string()).join(", ");
        write!(self.f, "or[{str}]")
    }
    fn visit_and(&mut self, shapes: &[Object]) -> Result<(), std::fmt::Error> {
        let str = shapes.iter().map(|s| s.to_string()).join(", ");
        write!(self.f, "and[{str}]")
    }
    fn visit_not(&mut self, shape: &Object) -> Result<(), std::fmt::Error> {
        write!(self.f, "not({shape})")
    }
    fn visit_xone(&mut self, shapes: &[Object]) -> Result<(), std::fmt::Error> {
        let str = shapes.iter().map(|s| s.to_string()).join(", ");
        write!(self.f, "xone[{str}]")
    }
    fn visit_closed(&mut self, is_closed: bool, ignored: &HashSet<IriS>) -> Result<(), std::fmt::Error> {
        write!(
            self.f,
            "closed({is_closed}{})",
            if ignored.is_empty() {
                "".to_string()
            } else {
                format!(
                    ", Ignored props: [{}]",
                    ignored.iter().map(|p| p.to_string()).join(", ")
                )
            }
        )
    }
    fn visit_node(&mut self, shape: &Object) -> Result<(), std::fmt::Error> {
        write!(self.f, "node({shape})")
    }
    fn visit_has_value(&mut self, value: &Value) -> Result<(), std::fmt::Error> {
        write!(self.f, "hasValue({value})")
    }
    fn visit_in(&mut self, values: &[Value]) -> Result<(), std::fmt::Error> {
        let str = values.iter().map(|v| v.to_string()).join(", ");
        write!(self.f, "in[{str}]")
    }
    fn visit_qualified_value_shape(
        &mut self,
        shape: &Object,
        q_min_count: Option<isize>,
        q_max_count: Option<isize>,
        disjoint: Option<bool>,
        siblings: &[Object],
    ) -> Result<(), std::fmt::Error> {
        write!(
            self.f,
            "qualifiedValueShape(shape: {shape}, qualified_min_count: {q_min_count:?}, qualified_max_count: {q_max_count:?}, qualified_value_shape_disjoint: {disjoint:?}{}",
            if siblings.is_empty() {
                "".to_string()
            } else {
                format!(", siblings: [{}]", siblings.iter().map(|s| s.to_string()).join(", "))
            }
        )
    }
    fn visit_deactivated(&mut self, deactivated: bool) -> Result<(), std::fmt::Error> {
        write!(self.f, "deactivated({deactivated})")
    }
    fn visit_basic_sparql(
        &mut self,
        select: &str,
        message: Option<&MessageMap>,
        deactivated: Option<bool>,
        prefixes: Option<&PrefixMap>,
    ) -> Result<(), std::fmt::Error> {
        write!(
            self.f,
            "basic_sparql: (select: {select}{}{}{})",
            if let Some(deactivated) = deactivated {
                format!(", deactivated: {deactivated}")
            } else {
                "".to_string()
            },
            if let Some(msg) = message {
                format!(", message: {msg}")
            } else {
                "".to_string()
            },
            if let Some(prefixes) = prefixes {
                format!(", prefixes: {prefixes}")
            } else {
                "".to_string()
            }
        )
    }
}

impl Display for ASTComponent {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.accept(&mut DisplayVisitor { f })
    }
}

/// Maps a component to its defining SHACL constraint IRI. Total and infallible.
struct ConstraintIriVisitor;

impl ComponentVisitor for ConstraintIriVisitor {
    type Output = IriS;
    type Error = Infallible;

    // Every arm is overridden below, so this is never reached; return the base
    // SHACL namespace rather than panic.
    fn default_component(&mut self) -> Result<IriS, Infallible> {
        Ok(ShaclVocab::sh())
    }

    fn visit_class(&mut self, _: &Object) -> Result<IriS, Infallible> {
        Ok(ShaclVocab::sh_class())
    }
    fn visit_datatype(&mut self, _: &IriRef) -> Result<IriS, Infallible> {
        Ok(ShaclVocab::sh_datatype())
    }
    fn visit_node_kind(&mut self, _: &NodeKind) -> Result<IriS, Infallible> {
        Ok(ShaclVocab::sh_node_kind())
    }
    fn visit_min_count(&mut self, _: isize) -> Result<IriS, Infallible> {
        Ok(ShaclVocab::sh_min_count())
    }
    fn visit_max_count(&mut self, _: isize) -> Result<IriS, Infallible> {
        Ok(ShaclVocab::sh_max_count())
    }
    fn visit_min_exclusive(&mut self, _: &ConcreteLiteral) -> Result<IriS, Infallible> {
        Ok(ShaclVocab::sh_min_exclusive())
    }
    fn visit_max_exclusive(&mut self, _: &ConcreteLiteral) -> Result<IriS, Infallible> {
        Ok(ShaclVocab::sh_max_exclusive())
    }
    fn visit_min_inclusive(&mut self, _: &ConcreteLiteral) -> Result<IriS, Infallible> {
        Ok(ShaclVocab::sh_min_inclusive())
    }
    fn visit_max_inclusive(&mut self, _: &ConcreteLiteral) -> Result<IriS, Infallible> {
        Ok(ShaclVocab::sh_max_inclusive())
    }
    fn visit_min_length(&mut self, _: isize) -> Result<IriS, Infallible> {
        Ok(ShaclVocab::sh_min_length())
    }
    fn visit_max_length(&mut self, _: isize) -> Result<IriS, Infallible> {
        Ok(ShaclVocab::sh_max_length())
    }
    fn visit_pattern(&mut self, _: &str, _: Option<&str>) -> Result<IriS, Infallible> {
        Ok(ShaclVocab::sh_pattern())
    }
    fn visit_unique_lang(&mut self, _: bool) -> Result<IriS, Infallible> {
        Ok(ShaclVocab::sh_unique_lang())
    }
    fn visit_language_in(&mut self, _: &[Lang]) -> Result<IriS, Infallible> {
        Ok(ShaclVocab::sh_language_in())
    }
    fn visit_equals(&mut self, _: &IriRef) -> Result<IriS, Infallible> {
        Ok(ShaclVocab::sh_equals())
    }
    fn visit_disjoint(&mut self, _: &IriRef) -> Result<IriS, Infallible> {
        Ok(ShaclVocab::sh_disjoint())
    }
    fn visit_less_than(&mut self, _: &IriRef) -> Result<IriS, Infallible> {
        Ok(ShaclVocab::sh_less_than())
    }
    fn visit_less_than_or_equals(&mut self, _: &IriRef) -> Result<IriS, Infallible> {
        Ok(ShaclVocab::sh_less_than_or_equals())
    }
    fn visit_or(&mut self, _: &[Object]) -> Result<IriS, Infallible> {
        Ok(ShaclVocab::sh_or())
    }
    fn visit_and(&mut self, _: &[Object]) -> Result<IriS, Infallible> {
        Ok(ShaclVocab::sh_and())
    }
    fn visit_not(&mut self, _: &Object) -> Result<IriS, Infallible> {
        Ok(ShaclVocab::sh_not())
    }
    fn visit_xone(&mut self, _: &[Object]) -> Result<IriS, Infallible> {
        Ok(ShaclVocab::sh_xone())
    }
    fn visit_closed(&mut self, _: bool, _: &HashSet<IriS>) -> Result<IriS, Infallible> {
        Ok(ShaclVocab::sh_closed())
    }
    fn visit_node(&mut self, _: &Object) -> Result<IriS, Infallible> {
        Ok(ShaclVocab::sh_node())
    }
    fn visit_has_value(&mut self, _: &Value) -> Result<IriS, Infallible> {
        Ok(ShaclVocab::sh_has_value())
    }
    fn visit_in(&mut self, _: &[Value]) -> Result<IriS, Infallible> {
        Ok(ShaclVocab::sh_in())
    }
    fn visit_qualified_value_shape(
        &mut self,
        _: &Object,
        _: Option<isize>,
        _: Option<isize>,
        _: Option<bool>,
        _: &[Object],
    ) -> Result<IriS, Infallible> {
        Ok(ShaclVocab::sh_qualified_value_shape())
    }
    fn visit_deactivated(&mut self, _: bool) -> Result<IriS, Infallible> {
        Ok(ShaclVocab::sh_deactivated())
    }
    fn visit_basic_sparql(
        &mut self,
        _: &str,
        _: Option<&MessageMap>,
        _: Option<bool>,
        _: Option<&PrefixMap>,
    ) -> Result<IriS, Infallible> {
        Ok(ShaclVocab::sh_sparql())
    }
}

impl From<&ASTComponent> for IriS {
    fn from(value: &ASTComponent) -> Self {
        match value.accept(&mut ConstraintIriVisitor) {
            Ok(iri) => iri,
            Err(infallible) => match infallible {},
        }
    }
}

impl From<ASTComponent> for IriS {
    fn from(value: ASTComponent) -> Self {
        IriS::from(&value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_and_iri_via_visitor() {
        let c = ASTComponent::MinCount(2);
        assert_eq!(c.to_string(), "minCount(2)");
        assert_eq!(IriS::from(&c), ShaclVocab::sh_min_count());

        let p = ASTComponent::Pattern {
            pattern: "^a".to_string(),
            flags: Some("i".to_string()),
        };
        assert_eq!(p.to_string(), "pattern(^a, i)");
        assert_eq!(IriS::from(&p), ShaclVocab::sh_pattern());
    }
}
