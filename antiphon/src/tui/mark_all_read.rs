use std::collections::HashSet;

use antiphon_store::{MessageSummary, SearchIndex, StoreLayout};

use super::actions::{OpIntent, account_of};
use super::app::App;

const UNREAD_TAG: &str = "unread";

/// `,r`: queues a read op for every unread message the current
/// listing's query covers, not only the visible window, then
/// flips the in-memory rows that happen to be on screen.
pub(super) fn mark_all_read(app: &mut App, layout: &StoreLayout) {
    let query = match app.scoped(&app.current_query) {
        Ok(query) => query,
        Err(error) => {
            app.notice = Some(error.to_string());
            return;
        }
    };
    let unread_query = format!("tag:unread and ({query})");
    let index = match SearchIndex::open(layout) {
        Ok(index) => index,
        Err(error) => {
            app.notice = Some(error.to_string());
            return;
        }
    };
    let summaries = match index.query(&unread_query, None) {
        Ok(summaries) => summaries,
        Err(error) => {
            app.notice = Some(error.to_string());
            return;
        }
    };
    if summaries.is_empty() {
        app.notice = Some("nothing unread here".to_string());
        return;
    }
    app.pending_ops.extend(read_intents(&summaries));
    mark_rows_read(app, &summaries);
    app.notice = Some(format!("marked {} read", summaries.len()));
}

/// One flag op per unread summary, remove-only; pure so the
/// intent shape is testable without a live index.
fn read_intents(summaries: &[MessageSummary]) -> Vec<OpIntent> {
    summaries
        .iter()
        .map(|summary| OpIntent::Flag {
            account: account_of(&summary.path),
            message_id: summary.id.clone(),
            add: Vec::new(),
            remove: vec![UNREAD_TAG.to_string()],
        })
        .collect()
}

fn mark_rows_read(app: &mut App, summaries: &[MessageSummary]) {
    let ids: HashSet<&str> = summaries
        .iter()
        .map(|summary| summary.id.as_str())
        .collect();
    for message in &mut app.messages {
        if ids.contains(message.id.as_str()) {
            message.unread = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(id: &str, path: &str) -> MessageSummary {
        MessageSummary {
            id: id.to_string(),
            thread_id: String::new(),
            subject: String::new(),
            from: String::new(),
            to: String::new(),
            date_unix: 0,
            tags: vec![UNREAD_TAG.to_string()],
            unread: true,
            path: std::path::PathBuf::from(path),
        }
    }

    #[test]
    fn read_intents_carry_the_message_account_and_remove_unread() {
        let summaries = [
            summary("m1", "store/maildir/work/cur/1.eml"),
            summary("m2", "store/maildir/home/new/2.eml"),
        ];
        let intents = read_intents(&summaries);
        assert_eq!(intents.len(), 2);
        let OpIntent::Flag {
            account,
            message_id,
            add,
            remove,
        } = &intents[0]
        else {
            panic!("expected a flag intent");
        };
        assert_eq!(account, "work");
        assert_eq!(message_id, "m1");
        assert!(add.is_empty());
        assert_eq!(remove, &vec![UNREAD_TAG.to_string()]);
        let OpIntent::Flag { account, .. } = &intents[1] else {
            panic!("expected a flag intent");
        };
        assert_eq!(account, "home");
    }

    #[test]
    fn read_intents_of_no_summaries_is_empty() {
        assert!(read_intents(&[]).is_empty());
    }
}
