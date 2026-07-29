use super::super::testkit::app_with_messages;
use super::*;

fn summary(id: &str, path: &str, subject: &str) -> MessageSummary {
    MessageSummary {
        id: id.to_string(),
        thread_id: String::new(),
        subject: subject.to_string(),
        from: String::new(),
        to: String::new(),
        date_unix: 0,
        tags: Vec::new(),
        unread: false,
        path: std::path::PathBuf::from(path),
        in_reply_to: None,
        references: Vec::new(),
    }
}

fn routed_app() -> App {
    let mut app = app_with_messages(1);
    app.trash_folders = vec![
        ("work".to_string(), "trash".to_string()),
        ("home".to_string(), "bin".to_string()),
    ];
    app.archive_folders =
        vec![("work".to_string(), "Archief".to_string())];
    app.folder_aliases = vec![(
        "work".to_string(),
        "lists/rust".to_string(),
        "rust".to_string(),
    )];
    app
}

#[test]
fn the_four_commands_arm_their_actions() {
    let cases: &[(&str, BulkAction)] = &[
        ("trash-all", BulkAction::Trash),
        ("archive-all", BulkAction::Archive),
        ("delete-all", BulkAction::Delete),
        ("move-all lists/rust", BulkAction::Move("lists/rust".into())),
    ];
    for (command, action) in cases {
        let mut app = app_with_messages(1);
        app.run_command(command);
        assert_eq!(
            app.bulk,
            Some(Bulk::Armed(action.clone())),
            "{command}"
        );
    }
}

#[test]
fn move_all_without_a_folder_arms_nothing() {
    let mut app = app_with_messages(1);
    app.run_command("move-all");
    assert!(app.bulk.is_none());
    assert_eq!(app.notice.as_deref(), Some("usage: move-all <folder>"));
}

#[test]
fn trash_routes_each_message_to_its_own_account_folder() {
    let app = routed_app();
    let summaries = [
        summary("m1", "store/maildir/work/cur/1.eml", "One"),
        summary("m2", "store/maildir/home/lists/rust/new/2.eml", "Two"),
    ];
    let intents = intents_for(&app, &BulkAction::Trash, &summaries);
    assert_eq!(intents.len(), 2);
    assert_eq!(
        intents[0],
        OpIntent::Move {
            account: "work".to_string(),
            message_id: "m1".to_string(),
            to_folder: "trash".to_string(),
            from_folder: String::new(),
        }
    );
    assert_eq!(
        intents[1],
        OpIntent::Move {
            account: "home".to_string(),
            message_id: "m2".to_string(),
            to_folder: "bin".to_string(),
            from_folder: "lists/rust".to_string(),
        }
    );
}

#[test]
fn a_message_synced_to_two_accounts_routes_per_account() {
    let app = routed_app();
    let summaries = [
        summary("shared", "store/maildir/work/cur/a.eml", "Bcc"),
        summary("shared", "store/maildir/home/cur/b.eml", "Bcc"),
    ];
    let intents = intents_for(&app, &BulkAction::Archive, &summaries);
    let accounts: Vec<&str> = intents
        .iter()
        .map(|op| match op {
            OpIntent::Move { account, .. } => account.as_str(),
            _ => panic!("expected moves"),
        })
        .collect();
    assert_eq!(accounts, ["work", "home"]);
    let OpIntent::Move { to_folder, .. } = &intents[0] else {
        panic!("expected a move");
    };
    assert_eq!(to_folder, "Archief", "work's archive folder");
}

#[test]
fn move_all_resolves_the_folder_alias_per_account() {
    let app = routed_app();
    let summaries =
        [summary("m1", "store/maildir/work/cur/1.eml", "One")];
    let action = BulkAction::Move("rust".to_string());
    let intents = intents_for(&app, &action, &summaries);
    let OpIntent::Move { to_folder, .. } = &intents[0] else {
        panic!("expected a move");
    };
    assert_eq!(to_folder, "lists/rust");
}

#[test]
fn delete_all_builds_delete_ops() {
    let app = routed_app();
    let summaries =
        [summary("m1", "store/maildir/work/cur/1.eml", "One")];
    let intents = intents_for(&app, &BulkAction::Delete, &summaries);
    assert_eq!(
        intents[0],
        OpIntent::Delete {
            account: "work".to_string(),
            message_id: "m1".to_string(),
        }
    );
}

#[test]
fn open_confirm_shows_the_count_and_a_capped_sample() {
    let mut app = routed_app();
    let summaries: Vec<MessageSummary> = (0..25)
        .map(|index| {
            summary(
                &format!("m{index}"),
                "store/maildir/work/cur/x.eml",
                &format!("Subject {index}"),
            )
        })
        .collect();
    open_confirm(&mut app, BulkAction::Trash, summaries);
    let Some(Bulk::Confirm(confirm)) = &app.bulk else {
        panic!("expected a confirm");
    };
    assert_eq!(confirm.count, 25);
    assert_eq!(confirm.examples.len(), EXAMPLE_LIMIT);
    assert_eq!(confirm.examples[0], "Subject 0");
    assert_eq!(confirm.intents.len(), 25);
    assert_eq!(
        app.prompt.as_ref().map(|p| p.kind),
        Some(PromptKind::ConfirmBulk)
    );
}

#[test]
fn a_blank_subject_shows_a_placeholder() {
    let mut app = routed_app();
    let summaries =
        [summary("m1", "store/maildir/work/cur/1.eml", "  ")];
    open_confirm(&mut app, BulkAction::Trash, summaries.to_vec());
    let Some(Bulk::Confirm(confirm)) = &app.bulk else {
        panic!("expected a confirm");
    };
    assert_eq!(confirm.examples, [NO_SUBJECT.to_string()]);
}

#[test]
fn open_confirm_on_no_matches_queues_nothing() {
    let mut app = routed_app();
    open_confirm(&mut app, BulkAction::Trash, Vec::new());
    assert!(app.bulk.is_none());
    assert!(app.prompt.is_none());
    assert_eq!(
        app.notice.as_deref(),
        Some("nothing matches this search")
    );
}

#[test]
fn confirming_queues_the_ops_and_drops_the_rows() {
    let mut app = app_with_messages(3);
    let intents = vec![
        OpIntent::Delete {
            account: "work".to_string(),
            message_id: "m0".to_string(),
        },
        OpIntent::Delete {
            account: "work".to_string(),
            message_id: "m1".to_string(),
        },
    ];
    app.bulk = Some(Bulk::Confirm(BulkConfirm {
        action: BulkAction::Delete,
        count: 2,
        examples: Vec::new(),
        intents,
    }));
    let queued = queue_confirmed(&mut app);
    assert_eq!(queued, 2);
    assert_eq!(app.pending_ops.len(), 2);
    let ids: Vec<&str> =
        app.messages.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, ["m2"], "the touched rows leave the view");
    assert_eq!(app.total_messages, 1);
    assert!(app.bulk.is_none());
    assert_eq!(
        app.notice.as_deref(),
        Some("queued permanent delete for 2 messages")
    );
}

#[test]
fn cancelling_queues_nothing() {
    let mut app = app_with_messages(2);
    app.bulk = Some(Bulk::Confirm(BulkConfirm {
        action: BulkAction::Trash,
        count: 2,
        examples: Vec::new(),
        intents: vec![OpIntent::Delete {
            account: "work".to_string(),
            message_id: "m0".to_string(),
        }],
    }));
    cancel(&mut app);
    assert!(app.bulk.is_none());
    assert!(app.pending_ops.is_empty());
    assert_eq!(app.messages.len(), 2, "nothing removed");
}

#[test]
fn destructive_and_large_sets_warn() {
    let delete = warning_line(&BulkAction::Delete, 3).unwrap();
    assert!(delete.contains("cannot be undone"));
    assert!(
        warning_line(&BulkAction::Trash, BULK_WARN_THRESHOLD).is_none()
    );
    let loud =
        warning_line(&BulkAction::Trash, BULK_WARN_THRESHOLD + 1)
            .unwrap();
    assert!(loud.contains("far beyond"));
}

#[test]
fn the_modal_draws_the_count_and_sample_over_the_list() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut app = routed_app();
    let summaries: Vec<MessageSummary> = (0..25)
        .map(|index| {
            summary(
                &format!("m{index}"),
                "store/maildir/work/cur/x.eml",
                &format!("Subject {index}"),
            )
        })
        .collect();
    open_confirm(&mut app, BulkAction::Trash, summaries);
    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| super::super::draw::draw(frame, &app))
        .unwrap();
    let buffer = terminal.backend().buffer().clone();
    let mut text = String::new();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            text.push_str(buffer.cell((x, y)).unwrap().symbol());
        }
        text.push('\n');
    }
    assert!(text.contains("trash 25 messages"), "{text}");
    assert!(text.contains("Subject 0"), "a sample subject shows");
    assert!(text.contains("and 15 more"), "the remainder is noted");
}
