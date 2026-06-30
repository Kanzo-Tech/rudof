use rudof_iri::IriS;
use serde::{Deserialize, Serialize};

/// SHACL-UI (`shui:`) presentation hints carried verbatim on a shape.
///
/// rudof models the *standard vocabulary* only: it stores the declared
/// `shui:editor` / `shui:viewer` IRIs without interpreting them. Downstream
/// consumers (e.g. metadata-form) map those IRIs to concrete widgets. See
/// [`crate::vocab::shui`] for the term constants. Namespace:
/// <https://www.w3.org/TR/shacl12-ui/> (`http://www.w3.org/ns/shacl-ui#`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Presentation {
    /// `shui:editor` — the suggested editor class IRI.
    editor: Option<IriS>,
    /// `shui:viewer` — the suggested viewer class IRI.
    viewer: Option<IriS>,
}

impl Presentation {
    /// True when no presentation hint was declared (the common case).
    pub fn is_empty(&self) -> bool {
        self.editor.is_none() && self.viewer.is_none()
    }

    pub fn editor(&self) -> Option<&IriS> {
        self.editor.as_ref()
    }

    pub fn viewer(&self) -> Option<&IriS> {
        self.viewer.as_ref()
    }

    pub fn with_editor(mut self, editor: Option<IriS>) -> Self {
        self.editor = editor;
        self
    }

    pub fn with_viewer(mut self, viewer: Option<IriS>) -> Self {
        self.viewer = viewer;
        self
    }
}
