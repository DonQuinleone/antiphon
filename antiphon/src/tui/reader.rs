use antiphon_pgp::Signature;
use antiphon_render::{MessageHeader, RenderedBody};
use antiphon_store::MessageSummary;

use super::app::{App, View};

impl App {
    pub fn open_pager(
        &mut self,
        body: String,
        signature: Signature,
        invite: Vec<String>,
    ) {
        let rendered = antiphon_render::scan_text(&body);
        self.open_with(body, rendered, signature, invite);
    }

    /// A message read through decrypt::read_message keeps the
    /// link spans its rendering produced (html labels
    /// included); open_pager rescans plain text instead.
    pub fn open_message(&mut self, opened: super::decrypt::Opened) {
        self.open_with(
            opened.body,
            opened.rendered,
            opened.signature,
            opened.invite,
        );
    }

    fn open_with(
        &mut self,
        body: String,
        rendered: RenderedBody,
        signature: Signature,
        invite: Vec<String>,
    ) {
        self.set_unread(false);
        self.pager_patch = patch_lines(self.selected_message(), &body);
        self.pager_body = body;
        self.pager_rendered = rendered;
        self.pager_signature = signature;
        self.pager_invite = invite;
        self.pager_scroll = 0;
        self.link_picker = None;
        self.pager_attachments =
            antiphon_render::attachments(&self.pager_raw);
        self.pager_images = antiphon_render::images(&self.pager_raw);
        self.image_view = None;
        self.drawer_open = false;
        self.drawer_selected = 0;
        self.pager_headers = antiphon_render::selected_headers(
            &self.pager_raw,
            &self.header_names,
        );
        self.pager_headers_all =
            antiphon_render::all_headers(&self.pager_raw);
        self.view = View::Pager;
    }

    /// The header block the pager shows right now: the
    /// configured set, or everything once t toggles it.
    pub fn pager_header_view(&self) -> &[MessageHeader] {
        if self.headers_all {
            &self.pager_headers_all
        } else {
            &self.pager_headers
        }
    }
}

fn patch_lines(
    selected: Option<&MessageSummary>,
    body: &str,
) -> Vec<antiphon_render::PatchLine> {
    let subject = selected
        .map(|message| message.subject.as_str())
        .unwrap_or_default();
    if !antiphon_render::is_patch(subject, body) {
        return Vec::new();
    }
    antiphon_render::classify_patch(body)
}
