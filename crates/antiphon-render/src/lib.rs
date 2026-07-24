mod compose;
mod extract;
mod flowed;

pub use compose::{
    Draft, TemplateVars, build_message, expand_template,
};
pub use extract::{BodyKind, BodyText, body_text, delivered_addresses};
pub use flowed::flow;
