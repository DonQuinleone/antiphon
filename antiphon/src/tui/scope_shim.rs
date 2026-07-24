//! TEMPORARY: local mirror of the scope API landing in
//! antiphon-store on the wt/scope branch. Delete this module
//! at merge and import `Scope`, `ScopeError` and
//! `scoped_query` from antiphon_store instead; the signatures
//! here match that branch exactly.

use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scope {
    accounts: Vec<String>,
}

impl Scope {
    pub fn all(accounts: &[String]) -> Scope {
        Scope {
            accounts: accounts.to_vec(),
        }
    }

    pub fn one(account: &str) -> Scope {
        Scope {
            accounts: vec![account.to_string()],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScopeError {
    NoAccounts,
}

impl fmt::Display for ScopeError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoAccounts => out.write_str(
                "no accounts in scope; configure accounts/*.toml",
            ),
        }
    }
}

impl std::error::Error for ScopeError {}

pub fn scoped_query(
    scope: &Scope,
    user_query: &str,
) -> Result<String, ScopeError> {
    if scope.accounts.is_empty() {
        return Err(ScopeError::NoAccounts);
    }
    let clause = scope
        .accounts
        .iter()
        .map(|account| format!("path:\"{account}/**\""))
        .collect::<Vec<_>>()
        .join(" or ");
    let trimmed = user_query.trim();
    if trimmed.is_empty() || trimmed == "*" {
        return Ok(format!("({clause})"));
    }
    Ok(format!("({clause}) and ({trimmed})"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|name| (*name).to_string()).collect()
    }

    #[test]
    fn one_account_conjoins_its_path_with_the_query() {
        let scope = Scope::one("work");
        assert_eq!(
            scoped_query(&scope, "tag:unread").unwrap(),
            "(path:\"work/**\") and (tag:unread)",
        );
    }

    #[test]
    fn all_accounts_disjoin_paths_before_the_query() {
        let scope = Scope::all(&names(&["a", "b"]));
        assert_eq!(
            scoped_query(&scope, "tag:flagged").unwrap(),
            "(path:\"a/**\" or path:\"b/**\") and (tag:flagged)",
        );
    }

    #[test]
    fn match_all_collapses_to_the_bare_scope_clause() {
        let scope = Scope::one("work");
        assert_eq!(
            scoped_query(&scope, "*").unwrap(),
            "(path:\"work/**\")",
        );
        assert_eq!(
            scoped_query(&scope, "  ").unwrap(),
            "(path:\"work/**\")",
        );
    }

    #[test]
    fn an_empty_scope_can_never_produce_a_query() {
        let scope = Scope::all(&[]);
        assert_eq!(
            scoped_query(&scope, "*"),
            Err(ScopeError::NoAccounts),
        );
    }
}
