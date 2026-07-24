use std::fmt;

const MATCH_ALL: &str = "*";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scope {
    accounts: Vec<String>,
}

impl Scope {
    pub fn all(accounts: &[String]) -> Self {
        Self {
            accounts: accounts.to_vec(),
        }
    }

    pub fn permits(&self, account: &str) -> bool {
        self.accounts.iter().any(|name| name == account)
    }

    pub fn one(account: &str) -> Self {
        Self {
            accounts: vec![account.to_owned()],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScopeError {
    NoAccounts,
    QuoteInAccount { account: String },
    UnbalancedQuery { query: String },
}

impl fmt::Display for ScopeError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoAccounts => {
                write!(out, "scope covers no accounts")
            }
            Self::QuoteInAccount { account } => {
                write!(out, "account name {account:?} contains a quote")
            }
            Self::UnbalancedQuery { query } => write!(
                out,
                "query `{query}` has unbalanced quotes or \
                 parentheses"
            ),
        }
    }
}

impl std::error::Error for ScopeError {}

/// The one query builder every view goes through. A blank or
/// `*` user query becomes the scope clause alone: notmuch only
/// treats `*` as match-all when it is the whole query, and
/// `(scope) and (*)` silently matches nothing (notmuch 0.40).
pub fn scoped_query(
    scope: &Scope,
    user_query: &str,
) -> Result<String, ScopeError> {
    let clause = scope_clause(scope)?;
    let trimmed = user_query.trim();
    if trimmed.is_empty() || trimmed == MATCH_ALL {
        return Ok(clause);
    }
    ensure_balanced(trimmed)?;
    Ok(format!("{clause} and ({trimmed})"))
}

fn scope_clause(scope: &Scope) -> Result<String, ScopeError> {
    if scope.accounts.is_empty() {
        return Err(ScopeError::NoAccounts);
    }
    let terms = scope
        .accounts
        .iter()
        .map(|account| path_term(account))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(format!("({})", terms.join(" or ")))
}

fn path_term(account: &str) -> Result<String, ScopeError> {
    if account.contains('"') {
        return Err(ScopeError::QuoteInAccount {
            account: account.to_owned(),
        });
    }
    Ok(format!("path:\"{account}/**\""))
}

/// The user query is embedded as `and (<query>)`; a stray
/// closing parenthesis would end that group early and splice an
/// `or` past the scope conjunction (verified against notmuch
/// 0.40), so unbalanced parentheses or quotes are rejected.
fn ensure_balanced(query: &str) -> Result<(), ScopeError> {
    let unbalanced = || ScopeError::UnbalancedQuery {
        query: query.to_owned(),
    };
    let mut depth: usize = 0;
    let mut in_phrase = false;
    for ch in query.chars() {
        match ch {
            '"' => in_phrase = !in_phrase,
            '(' if !in_phrase => depth += 1,
            ')' if !in_phrase => {
                depth = depth.checked_sub(1).ok_or_else(unbalanced)?;
            }
            _ => {}
        }
    }
    if depth != 0 || in_phrase {
        return Err(unbalanced());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn accounts(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn single_account_scopes_the_query() {
        let scope = Scope::one("work");
        assert_eq!(
            scoped_query(&scope, "tag:unread").unwrap(),
            "(path:\"work/**\") and (tag:unread)"
        );
    }

    #[test]
    fn several_accounts_join_with_or() {
        let scope = Scope::all(&accounts(&["work", "home"]));
        assert_eq!(
            scoped_query(&scope, "tag:flagged").unwrap(),
            "(path:\"work/**\" or path:\"home/**\") \
             and (tag:flagged)"
        );
    }

    #[test]
    fn blank_and_star_mean_everything_in_scope() {
        let scope = Scope::one("work");
        for query in ["", "   ", "*", " * ", "\t\n"] {
            assert_eq!(
                scoped_query(&scope, query).unwrap(),
                "(path:\"work/**\")",
                "for user query {query:?}"
            );
        }
    }

    #[test]
    fn user_or_stays_inside_the_scope_group() {
        let scope = Scope::one("work");
        assert_eq!(
            scoped_query(&scope, "tag:a or tag:b").unwrap(),
            "(path:\"work/**\") and (tag:a or tag:b)"
        );
    }

    #[test]
    fn quote_in_account_name_is_rejected() {
        let scope = Scope::one("wo\"rk");
        assert_eq!(
            scoped_query(&scope, "tag:unread"),
            Err(ScopeError::QuoteInAccount {
                account: "wo\"rk".to_owned()
            })
        );
    }

    #[test]
    fn empty_scope_is_rejected() {
        let scope = Scope::all(&[]);
        assert_eq!(
            scoped_query(&scope, "tag:unread"),
            Err(ScopeError::NoAccounts)
        );
    }

    #[test]
    fn paren_breakout_is_rejected() {
        let scope = Scope::one("visible");
        let breakout = "tag:unread) or (path:hidden/**";
        assert_eq!(
            scoped_query(&scope, breakout),
            Err(ScopeError::UnbalancedQuery {
                query: breakout.to_owned()
            })
        );
    }

    #[test]
    fn unterminated_phrase_is_rejected() {
        let scope = Scope::one("work");
        let open = "subject:\"unterminated";
        assert_eq!(
            scoped_query(&scope, open),
            Err(ScopeError::UnbalancedQuery {
                query: open.to_owned()
            })
        );
    }

    #[test]
    fn parens_inside_phrases_are_literal() {
        let scope = Scope::one("work");
        assert_eq!(
            scoped_query(&scope, "subject:\"(draft\"").unwrap(),
            "(path:\"work/**\") and (subject:\"(draft\")"
        );
    }
}
