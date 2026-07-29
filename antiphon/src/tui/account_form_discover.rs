//! Autodiscovery for the account form: `^d` looks the entered
//! e-mail address up through Thunderbird-style autoconfig and
//! fills the IMAP and SMTP rows. The network call goes through the
//! [`Fetcher`] seam, so `run` wires the live HTTP fetcher while
//! tests drive `run_with` from a canned table.

use antiphon_autodiscover::{
    Discovered, Fetcher, HttpFetcher, discover,
};

use super::account_form::AccountFormState;
use super::app::App;

pub(super) fn run(app: &mut App) {
    run_with(app, &HttpFetcher::new());
}

fn run_with<F: Fetcher>(app: &mut App, fetcher: &F) {
    let Some(form) = app.account_form.as_mut() else {
        return;
    };
    if form.provider().is_some() {
        form.error = Some(
            "autodiscovery fills IMAP accounts; this type has \
             fixed servers"
                .to_string(),
        );
        return;
    }
    let address = form.address.trim().to_string();
    if !address.contains('@') {
        form.error =
            Some("enter the e-mail address first, then ^d".to_string());
        return;
    }
    settle(form, &address, discover(&address, fetcher));
}

type Lookup =
    Result<Option<Discovered>, antiphon_autodiscover::DiscoverError>;

fn settle(form: &mut AccountFormState, address: &str, lookup: Lookup) {
    match lookup {
        Ok(Some(found)) => {
            apply(form, &found);
            form.error = None;
        }
        Ok(None) => {
            let domain =
                address.rsplit_once('@').map_or(address, |at| at.1);
            form.error = Some(format!(
                "no settings found for {domain}; fill them in"
            ));
        }
        Err(_) => {
            form.error =
                Some("enter a full e-mail address first".to_string());
        }
    }
}

/// The form holds only hosts and the IMAP user, so the discovered
/// ports and socket types are not written; the daemon supplies the
/// standard ports.
fn apply(form: &mut AccountFormState, found: &Discovered) {
    if let Some(imap) = &found.imap {
        form.imap_host = imap.host.clone();
        form.imap_user = imap.username.clone();
    }
    if let Some(smtp) = &found.smtp {
        form.smtp_host = smtp.host.clone();
    }
}

#[cfg(test)]
#[path = "account_form_discover_tests.rs"]
mod tests;
