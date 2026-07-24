use antiphon_store::{Scope, ScopeError, scoped_query};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ViewScope {
    Unified,
    Account(String),
}

impl ViewScope {
    pub fn label(&self) -> &str {
        match self {
            ViewScope::Unified => "unified",
            ViewScope::Account(account) => account,
        }
    }
}

/// The one builder every client query goes through: whatever
/// the user asked for is conjoined with the accounts the
/// current scope allows, so no view can ever query unscoped.
pub fn effective_query(
    scope: &ViewScope,
    accounts: &[String],
    user_query: &str,
) -> Result<String, ScopeError> {
    let scope = match scope {
        ViewScope::Unified => Scope::all(accounts),
        ViewScope::Account(account) => Scope::one(account),
    };
    scoped_query(&scope, user_query)
}

pub fn next_scope(scope: &ViewScope, accounts: &[String]) -> ViewScope {
    match position(scope, accounts) {
        None => account_at(accounts, 0),
        Some(index) if index + 1 < accounts.len() => {
            account_at(accounts, index + 1)
        }
        Some(_) => ViewScope::Unified,
    }
}

pub fn previous_scope(
    scope: &ViewScope,
    accounts: &[String],
) -> ViewScope {
    match position(scope, accounts) {
        None if accounts.is_empty() => ViewScope::Unified,
        None => account_at(accounts, accounts.len() - 1),
        Some(0) => ViewScope::Unified,
        Some(index) => account_at(accounts, index - 1),
    }
}

fn position(scope: &ViewScope, accounts: &[String]) -> Option<usize> {
    let ViewScope::Account(current) = scope else {
        return None;
    };
    accounts.iter().position(|account| account == current)
}

fn account_at(accounts: &[String], index: usize) -> ViewScope {
    match accounts.get(index) {
        Some(account) => ViewScope::Account(account.clone()),
        None => ViewScope::Unified,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|name| (*name).to_string()).collect()
    }

    #[test]
    fn scopes_cycle_forward_through_accounts_and_back() {
        let accounts = names(&["a", "b"]);
        let mut scope = ViewScope::Unified;
        let expected = [
            ViewScope::Account("a".into()),
            ViewScope::Account("b".into()),
            ViewScope::Unified,
        ];
        for want in expected {
            scope = next_scope(&scope, &accounts);
            assert_eq!(scope, want);
        }
    }

    #[test]
    fn scopes_cycle_backward_from_unified_to_the_last() {
        let accounts = names(&["a", "b"]);
        let scope = previous_scope(&ViewScope::Unified, &accounts);
        assert_eq!(scope, ViewScope::Account("b".into()));
        let scope = previous_scope(&scope, &accounts);
        assert_eq!(scope, ViewScope::Account("a".into()));
        let scope = previous_scope(&scope, &accounts);
        assert_eq!(scope, ViewScope::Unified);
    }

    #[test]
    fn cycling_with_no_accounts_stays_unified() {
        let accounts = names(&[]);
        let scope = next_scope(&ViewScope::Unified, &accounts);
        assert_eq!(scope, ViewScope::Unified);
        let scope = previous_scope(&ViewScope::Unified, &accounts);
        assert_eq!(scope, ViewScope::Unified);
    }

    #[test]
    fn a_vanished_account_scope_recovers_via_unified() {
        let accounts = names(&["a"]);
        let gone = ViewScope::Account("gone".into());
        assert_eq!(
            next_scope(&gone, &accounts),
            ViewScope::Account("a".into()),
        );
    }

    #[test]
    fn unified_queries_conjoin_every_configured_account() {
        let accounts = names(&["a", "b"]);
        let query = effective_query(
            &ViewScope::Unified,
            &accounts,
            "tag:unread",
        )
        .unwrap();
        assert!(query.contains("path:\"a/**\""));
        assert!(query.contains("path:\"b/**\""));
        assert!(query.ends_with("and (tag:unread)"));
    }

    #[test]
    fn a_message_in_a_hidden_account_can_never_match() {
        let accounts = names(&["visible", "hidden"]);
        let query = effective_query(
            &ViewScope::Account("visible".into()),
            &accounts,
            "*",
        )
        .unwrap();
        assert_eq!(query, "(path:\"visible/**\")");
        assert!(!query.contains("hidden"));
    }

    #[test]
    fn no_accounts_means_no_query_at_all() {
        let scoped =
            effective_query(&ViewScope::Unified, &[], "tag:unread");
        assert!(scoped.is_err());
    }

    #[test]
    fn labels_name_the_scope_for_the_statusline() {
        assert_eq!(ViewScope::Unified.label(), "unified");
        assert_eq!(ViewScope::Account("work".into()).label(), "work",);
    }
}
