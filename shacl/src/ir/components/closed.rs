use rudof_iri::IriS;
use std::fmt::{Display, Formatter};

/// `sh:closed` (SHACL 1.2 Core §8.4): every triple of a value node has a
/// permitted predicate. With `sh:closed true` the permitted ones are the
/// shape's `sh:property/sh:path` and the ignored properties; with
/// `sh:closed sh:ByTypes` they are, per value node, `rdf:type`, the ignored
/// properties and those `collectProperties` gives each of its types.
#[derive(Debug, Clone)]
pub struct Closed {
    is_closed: bool,
    ignored_properties: Vec<IriS>,
    by_types: Option<Vec<(IriS, Vec<IriS>)>>,
}

impl Closed {
    pub fn new(is_closed: bool, ignored_properties: Vec<IriS>, by_types: Option<Vec<(IriS, Vec<IriS>)>>) -> Self {
        Closed {
            is_closed,
            ignored_properties,
            by_types,
        }
    }

    pub fn is_closed(&self) -> bool {
        self.is_closed
    }

    pub fn ignored_properties(&self) -> &Vec<IriS> {
        &self.ignored_properties
    }

    /// For `sh:closed sh:ByTypes`, the properties each class permits.
    pub fn by_types(&self) -> Option<&Vec<(IriS, Vec<IriS>)>> {
        self.by_types.as_ref()
    }
}

impl Display for Closed {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let closed = if self.by_types.is_some() {
            "ByTypes".to_owned()
        } else {
            self.is_closed.to_string()
        };
        write!(
            f,
            "Closed: {closed}, ignored_properties: [{}]",
            self.ignored_properties()
                .iter()
                .map(|p| p.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}
