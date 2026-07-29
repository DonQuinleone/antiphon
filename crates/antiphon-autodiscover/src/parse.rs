//! Reading the `clientConfig` autoconfig document. Only the IMAP
//! incoming server and the SMTP outgoing server are taken; POP3
//! and other blocks are ignored, since the account form speaks
//! IMAP only.

use roxmltree::{Document, Node};

use crate::{Discovered, Security, ServerSettings};

const IMAP_SSL_PORT: u16 = 993;
const IMAP_PLAIN_PORT: u16 = 143;
const SMTP_SSL_PORT: u16 = 465;
const SMTP_SUBMISSION_PORT: u16 = 587;

const EMAIL_PLACEHOLDER: &str = "%EMAILADDRESS%";
const LOCALPART_PLACEHOLDER: &str = "%EMAILLOCALPART%";

pub(crate) fn config(body: &str, email: &str) -> Discovered {
    let Ok(doc) = Document::parse(body) else {
        return Discovered::default();
    };
    let root = doc.root_element();
    Discovered {
        provider: provider_name(root),
        imap: server(root, "incomingServer", "imap", email, imap_port),
        smtp: server(root, "outgoingServer", "smtp", email, smtp_port),
    }
}

fn provider_name(root: Node) -> Option<String> {
    let provider = child_by_tag(root, "emailProvider")?;
    let display = child_text(provider, "displayName")
        .or_else(|| child_text(provider, "displayShortName"))?;
    let trimmed = display.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn server(
    root: Node,
    tag: &str,
    kind: &str,
    email: &str,
    default_port: fn(Security) -> u16,
) -> Option<ServerSettings> {
    let node = servers(root, tag)
        .find(|node| node.attribute("type") == Some(kind))?;
    let host = child_text(node, "hostname")?.trim().to_string();
    if host.is_empty() {
        return None;
    }
    let security = socket_type(node);
    Some(ServerSettings {
        host,
        port: port(node).unwrap_or_else(|| default_port(security)),
        security,
        username: username(node, email),
    })
}

fn servers<'a>(
    root: Node<'a, 'a>,
    tag: &'a str,
) -> impl Iterator<Item = Node<'a, 'a>> {
    root.descendants()
        .filter(move |node| node.has_tag_name(tag))
}

fn socket_type(node: Node) -> Security {
    match child_text(node, "socketType")
        .map(|value| value.trim().to_ascii_lowercase())
        .as_deref()
    {
        Some("ssl") => Security::Ssl,
        Some("starttls") => Security::Starttls,
        _ => Security::Plain,
    }
}

fn port(node: Node) -> Option<u16> {
    child_text(node, "port")?.trim().parse().ok()
}

fn imap_port(security: Security) -> u16 {
    match security {
        Security::Ssl => IMAP_SSL_PORT,
        Security::Starttls | Security::Plain => IMAP_PLAIN_PORT,
    }
}

fn smtp_port(security: Security) -> u16 {
    match security {
        Security::Ssl => SMTP_SSL_PORT,
        Security::Starttls | Security::Plain => SMTP_SUBMISSION_PORT,
    }
}

fn username(node: Node, email: &str) -> String {
    let raw = child_text(node, "username").unwrap_or(email);
    let local = email.rsplit_once('@').map_or(email, |split| split.0);
    raw.replace(EMAIL_PLACEHOLDER, email)
        .replace(LOCALPART_PLACEHOLDER, local)
}

fn child_by_tag<'a>(
    node: Node<'a, 'a>,
    tag: &str,
) -> Option<Node<'a, 'a>> {
    node.children().find(|child| child.has_tag_name(tag))
}

fn child_text<'a>(node: Node<'a, 'a>, tag: &str) -> Option<&'a str> {
    child_by_tag(node, tag)?.text()
}
