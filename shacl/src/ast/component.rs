use crate::types::{MessageMap, NodeKind, Value};
use itertools::Itertools;
use prefixmap::{IriRef, PrefixMap};
use rudof_iri::IriS;
use rudof_rdf::SHACLPath;
use rudof_rdf::term::Object;
use rudof_rdf::term::literal::{ConcreteLiteral, Lang};
use rudof_rdf::vocab::ShaclVocab;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fmt::{Display, Formatter};

// TODO - For node expr only derive Debug (maybe)
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub enum ASTComponent {
    /// `sh:class`: every value node is a SHACL instance of one of these — a
    /// single IRI, or the members of the SHACL list SHACL 1.2 Core allows.
    Class(Vec<IriRef>),
    /// `sh:datatype`: the datatype of every value node must be one of these — a single
    /// IRI, or the members of the SHACL list SHACL 1.2 Core allows.
    Datatype(Vec<IriRef>),
    /// `sh:nodeKind`: the kind of every value node is one of these — a single
    /// value, or the members of the SHACL list SHACL 1.2 Core allows.
    NodeKind(Vec<NodeKind>),
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
    SingleLine(bool),
    UniqueLang(bool),
    LanguageIn(Vec<Lang>),
    MemberShape(Object),
    MinListLength(isize),
    MaxListLength(isize),
    UniqueMembers(bool),
    /// The property pair components compare the value nodes with the nodes a
    /// SHACL property path reaches (SHACL 1.2 Core; a predicate in 1.0).
    Equals(SHACLPath),
    Disjoint(SHACLPath),
    SubsetOf(SHACLPath),
    LessThan(SHACLPath),
    LessThanOrEquals(SHACLPath),
    Or(Vec<Object>),
    And(Vec<Object>),
    Not(Object),
    Xone(Vec<Object>),
    If {
        cond: Object,
        then_: Option<Object>,
        else_: Option<Object>,
    },
    /// `sh:closed`. `by_types` is `Some` for `sh:closed sh:ByTypes` (SHACL 1.2):
    /// the properties each class of the shapes graph permits, by
    /// `collectProperties` (§8.4.1).
    Closed {
        is_closed: bool,
        ignored_properties: HashSet<IriS>,
        by_types: Option<Vec<(IriS, Vec<IriS>)>>,
    },
    Node(Object),
    /// `sh:nodeByExpression` with an IRI expression, the one node expression
    /// SHACL Core defines for it: the shape the IRI names.
    NodeByExpression(Object),
    SomeValue(Object),
    HasValue(Value),
    In(Vec<Value>),
    RootClass(Vec<IriRef>),
    UniqueValuesFor(Vec<IriRef>),
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

impl ASTComponent {
    /// The parameters whose triples state the component in the shapes graph,
    /// its main one first: what a reifier annotates (SHACL 1.2 §2.1.3).
    pub fn parameters(&self) -> Vec<IriS> {
        let main = match self {
            ASTComponent::Class(_) => ShaclVocab::sh_class(),
            ASTComponent::Datatype(_) => ShaclVocab::sh_datatype(),
            ASTComponent::NodeKind(_) => ShaclVocab::sh_node_kind(),
            ASTComponent::MinCount(_) => ShaclVocab::sh_min_count(),
            ASTComponent::MaxCount(_) => ShaclVocab::sh_max_count(),
            ASTComponent::MinExclusive(_) => ShaclVocab::sh_min_exclusive(),
            ASTComponent::MaxExclusive(_) => ShaclVocab::sh_max_exclusive(),
            ASTComponent::MinInclusive(_) => ShaclVocab::sh_min_inclusive(),
            ASTComponent::MaxInclusive(_) => ShaclVocab::sh_max_inclusive(),
            ASTComponent::MinLength(_) => ShaclVocab::sh_min_length(),
            ASTComponent::MaxLength(_) => ShaclVocab::sh_max_length(),
            ASTComponent::Pattern { .. } => return vec![ShaclVocab::sh_pattern(), ShaclVocab::sh_flags()],
            ASTComponent::SingleLine(_) => ShaclVocab::sh_single_line(),
            ASTComponent::UniqueLang(_) => ShaclVocab::sh_unique_lang(),
            ASTComponent::LanguageIn(_) => ShaclVocab::sh_language_in(),
            ASTComponent::MemberShape(_) => ShaclVocab::sh_member_shape(),
            ASTComponent::MinListLength(_) => ShaclVocab::sh_min_list_length(),
            ASTComponent::MaxListLength(_) => ShaclVocab::sh_max_list_length(),
            ASTComponent::UniqueMembers(_) => ShaclVocab::sh_unique_members(),
            ASTComponent::Equals(_) => ShaclVocab::sh_equals(),
            ASTComponent::Disjoint(_) => ShaclVocab::sh_disjoint(),
            ASTComponent::SubsetOf(_) => ShaclVocab::sh_subset_of(),
            ASTComponent::LessThan(_) => ShaclVocab::sh_less_than(),
            ASTComponent::LessThanOrEquals(_) => ShaclVocab::sh_less_than_or_equals(),
            ASTComponent::Or(_) => ShaclVocab::sh_or(),
            ASTComponent::And(_) => ShaclVocab::sh_and(),
            ASTComponent::Not(_) => ShaclVocab::sh_not(),
            ASTComponent::Xone(_) => ShaclVocab::sh_xone(),
            ASTComponent::If { .. } => return vec![ShaclVocab::sh_if(), ShaclVocab::sh_then(), ShaclVocab::sh_else()],
            ASTComponent::Closed { .. } => return vec![ShaclVocab::sh_closed(), ShaclVocab::sh_ignored_properties()],
            ASTComponent::Node(_) => ShaclVocab::sh_node(),
            ASTComponent::NodeByExpression(_) => ShaclVocab::sh_node_by_expression(),
            ASTComponent::SomeValue(_) => ShaclVocab::sh_some_value(),
            ASTComponent::HasValue(_) => ShaclVocab::sh_has_value(),
            ASTComponent::In(_) => ShaclVocab::sh_in(),
            ASTComponent::RootClass(_) => ShaclVocab::sh_root_class(),
            ASTComponent::UniqueValuesFor(_) => ShaclVocab::sh_unique_values_for(),
            ASTComponent::QualifiedValueShape { .. } => {
                return vec![
                    ShaclVocab::sh_qualified_value_shape(),
                    ShaclVocab::sh_qualified_min_count(),
                    ShaclVocab::sh_qualified_max_count(),
                    ShaclVocab::sh_qualified_value_shapes_disjoint(),
                ];
            },
            ASTComponent::Deactivated(_) => ShaclVocab::sh_deactivated(),
            ASTComponent::BasicSparql { .. } => ShaclVocab::sh_sparql(),
        };
        vec![main]
    }
}

/// `name(value)`, the name the component's main parameter's.
impl Display for ASTComponent {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        fn all<T: ToString>(items: &[T]) -> String {
            items.iter().map(ToString::to_string).join(", ")
        }
        let value = match self {
            ASTComponent::Class(iris)
            | ASTComponent::Datatype(iris)
            | ASTComponent::RootClass(iris)
            | ASTComponent::UniqueValuesFor(iris) => all(iris),
            ASTComponent::NodeKind(kinds) => all(kinds),
            ASTComponent::MinCount(n)
            | ASTComponent::MaxCount(n)
            | ASTComponent::MinLength(n)
            | ASTComponent::MaxLength(n)
            | ASTComponent::MinListLength(n)
            | ASTComponent::MaxListLength(n) => n.to_string(),
            ASTComponent::MinExclusive(l)
            | ASTComponent::MaxExclusive(l)
            | ASTComponent::MinInclusive(l)
            | ASTComponent::MaxInclusive(l) => l.to_string(),
            ASTComponent::Pattern { pattern, flags } => match flags {
                None => pattern.clone(),
                Some(flags) => format!("{pattern}, {flags}"),
            },
            ASTComponent::SingleLine(b)
            | ASTComponent::UniqueLang(b)
            | ASTComponent::UniqueMembers(b)
            | ASTComponent::Deactivated(b) => b.to_string(),
            ASTComponent::LanguageIn(langs) => all(langs),
            ASTComponent::Equals(p)
            | ASTComponent::Disjoint(p)
            | ASTComponent::SubsetOf(p)
            | ASTComponent::LessThan(p)
            | ASTComponent::LessThanOrEquals(p) => p.to_string(),
            ASTComponent::Or(shapes) | ASTComponent::And(shapes) | ASTComponent::Xone(shapes) => all(shapes),
            ASTComponent::Not(shape)
            | ASTComponent::Node(shape)
            | ASTComponent::NodeByExpression(shape)
            | ASTComponent::SomeValue(shape)
            | ASTComponent::MemberShape(shape) => shape.to_string(),
            ASTComponent::If { cond, then_, else_ } => {
                let branch = |o: &Option<Object>| o.as_ref().map_or("-".to_owned(), ToString::to_string);
                format!("{cond}, then: {}, else: {}", branch(then_), branch(else_))
            },
            ASTComponent::Closed {
                is_closed,
                ignored_properties,
                by_types,
            } => {
                let closed = if by_types.is_some() {
                    "ByTypes".to_owned()
                } else {
                    is_closed.to_string()
                };
                let mut ignored: Vec<&IriS> = ignored_properties.iter().collect();
                ignored.sort_by_key(|i| i.as_str());
                format!("{closed}, ignored: [{}]", all(&ignored))
            },
            ASTComponent::HasValue(v) => v.to_string(),
            ASTComponent::In(values) => all(values),
            ASTComponent::QualifiedValueShape {
                shape,
                q_min_count,
                q_max_count,
                disjoint,
                ..
            } => format!("{shape}, min: {q_min_count:?}, max: {q_max_count:?}, disjoint: {disjoint:?}"),
            ASTComponent::BasicSparql { select, .. } => select.clone(),
        };
        let name = self.parameters()[0]
            .as_str()
            .trim_start_matches(ShaclVocab::SH)
            .to_owned();
        write!(f, "{name}({value})")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_and_parameters() {
        let c = ASTComponent::MinCount(2);
        assert_eq!(c.to_string(), "minCount(2)");
        assert_eq!(c.parameters(), vec![ShaclVocab::sh_min_count()]);

        let p = ASTComponent::Pattern {
            pattern: "^a".to_string(),
            flags: Some("i".to_string()),
        };
        assert_eq!(p.to_string(), "pattern(^a, i)");
        assert_eq!(p.parameters()[0], ShaclVocab::sh_pattern());
    }
}
