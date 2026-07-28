use super::super::app::DEFAULT_QUERY;
use super::super::testkit::{
    app_with_accounts, app_with_folders, app_with_messages,
};
use super::*;

#[test]
fn t_pivots_to_the_thread_and_back_restores() {
    let mut app = app_with_messages(2);
    app.messages[0].thread_id = "th7".to_string();
    app.current_query = "tag:inbox".to_string();
    app.active_search = Some("inbox".to_string());

    app.apply_in_list(Action::ThreadView);
    assert_eq!(app.current_query, "thread:th7");
    assert_eq!(app.active_search.as_deref(), Some("thread"));
    assert!(app.take_requery());

    app.apply_in_list(Action::Back);
    assert_eq!(app.current_query, "tag:inbox");
    assert_eq!(app.active_search.as_deref(), Some("inbox"));
    assert!(app.take_requery());
    app.apply_in_list(Action::Back);
    assert!(!app.take_requery(), "back is idle with no thread");
}

#[test]
fn a_threadless_message_never_pivots() {
    let mut app = app_with_messages(1);
    app.apply_in_list(Action::ThreadView);
    assert!(app.thread_return.is_none());
    assert!(!app.take_requery());
}

fn summary(
    id: &str,
    parent: Option<&str>,
    date: i64,
) -> antiphon_store::MessageSummary {
    antiphon_store::MessageSummary {
        id: id.to_string(),
        thread_id: "t1".to_string(),
        subject: id.to_string(),
        from: String::new(),
        to: String::new(),
        date_unix: date,
        tags: Vec::new(),
        unread: false,
        path: std::path::PathBuf::new(),
        in_reply_to: parent.map(str::to_string),
        references: parent.into_iter().map(str::to_string).collect(),
    }
}

fn threaded_app() -> App {
    let mut app = app_with_messages(0);
    let messages = vec![
        summary("root", None, 0),
        summary("b", Some("root"), 20),
        summary("a", Some("root"), 10),
        summary("a1", Some("a"), 15),
    ];
    app.set_results(messages, 4, "thread:t1".to_string());
    app
}

#[test]
fn entering_a_thread_reorders_into_reply_preorder() {
    let app = threaded_app();
    assert!(app.thread_tree.is_some());
    let ids: Vec<&str> =
        app.messages.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, ["root", "a", "a1", "b"]);
}

#[test]
fn folding_a_subtree_skips_it_when_navigating() {
    let mut app = threaded_app();
    app.selected = 1;
    app.apply(Action::FoldClose);
    app.apply(Action::MoveDown);
    assert_eq!(app.selected, 3, "the folded a1 is skipped");
    app.apply(Action::Bottom);
    assert_eq!(app.selected, 3);
    app.apply(Action::Top);
    app.apply(Action::MoveDown);
    assert_eq!(app.selected, 1, "root then a, a1 still hidden");
}

#[test]
fn folding_a_leaf_or_a_flat_list_reports_it() {
    let mut app = threaded_app();
    app.selected = 3;
    app.apply(Action::FoldToggle);
    assert_eq!(app.notice.as_deref(), Some("no replies to fold here"));

    let mut flat = app_with_messages(2);
    flat.apply(Action::FoldToggle);
    assert_eq!(flat.notice.as_deref(), Some("open a thread first"));
}

#[test]
fn leaving_a_thread_drops_the_tree() {
    let mut app = threaded_app();
    app.thread_return =
        Some(("tag:inbox".to_string(), Some("inbox".to_string())));
    app.apply(Action::Back);
    assert!(app.thread_tree.is_none());
    assert_eq!(app.current_query, "tag:inbox");
    assert!(app.take_requery());
}

#[test]
fn movement_clamps_at_both_ends() {
    let mut app = app_with_messages(3);
    app.apply(Action::MoveUp);
    assert_eq!(app.selected, 0);
    app.apply(Action::Bottom);
    assert_eq!(app.selected, 2);
    app.apply(Action::MoveDown);
    assert_eq!(app.selected, 2);
    app.apply(Action::HalfPageUp);
    assert_eq!(app.selected, 0);
}

#[test]
fn half_page_moves_by_the_constant() {
    let mut app = app_with_messages(30);
    app.apply(Action::HalfPageDown);
    assert_eq!(app.selected, HALF_PAGE_ROWS);
}

#[test]
fn reading_pane_cycles_through_all_three() {
    let mut app = app_with_messages(1);
    app.apply(Action::CycleReadingPane);
    assert_eq!(app.reading_pane, ReadingPane::Right);
    app.apply(Action::CycleReadingPane);
    assert_eq!(app.reading_pane, ReadingPane::Off);
    app.apply(Action::CycleReadingPane);
    assert_eq!(app.reading_pane, ReadingPane::Below);
}

#[test]
fn unhandled_actions_leave_a_notice_and_quit_quits() {
    let mut app = app_with_messages(1);
    app.apply(Action::OpenLink);
    assert!(app.notice.is_some());
    app.apply(Action::Quit);
    assert!(app.quit);
}

#[test]
fn gt_cycles_unified_through_accounts_and_back() {
    let mut app = app_with_accounts(&["a", "b"]);
    app.apply(Action::NextAccount);
    assert_eq!(app.scope, ViewScope::Account("a".into()));
    assert!(app.take_requery());
    app.apply(Action::NextAccount);
    assert_eq!(app.scope, ViewScope::Account("b".into()));
    app.apply(Action::NextAccount);
    assert_eq!(app.scope, ViewScope::Unified);
    app.apply(Action::PreviousAccount);
    assert_eq!(app.scope, ViewScope::Account("b".into()));
    assert!(app.take_requery());
    assert!(!app.take_requery());
}

#[test]
fn sidebar_moves_in_entry_order_without_querying() {
    let mut app = app_with_accounts(&["a"]);
    app.apply(Action::SidebarNext);
    app.apply(Action::SidebarNext);
    assert_eq!(app.sidebar_selected, 2);
    assert!(!app.take_requery());
    app.apply(Action::SidebarPrevious);
    assert_eq!(app.sidebar_selected, 1);
}

#[test]
fn opening_an_account_entry_sets_the_scope() {
    let mut app = app_with_accounts(&["a", "b"]);
    let position = app
        .sidebar_entries
        .iter()
        .position(|entry| entry == &SidebarEntry::Account("b".into()))
        .expect("account b entry");
    app.sidebar_selected = position;
    app.apply(Action::SidebarOpen);
    assert_eq!(app.scope, ViewScope::Account("b".into()));
    assert!(app.take_requery());
    assert_eq!(app.current_query, DEFAULT_QUERY);
    assert!(app.active_search.is_none());
}

#[test]
fn opening_a_folder_scopes_its_account_and_queries_it() {
    let mut app =
        app_with_folders(&[("a", &[][..]), ("b", &["archive"][..])]);
    let cases = [
        ("b", "archive", "path:\"b/archive/**\""),
        ("a", "inbox", "path:\"a/cur\" or path:\"a/new\""),
    ];
    for (account, folder, query) in cases {
        let position = app
            .sidebar_entries
            .iter()
            .position(|entry| match entry {
                SidebarEntry::Folder {
                    account: entry_account,
                    name,
                    ..
                } => entry_account == account && name == folder,
                _ => false,
            })
            .expect("folder entry");
        app.sidebar_selected = position;
        app.apply(Action::SidebarOpen);
        assert_eq!(
            app.scope,
            ViewScope::Account(account.into()),
            "{folder}"
        );
        assert_eq!(app.current_query, query, "{folder}");
        assert_eq!(
            app.active_search.as_deref(),
            Some(folder),
            "{folder}"
        );
        assert!(app.take_requery(), "{folder}");
        let scoped = app.scoped(&app.current_query).unwrap();
        assert!(scoped.contains(query), "{folder}: {scoped}");
    }
}

#[test]
fn opening_a_saved_search_keeps_scope_and_names_it() {
    let mut app = app_with_accounts(&["a"]);
    app.scope = ViewScope::Account("a".into());
    let unread = app
        .sidebar_entries
        .iter()
        .position(|entry| entry.label() == "unread")
        .expect("built-in unread entry");
    app.sidebar_selected = unread;
    app.apply(Action::SidebarOpen);
    assert_eq!(app.current_query, "tag:unread");
    assert_eq!(app.active_search.as_deref(), Some("unread"));
    assert_eq!(app.scope, ViewScope::Account("a".into()));
    assert!(app.take_requery());
}

#[test]
fn empty_list_never_panics() {
    let mut app = app_with_messages(0);
    app.apply(Action::Bottom);
    app.apply(Action::MoveDown);
    assert_eq!(app.selected, 0);
    assert!(app.selected_message().is_none());
}
