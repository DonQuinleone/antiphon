mod compose;
mod extract;
mod flowed;
mod html;
mod invite;
mod links;
mod list;
mod patch;
mod series;
mod unsubscribe;
mod urls;

pub use compose::{
    Draft, TemplateVars, build_message, expand_template,
};
pub use extract::{
    BodyKind, BodyText, body_text, delivered_addresses, rendered_body,
};
pub use flowed::flow;
pub use invite::invite_lines;
pub use links::{BodyLine, Link, LinkSpan, RenderedBody};
pub use list::{
    ListHeaders, ListPost, ListReply, list_headers, reply_to_list,
};
pub use patch::{PatchLine, classify_patch, is_patch};
pub use series::{SeriesMessage, mbox, patch_series};
pub use unsubscribe::{
    MailtoUnsubscribe, Unsubscribe, unsubscribe_method,
};
