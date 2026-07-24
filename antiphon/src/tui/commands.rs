use std::path::PathBuf;

use super::app::App;
use super::crypto::PgpPlan;

/// Per-message overrides of the identity's pgp defaults:
/// command name, whether it targets signing (else encryption),
/// and the value it arms for the next compose.
const PGP_TOGGLES: [(&str, bool, bool); 4] = [
    ("sign", true, true),
    ("nosign", true, false),
    ("encrypt", false, true),
    ("noencrypt", false, false),
];

type ArgHandler = fn(&mut App, &str);

/// Commands taking one argument: name, usage line, and the
/// handler arming the app state the event loop consumes.
const ARG_COMMANDS: [(&str, &str, ArgHandler); 3] = [
    ("template", "template <name>", arm_template),
    ("save-patches", "save-patches <path>", arm_save_patches),
    ("apply", "apply <repo-dir>", arm_apply),
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PatchCommand {
    Save(PathBuf),
    Apply(PathBuf),
}

fn arm_template(app: &mut App, name: &str) {
    app.pending_template = Some(name.to_string());
}

fn arm_save_patches(app: &mut App, path: &str) {
    app.pending_patches = Some(PatchCommand::Save(path.into()));
}

fn arm_apply(app: &mut App, repo: &str) {
    app.pending_patches = Some(PatchCommand::Apply(repo.into()));
}

fn argument_of<'a>(command: &'a str, name: &str) -> Option<&'a str> {
    if command == name {
        return Some("");
    }
    let rest = command.strip_prefix(name)?;
    let rest = rest.strip_prefix(' ')?;
    Some(rest.trim())
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FrameStats {
    pub frames: u64,
    pub last_micros: u128,
    pub max_micros: u128,
    total_micros: u128,
}

impl FrameStats {
    pub fn record(&mut self, elapsed: std::time::Duration) {
        let micros = elapsed.as_micros();
        self.frames += 1;
        self.last_micros = micros;
        self.max_micros = self.max_micros.max(micros);
        self.total_micros += micros;
    }

    pub fn mean_micros(&self) -> u128 {
        if self.frames == 0 {
            return 0;
        }
        self.total_micros / u128::from(self.frames)
    }

    pub fn summary(&self) -> String {
        format!(
            "frames: {} drawn, last {} us, mean {} us, max {} us",
            self.frames,
            self.last_micros,
            self.mean_micros(),
            self.max_micros,
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptKind {
    Search,
    Command,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prompt {
    pub kind: PromptKind,
    pub buffer: String,
}

impl App {
    pub(super) fn open_prompt(&mut self, kind: PromptKind) {
        self.prompt = Some(Prompt {
            kind,
            buffer: String::new(),
        });
    }

    pub fn prompt_push(&mut self, ch: char) {
        if let Some(prompt) = &mut self.prompt {
            prompt.buffer.push(ch);
        }
    }

    pub fn prompt_backspace(&mut self) {
        if let Some(prompt) = &mut self.prompt {
            prompt.buffer.pop();
        }
    }

    pub fn prompt_cancel(&mut self) {
        self.prompt = None;
    }

    pub fn prompt_submit(&mut self) -> Option<Prompt> {
        self.prompt.take()
    }

    /// The pgp plan for a compose starting now: the identity
    /// default with any armed per-message overrides consumed.
    pub fn take_pgp_plan(&mut self, default_sign: bool) -> PgpPlan {
        PgpPlan {
            sign: self.pending_sign.take().unwrap_or(default_sign),
            encrypt: self.pending_encrypt.take().unwrap_or(false),
        }
    }

    fn pgp_toggle(&mut self, command: &str) -> bool {
        let Some((_, is_sign, value)) =
            PGP_TOGGLES.iter().find(|(name, _, _)| *name == command)
        else {
            return false;
        };
        let (slot, what) = if *is_sign {
            (&mut self.pending_sign, "signing")
        } else {
            (&mut self.pending_encrypt, "encryption")
        };
        *slot = Some(*value);
        let state = if *value { "on" } else { "off" };
        self.notice = Some(format!("next compose: {what} {state}"));
        true
    }

    pub fn run_command(&mut self, command: &str) {
        let command = command.trim();
        if self.pgp_toggle(command) {
            return;
        }
        if self.arg_command(command) {
            return;
        }
        match command {
            "q" | "quit" => self.quit = true,
            "frames" => self.notice = Some(self.frame_stats.summary()),
            "" => {}
            other => {
                self.notice = Some(format!("unknown command: {other}"))
            }
        }
    }

    fn arg_command(&mut self, command: &str) -> bool {
        for (name, usage, arm) in ARG_COMMANDS {
            let Some(argument) = argument_of(command, name) else {
                continue;
            };
            if argument.is_empty() {
                self.notice = Some(format!("usage: {usage}"));
            } else {
                arm(self, argument);
            }
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use antiphon_core::Action;

    use super::super::app::app_with_messages;
    use super::*;

    #[test]
    fn prompt_edits_cancels_and_submits() {
        let mut app = app_with_messages(1);
        app.apply(Action::Search);
        for ch in "tag:unread".chars() {
            app.prompt_push(ch);
        }
        app.prompt_backspace();
        let prompt = app.prompt_submit().expect("open prompt");
        assert_eq!(prompt.kind, PromptKind::Search);
        assert_eq!(prompt.buffer, "tag:unrea");
        assert!(app.prompt.is_none());

        app.apply(Action::Command);
        app.prompt_cancel();
        assert!(app.prompt.is_none());
    }

    #[test]
    fn frame_stats_track_last_mean_and_max() {
        let mut stats = FrameStats::default();
        stats.record(std::time::Duration::from_micros(100));
        stats.record(std::time::Duration::from_micros(300));
        assert_eq!(stats.frames, 2);
        assert_eq!(stats.last_micros, 300);
        assert_eq!(stats.max_micros, 300);
        assert_eq!(stats.mean_micros(), 200);
        assert_eq!(FrameStats::default().mean_micros(), 0);
    }

    #[test]
    fn pgp_toggles_shape_the_next_compose_plan() {
        let cases: &[(bool, &[&str], bool, bool)] = &[
            (false, &[], false, false),
            (true, &[], true, false),
            (true, &["nosign"], false, false),
            (false, &["sign"], true, false),
            (false, &["encrypt"], false, true),
            (false, &["sign", "encrypt"], true, true),
            (true, &["encrypt"], true, true),
            (true, &["encrypt", "noencrypt"], true, false),
            (false, &["encrypt", "nosign"], false, true),
            (false, &["sign", "nosign"], false, false),
        ];
        for (default_sign, commands, sign, encrypt) in cases {
            let mut app = app_with_messages(1);
            for command in *commands {
                app.run_command(command);
            }
            let plan = app.take_pgp_plan(*default_sign);
            let expected = PgpPlan {
                sign: *sign,
                encrypt: *encrypt,
            };
            assert_eq!(plan, expected, "{commands:?}");
        }
    }

    #[test]
    fn pgp_overrides_are_consumed_by_one_compose() {
        let mut app = app_with_messages(1);
        app.run_command("sign");
        app.run_command("encrypt");
        assert!(
            app.notice
                .as_deref()
                .is_some_and(|n| n.contains("encryption on"))
        );
        let armed = app.take_pgp_plan(false);
        assert_eq!(
            armed,
            PgpPlan {
                sign: true,
                encrypt: true,
            }
        );
        let next = app.take_pgp_plan(false);
        assert_eq!(next, PgpPlan::default());
        assert!(app.take_pgp_plan(true).sign);
    }

    #[test]
    fn patch_commands_arm_pending_state() {
        let mut app = app_with_messages(1);
        app.run_command("save-patches");
        assert_eq!(
            app.notice.as_deref(),
            Some("usage: save-patches <path>")
        );
        assert!(app.pending_patches.is_none());
        app.run_command("save-patches /tmp/series.mbox");
        assert_eq!(
            app.pending_patches,
            Some(PatchCommand::Save("/tmp/series.mbox".into()))
        );
        app.run_command("apply ../repo");
        assert_eq!(
            app.pending_patches,
            Some(PatchCommand::Apply("../repo".into()))
        );
        app.run_command("template reply");
        assert_eq!(app.pending_template.as_deref(), Some("reply"));
    }

    #[test]
    fn commands_quit_or_complain() {
        let mut app = app_with_messages(1);
        app.run_command("nonsense");
        assert!(
            app.notice
                .as_deref()
                .is_some_and(|n| n.contains("nonsense"))
        );
        app.run_command("q");
        assert!(app.quit);
    }
}
