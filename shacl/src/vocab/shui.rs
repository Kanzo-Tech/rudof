//! SHACL-UI (`shui:`) vocabulary constants.
//!
//! Models the standard SHACL 1.2 User Interfaces vocabulary
//! (<https://www.w3.org/TR/shacl12-ui/>, namespace
//! `http://www.w3.org/ns/shacl-ui#`). rudof only *recognises* these terms; the
//! interpretation (editor IRI → widget) stays in downstream consumers. The
//! well-known editor classes are exposed as `&str` constants so a parser can
//! match them without committing to any rendering semantics.
//!
//! Style mirrors `rudof_rdf`'s vocab modules (a `&str` constant plus a cached
//! `IriS` accessor) but stays dependency-free (no `const_format`/`paste`).

use rudof_iri::IriS;
use std::sync::OnceLock;

/// Base namespace IRI for the SHACL-UI vocabulary.
pub const SHUI: &str = "http://www.w3.org/ns/shacl-ui#";

/// `shui:editor` — assigns an editor class to a property shape (≈ `dash:editor`).
pub const SHUI_EDITOR: &str = "http://www.w3.org/ns/shacl-ui#editor";
/// `shui:viewer` — assigns a viewer class to a property shape (≈ `dash:viewer`).
pub const SHUI_VIEWER: &str = "http://www.w3.org/ns/shacl-ui#viewer";

macro_rules! shui_iri {
    ($name:ident, $iri:expr) => {
        /// Cached [`IriS`] for the corresponding SHACL-UI term.
        pub fn $name() -> &'static IriS {
            static IRI: OnceLock<IriS> = OnceLock::new();
            IRI.get_or_init(|| IriS::new_unchecked($iri))
        }
    };
}

shui_iri!(shui_editor, SHUI_EDITOR);
shui_iri!(shui_viewer, SHUI_VIEWER);

/// Well-known SHACL-UI editor class IRIs (constants only — recognised, not
/// interpreted). Kept in sync with the downstream `Editors` table.
pub mod editors {
    pub const TEXT_FIELD: &str = "http://www.w3.org/ns/shacl-ui#TextFieldEditor";
    pub const TEXT_AREA: &str = "http://www.w3.org/ns/shacl-ui#TextAreaEditor";
    pub const TEXT_FIELD_WITH_LANG: &str = "http://www.w3.org/ns/shacl-ui#TextFieldWithLangEditor";
    pub const TEXT_AREA_WITH_LANG: &str = "http://www.w3.org/ns/shacl-ui#TextAreaWithLangEditor";
    pub const NUMBER_FIELD: &str = "http://www.w3.org/ns/shacl-ui#NumberFieldEditor";
    pub const DATE_PICKER: &str = "http://www.w3.org/ns/shacl-ui#DatePickerEditor";
    pub const DATE_TIME_PICKER: &str = "http://www.w3.org/ns/shacl-ui#DateTimePickerEditor";
    pub const BOOLEAN: &str = "http://www.w3.org/ns/shacl-ui#BooleanEditor";
    pub const ENUM_SELECT: &str = "http://www.w3.org/ns/shacl-ui#EnumSelectEditor";
    pub const IRI: &str = "http://www.w3.org/ns/shacl-ui#IRIEditor";
    pub const AUTO_COMPLETE: &str = "http://www.w3.org/ns/shacl-ui#AutoCompleteEditor";
    pub const INSTANCES_SELECT: &str = "http://www.w3.org/ns/shacl-ui#InstancesSelectEditor";
    pub const SUB_CLASS: &str = "http://www.w3.org/ns/shacl-ui#SubClassEditor";
    pub const DETAILS: &str = "http://www.w3.org/ns/shacl-ui#DetailsEditor";
    pub const RICH_TEXT: &str = "http://www.w3.org/ns/shacl-ui#RichTextEditor";
    pub const BLANK_NODE: &str = "http://www.w3.org/ns/shacl-ui#BlankNodeEditor";
}
