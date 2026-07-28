use super::super::testkit::{app_with_accounts, app_with_messages};
use super::*;

#[test]
fn keys_route_to_the_editor_view_before_the_keymap() {
    let mut app = app_with_messages(1);
    assert_eq!(app.key_route(), KeyRoute::Keymap);
    app.view = View::Editor;
    assert_eq!(app.key_route(), KeyRoute::Editor);
    app.view = View::List;
    app.apply(Action::Search);
    assert_eq!(app.key_route(), KeyRoute::Prompt);
}

#[test]
fn compose_stages_route_and_abort_back_to_the_list() {
    let mut app = app_with_messages(1);
    app.start_compose(super::super::compose::test_state());
    assert_eq!(app.view, View::Compose);
    assert_eq!(app.key_route(), KeyRoute::Compose);
    app.view = View::Review;
    assert_eq!(app.key_route(), KeyRoute::Review);
    app.apply(Action::Quit);
    assert_eq!(app.view, View::Review, "actions swallowed");
    app.abort_compose("compose aborted");
    assert_eq!(app.view, View::List);
    assert!(app.compose.is_none());
    assert_eq!(app.notice.as_deref(), Some("compose aborted"));
}

#[test]
fn editor_view_swallows_actions_unchanged() {
    let mut app = app_with_messages(1);
    app.view = View::Editor;
    app.apply(Action::Quit);
    assert!(!app.quit);
    assert_eq!(app.view, View::Editor);
}

#[test]
fn pager_scrolls_clamped_and_returns_to_the_list() {
    let mut app = app_with_messages(1);
    app.open_pager(
        "one\ntwo\nthree\n".to_string(),
        Signature::none(),
        Vec::new(),
    );
    assert_eq!(app.view, View::Pager);
    app.apply(Action::MoveUp);
    assert_eq!(app.pager_scroll, 0);
    app.apply(Action::HalfPageDown);
    assert_eq!(app.pager_scroll, 3);
    app.apply(Action::Top);
    assert_eq!(app.pager_scroll, 0);
    app.apply(Action::Quit);
    assert_eq!(app.view, View::List);
    assert!(!app.quit);
}

#[test]
fn pager_classification_follows_patch_detection() {
    use antiphon_render::PatchLine;

    let diff = "--- a/f\n+++ b/f\n@@ -1 +1 @@\n-a\n+b\n";
    let mut app = app_with_messages(1);
    app.open_pager(
        "plain words\n".into(),
        Signature::none(),
        Vec::new(),
    );
    assert!(app.pager_patch.is_empty());
    app.open_pager(diff.into(), Signature::none(), Vec::new());
    assert_eq!(app.pager_patch[3], PatchLine::Removal);
    assert_eq!(app.pager_patch[4], PatchLine::Addition);
    app.messages[0].subject = "[PATCH] prose only".into();
    app.open_pager(
        "no diff here\n".into(),
        Signature::none(),
        Vec::new(),
    );
    assert_eq!(app.pager_patch, [PatchLine::Text]);
}

#[test]
fn opening_the_pager_marks_the_message_read() {
    let mut app = app_with_messages(1);
    app.open_pager(String::new(), Signature::none(), Vec::new());
    assert!(!app.messages[0].unread);
    assert_eq!(app.pending_ops.len(), 1);
}

#[test]
fn results_replace_the_window_and_reset_selection() {
    let mut app = app_with_messages(5);
    app.apply(Action::Bottom);
    app.set_results(Vec::new(), 0, "tag:flagged".into());
    assert_eq!(app.selected, 0);
    assert_eq!(app.total_messages, 0);
    assert_eq!(app.current_query, "tag:flagged");
}

#[test]
fn a_fresh_compose_follows_the_scoped_account() {
    let mut app = app_with_accounts(&["a", "b"]);
    assert_eq!(app.compose_account(), "a");
    app.scope = ViewScope::Account("b".into());
    assert_eq!(app.compose_account(), "b");
    app.scope = ViewScope::Unified;
    assert_eq!(app.compose_account(), "a");
}

#[test]
fn app_queries_are_always_scope_conjoined() {
    let mut app = app_with_accounts(&["a", "b"]);
    assert_eq!(
        app.scoped("tag:unread").unwrap(),
        "(path:\"a/**\" or path:\"b/**\") and (tag:unread)",
    );
    app.scope = ViewScope::Account("a".into());
    let scoped = app.scoped("*").unwrap();
    assert_eq!(scoped, "(path:\"a/**\")");
    assert!(!scoped.contains('b'));
}
