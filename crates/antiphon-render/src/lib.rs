mod attach;
mod compose;
mod extract;
mod flowed;
mod headers;
mod html;
mod invite;
mod itip;
mod links;
mod list;
mod parts;
mod patch;
mod series;
mod unsubscribe;
mod urls;

pub use attach::{AttachmentPart, content_type_for};
pub use compose::{
    Draft, TemplateVars, build_message, expand_template,
};
pub use extract::{
    BodyKind, BodyPreference, BodyText, body_text,
    body_text_preferring, delivered_addresses, has_html_part,
    rendered_body, rendered_body_preferring,
};
pub use flowed::flow;
pub use headers::{MessageHeader, all_headers, selected_headers};
pub use invite::invite_lines;
pub use itip::{ItipReply, Rsvp, itip_reply};
pub use links::{BodyLine, Link, LinkSpan, RenderedBody, scan_text};
pub use list::{
    ListHeaders, ListPost, ListReply, list_headers, reply_to_list,
};
pub use parts::{MessageAttachment, attachments};
pub use patch::{PatchLine, classify_patch, is_patch};
pub use series::{SeriesMessage, mbox, patch_series};
pub use unsubscribe::{
    MailtoUnsubscribe, Unsubscribe, unsubscribe_method,
};
