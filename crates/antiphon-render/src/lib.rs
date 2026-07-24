mod compose;
mod extract;
mod flowed;
mod patch;

pub use compose::{
    Draft, TemplateVars, build_message, expand_template,
};
pub use extract::{BodyKind, BodyText, body_text, delivered_addresses};
pub use flowed::flow;
pub use patch::{PatchLine, classify_patch, is_patch};
