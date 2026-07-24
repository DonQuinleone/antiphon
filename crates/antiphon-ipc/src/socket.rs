use std::ffi::OsString;
use std::path::PathBuf;

const APP: &str = "antiphon";
const SOCKET_FILE: &str = "antiphond.sock";
const STATE_HOME_FALLBACK: &str = ".local/state";

pub fn socket_path(env: impl Fn(&str) -> Option<OsString>) -> PathBuf {
    socket_dir(&env).join(SOCKET_FILE)
}

fn socket_dir(env: &impl Fn(&str) -> Option<OsString>) -> PathBuf {
    absolute(env, "XDG_RUNTIME_DIR")
        .or_else(|| absolute(env, "XDG_STATE_HOME"))
        .unwrap_or_else(|| default_state_home(env))
        .join(APP)
}

fn default_state_home(
    env: &impl Fn(&str) -> Option<OsString>,
) -> PathBuf {
    absolute(env, "HOME")
        .unwrap_or_default()
        .join(STATE_HOME_FALLBACK)
}

fn absolute(
    env: &impl Fn(&str) -> Option<OsString>,
    var: &str,
) -> Option<PathBuf> {
    // The XDG spec requires ignoring empty or relative values.
    env(var)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn env_with(
        pairs: &'static [(&'static str, &'static str)],
    ) -> impl Fn(&str) -> Option<OsString> {
        move |var| {
            pairs
                .iter()
                .find(|(name, _)| *name == var)
                .map(|(_, value)| OsString::from(value))
        }
    }

    #[test]
    fn the_runtime_dir_wins_when_set() {
        let cases: &[(&[(&str, &str)], &str)] = &[
            (
                &[
                    ("XDG_RUNTIME_DIR", "/run/user/1000"),
                    ("XDG_STATE_HOME", "/state"),
                ],
                "/run/user/1000/antiphon/antiphond.sock",
            ),
            (
                &[("XDG_RUNTIME_DIR", "/run/user/1000")],
                "/run/user/1000/antiphon/antiphond.sock",
            ),
        ];
        for (pairs, expected) in cases {
            assert_eq!(
                socket_path(env_with(pairs)),
                Path::new(expected)
            );
        }
    }

    #[test]
    fn the_state_dir_catches_the_fall() {
        let cases: &[(&[(&str, &str)], &str)] = &[
            (
                &[("XDG_STATE_HOME", "/state")],
                "/state/antiphon/antiphond.sock",
            ),
            (
                &[
                    ("XDG_RUNTIME_DIR", ""),
                    ("XDG_STATE_HOME", "/state"),
                ],
                "/state/antiphon/antiphond.sock",
            ),
            (
                &[
                    ("XDG_RUNTIME_DIR", "relative/run"),
                    ("XDG_STATE_HOME", "/state"),
                ],
                "/state/antiphon/antiphond.sock",
            ),
        ];
        for (pairs, expected) in cases {
            assert_eq!(
                socket_path(env_with(pairs)),
                Path::new(expected)
            );
        }
    }

    #[test]
    fn ignore_rules_apply_to_the_state_dir_too() {
        let pairs: &[(&str, &str)] = &[
            ("XDG_STATE_HOME", "relative/state"),
            ("HOME", "/home/q"),
        ];
        assert_eq!(
            socket_path(env_with(pairs)),
            Path::new("/home/q/.local/state/antiphon/antiphond.sock")
        );
    }

    #[test]
    fn home_backstops_a_bare_environment() {
        let pairs: &[(&str, &str)] = &[("HOME", "/home/q")];
        assert_eq!(
            socket_path(env_with(pairs)),
            Path::new("/home/q/.local/state/antiphon/antiphond.sock")
        );
    }
}
