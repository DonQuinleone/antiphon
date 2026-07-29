//! Thread-tree construction over the loaded results: a
//! thread: query reorders messages into reply pre-order and
//! restores folds, every other query stays a flat list.

use antiphon_store::MessageSummary;

use super::app::App;
use super::thread_tree::{self, Reply, ThreadTree};

const THREAD_QUERY_PREFIX: &str = "thread:";

impl App {
    /// The Message-IDs of the currently folded nodes, so a
    /// refresh that rebuilds the tree can restore the folds
    /// the reader had closed.
    pub(super) fn collapsed_ids(&self) -> Vec<String> {
        let Some(tree) = &self.thread_tree else {
            return Vec::new();
        };
        tree.nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| node.collapsed)
            .filter_map(|(position, _)| {
                self.messages.get(position).map(|m| m.id.clone())
            })
            .collect()
    }

    /// A thread pivot reorders the loaded messages into reply
    /// pre-order and builds the tree over them; any other query
    /// leaves a flat list.
    pub(super) fn build_thread_tree(
        &mut self,
        folded: Vec<String>,
    ) -> Option<ThreadTree> {
        if !self.current_query.starts_with(THREAD_QUERY_PREFIX) {
            return None;
        }
        let (order, mut tree) = {
            let items: Vec<Reply> =
                self.messages.iter().map(reply_of).collect();
            thread_tree::build(&items)
        };
        if tree.is_empty() {
            return None;
        }
        self.messages =
            order.iter().map(|i| self.messages[*i].clone()).collect();
        for (position, message) in self.messages.iter().enumerate() {
            if folded.contains(&message.id) {
                tree.set_collapsed(position, true);
            }
        }
        Some(tree)
    }
}

fn reply_of(message: &MessageSummary) -> Reply<'_> {
    Reply {
        id: &message.id,
        in_reply_to: message.in_reply_to.as_deref(),
        references: message
            .references
            .iter()
            .map(String::as_str)
            .collect(),
        date_unix: message.date_unix,
    }
}
