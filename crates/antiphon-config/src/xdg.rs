use std::ffi::OsString;
use std::path::{Path, PathBuf};

const APP: &str = "antiphon";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dirs {
    pub config: PathBuf,
    pub state: PathBuf,
    pub cache: PathBuf,
}

impl Dirs {
    pub fn from_process() -> Option<Dirs> {
        let home = std::env::home_dir()?;
        Some(resolve(|var| std::env::var_os(var), &home))
    }
}

pub fn resolve(
    env: impl Fn(&str) -> Option<OsString>,
    home: &Path,
) -> Dirs {
    Dirs {
        config: base(&env, home, "XDG_CONFIG_HOME", ".config"),
        state: base(&env, home, "XDG_STATE_HOME", ".local/state"),
        cache: base(&env, home, "XDG_CACHE_HOME", ".cache"),
    }
}

fn base(
    env: &impl Fn(&str) -> Option<OsString>,
    home: &Path,
    var: &str,
    fallback: &str,
) -> PathBuf {
    // The XDG spec requires ignoring empty or relative values.
    env(var)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| home.join(fallback))
        .join(APP)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CASES: &[(&str, Option<&str>, &str)] = &[
        ("set", Some("/xdg/config"), "/xdg/config/antiphon"),
        ("unset", None, "/home/q/.config/antiphon"),
        ("empty", Some(""), "/home/q/.config/antiphon"),
        ("relative", Some("rel"), "/home/q/.config/antiphon"),
    ];

    #[test]
    fn config_dir_follows_the_spec() {
        for (name, value, expected) in CASES {
            let env = |var: &str| match var {
                "XDG_CONFIG_HOME" => value.map(OsString::from),
                _ => None,
            };
            let dirs = resolve(env, Path::new("/home/q"));
            assert_eq!(
                dirs.config,
                PathBuf::from(expected),
                "case {name}"
            );
        }
    }

    #[test]
    fn state_and_cache_have_their_own_fallbacks() {
        let dirs = resolve(|_| None, Path::new("/home/q"));
        assert_eq!(
            dirs.state,
            PathBuf::from("/home/q/.local/state/antiphon")
        );
        assert_eq!(
            dirs.cache,
            PathBuf::from("/home/q/.cache/antiphon")
        );
    }
}
