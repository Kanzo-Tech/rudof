use itertools::Itertools;
use rudof_rdf::term::literal::{ConcreteLiteral, Lang};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::collections::hash_map::IntoIter;
use std::fmt::{Display, Formatter};

// `HashMap<Option<Lang>, String>` does not serialize to JSON (non-string map
// key). Round-trip through an order-independent `Vec<LangString>` instead so the
// public API keeps the map while the wire form is a plain list.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "Vec<LangString>", into = "Vec<LangString>")]
pub struct MessageMap {
    messages: HashMap<Option<Lang>, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LangString {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    lang: Option<Lang>,
    value: String,
}

impl From<Vec<LangString>> for MessageMap {
    fn from(items: Vec<LangString>) -> Self {
        MessageMap {
            messages: items.into_iter().map(|ls| (ls.lang, ls.value)).collect(),
        }
    }
}

impl From<MessageMap> for Vec<LangString> {
    fn from(map: MessageMap) -> Self {
        map.messages
            .into_iter()
            .map(|(lang, value)| LangString { lang, value })
            .collect()
    }
}

impl MessageMap {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_message(mut self, lang: Option<Lang>, message: String) -> Self {
        self.messages.insert(lang, message);
        self
    }

    pub fn messages(&self) -> &HashMap<Option<Lang>, String> {
        &self.messages
    }

    pub fn iter(&self) -> impl Iterator<Item = (&Option<Lang>, &String)> {
        self.messages.iter()
    }

    pub fn iter_literals(&self) -> impl Iterator<Item = ConcreteLiteral> {
        self.messages.iter().map(|(lang, msg)| ConcreteLiteral::StringLiteral {
            lang: lang.clone(),
            lexical_form: msg.clone(),
        })
    }

    pub fn get(&self, lang: Option<&Lang>) -> Option<&String> {
        self.messages.get(&lang.cloned())
    }

    pub fn merge(mut self, other: Self, over: bool) -> Self {
        other.into_iter().for_each(|(lang, msg)| {
            if over || !self.messages.contains_key(&lang) {
                self.messages.insert(lang, msg);
            }
        });
        self
    }
}

impl IntoIterator for MessageMap {
    type Item = (Option<Lang>, String);
    type IntoIter = IntoIter<Option<Lang>, String>;

    fn into_iter(self) -> Self::IntoIter {
        self.messages.into_iter()
    }
}

impl From<&str> for MessageMap {
    fn from(value: &str) -> Self {
        Self {
            messages: HashMap::from([(None, value.to_string())]),
        }
    }
}

impl From<String> for MessageMap {
    fn from(value: String) -> Self {
        Self {
            messages: HashMap::from([(None, value)]),
        }
    }
}

impl Display for MessageMap {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "MessageMap {{")?;

        let data = self
            .iter()
            .map(|(l, msg)| match l {
                None => format!("default: {:?}", msg),
                Some(l) => format!("{:?}: {:?}", l, msg),
            })
            .join(", ");

        write!(f, "{data}}}")
    }
}
