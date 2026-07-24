use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

use antiphon_render::{SeriesMessage, mbox, patch_series};
use antiphon_store::{SearchIndex, StoreLayout};

use super::app::App;
use super::commands::PatchCommand;

const TAIL_LINES: usize = 2;

pub(super) fn run_pending(app: &mut App, layout: &StoreLayout) {
    let Some(command) = app.pending_patches.take() else {
        return;
    };
    let notice = match command {
        PatchCommand::Save(path) => save(app, layout, &path),
        PatchCommand::Apply(repo) => apply(app, layout, &repo),
    };
    app.notice = Some(notice);
}

fn save(app: &App, layout: &StoreLayout, path: &Path) -> String {
    let (count, bytes) = match series_mbox(app, layout) {
        Ok(built) => built,
        Err(problem) => return problem,
    };
    match std::fs::write(path, &bytes) {
        Ok(()) => {
            format!("saved {count} patch(es) to {}", path.display())
        }
        Err(error) => format!("save-patches: {error}"),
    }
}

fn apply(app: &App, layout: &StoreLayout, repo: &Path) -> String {
    if !repo.is_dir() {
        return format!("apply: {} is not a directory", repo.display());
    }
    let (count, bytes) = match series_mbox(app, layout) {
        Ok(built) => built,
        Err(problem) => return problem,
    };
    match git_am(repo, &bytes) {
        Ok(()) => {
            format!("applied {count} patch(es) in {}", repo.display())
        }
        Err(problem) => problem,
    }
}

fn series_mbox(
    app: &App,
    layout: &StoreLayout,
) -> Result<(usize, Vec<u8>), String> {
    let thread = thread_messages(app, layout)?;
    let series = patch_series(&thread);
    if series.is_empty() {
        return Err("no patches in this thread".to_string());
    }
    Ok((series.len(), mbox(&series)))
}

fn thread_messages(
    app: &App,
    layout: &StoreLayout,
) -> Result<Vec<SeriesMessage>, String> {
    let Some(selected) = app.selected_message() else {
        return Err("no message selected".to_string());
    };
    let query = format!("thread:{}", selected.thread_id);
    let effective =
        app.scoped(&query).map_err(|error| error.to_string())?;
    let index =
        SearchIndex::open(layout).map_err(|error| error.to_string())?;
    let summaries = index
        .query(&effective, None)
        .map_err(|error| error.to_string())?;
    let mut thread = Vec::with_capacity(summaries.len());
    for summary in summaries {
        let raw = std::fs::read(&summary.path).map_err(|error| {
            format!("{}: {error}", summary.path.display())
        })?;
        thread.push(SeriesMessage {
            subject: summary.subject,
            date_unix: summary.date_unix,
            raw,
        });
    }
    Ok(thread)
}

/// Runs plain `git am` on the series written to a temp file.
/// A failed am is left in git's own am state so the user can
/// resolve it with git am --continue or --abort.
fn git_am(repo: &Path, series: &[u8]) -> Result<(), String> {
    let mbox_path = temp_mbox_path();
    std::fs::write(&mbox_path, series)
        .map_err(|error| format!("apply: {error}"))?;
    let output = Command::new("git")
        .arg("am")
        .arg(&mbox_path)
        .current_dir(repo)
        .output();
    let _ = std::fs::remove_file(&mbox_path);
    let output = output.map_err(|error| format!("git am: {error}"))?;
    if output.status.success() {
        return Ok(());
    }
    Err(format!(
        "git am: {}",
        output_tail(&output.stdout, &output.stderr)
    ))
}

fn temp_mbox_path() -> PathBuf {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let nonce = COUNTER.fetch_add(1, Ordering::Relaxed);
    let name =
        format!("antiphon-am-{}-{nonce}.mbox", std::process::id());
    std::env::temp_dir().join(name)
}

fn output_tail(stdout: &[u8], stderr: &[u8]) -> String {
    let stdout = String::from_utf8_lossy(stdout);
    let stderr = String::from_utf8_lossy(stderr);
    let mut lines: Vec<&str> = stdout
        .lines()
        .chain(stderr.lines())
        .filter(|line| !line.trim().is_empty())
        .collect();
    let keep_from = lines.len().saturating_sub(TAIL_LINES);
    lines.drain(..keep_from);
    lines.join("; ")
}

#[cfg(test)]
mod tests {
    use super::super::testkit::TempDir;
    use super::*;

    const PATCH_RAW: &str = concat!(
        "From: Dev <dev@example.com>\n",
        "Subject: [PATCH] add hello\n",
        "Date: Thu, 01 Jan 2026 00:00:00 +0000\n",
        "Message-Id: <patch-1@example.com>\n",
        "\n",
        "---\n",
        " hello.txt | 1 +\n",
        " 1 file changed, 1 insertion(+)\n",
        "\n",
        "diff --git a/hello.txt b/hello.txt\n",
        "new file mode 100644\n",
        "index 0000000..ce01362\n",
        "--- /dev/null\n",
        "+++ b/hello.txt\n",
        "@@ -0,0 +1 @@\n",
        "+hello\n",
        "-- \n",
        "2.45.0\n",
    );

    const BROKEN_RAW: &str = concat!(
        "From: Dev <dev@example.com>\n",
        "Subject: [PATCH] break\n",
        "Date: Thu, 01 Jan 2026 00:00:00 +0000\n",
        "Message-Id: <patch-2@example.com>\n",
        "\n",
        "---\n",
        "diff --git a/missing.txt b/missing.txt\n",
        "index 1111111..2222222 100644\n",
        "--- a/missing.txt\n",
        "+++ b/missing.txt\n",
        "@@ -1 +1 @@\n",
        "-absent\n",
        "+present\n",
        "-- \n",
        "2.45.0\n",
    );

    fn git_usable() -> bool {
        let probe = Command::new("git").arg("--version").output();
        matches!(probe, Ok(out) if out.status.success())
    }

    fn init_repo(root: &Path) {
        let steps: &[&[&str]] = &[
            &["init", "--quiet"],
            &["config", "user.name", "Antiphon Test"],
            &["config", "user.email", "antiphon-test@example.com"],
            &["config", "commit.gpgsign", "false"],
            &["commit", "--allow-empty", "-m", "init", "--quiet"],
        ];
        for args in steps {
            let status = Command::new("git")
                .args(*args)
                .current_dir(root)
                .status()
                .expect("running git");
            assert!(status.success(), "git {args:?}");
        }
    }

    fn series_bytes(raw: &str) -> Vec<u8> {
        let thread = vec![SeriesMessage {
            subject: "[PATCH] test".to_string(),
            date_unix: 1,
            raw: raw.as_bytes().to_vec(),
        }];
        let series = patch_series(&thread);
        assert_eq!(series.len(), 1);
        mbox(&series)
    }

    #[test]
    fn git_am_applies_a_series_mbox() {
        if !git_usable() {
            eprintln!("SKIP: no usable git CLI");
            return;
        }
        let dir = TempDir::new();
        init_repo(&dir.path);
        git_am(&dir.path, &series_bytes(PATCH_RAW))
            .expect("git am succeeds");
        let hello = std::fs::read_to_string(dir.path.join("hello.txt"))
            .expect("the applied file");
        assert_eq!(hello, "hello\n");
    }

    #[test]
    fn git_am_failure_surfaces_the_output_tail() {
        if !git_usable() {
            eprintln!("SKIP: no usable git CLI");
            return;
        }
        let dir = TempDir::new();
        init_repo(&dir.path);
        let problem = git_am(&dir.path, &series_bytes(BROKEN_RAW))
            .expect_err("git am fails");
        assert!(problem.starts_with("git am: "), "{problem}");
        assert!(problem.len() > "git am: ".len(), "{problem}");
    }

    #[test]
    fn output_tails_keep_the_last_meaningful_lines() {
        let cases: &[(&[u8], &[u8], &str)] = &[
            (b"", b"error: bad patch\n", "error: bad patch"),
            (
                b"Applying: one\n\n",
                b"error: x\nhint: y\n",
                "error: x; hint: y",
            ),
            (b"", b"", ""),
        ];
        for (stdout, stderr, expected) in cases {
            assert_eq!(
                output_tail(stdout, stderr),
                *expected,
                "stdout {stdout:?} stderr {stderr:?}"
            );
        }
    }
}
