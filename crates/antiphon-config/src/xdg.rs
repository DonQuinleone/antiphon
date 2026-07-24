use std::ffi::OsString;
use std::path::{Path, PathBuf};

const APP: &str = "antiphon";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dirs {
    pub config: PathBuf,
    pub state: PathBuf,
    pub cache: PathBuf,
    pub data: PathBuf,
}

impl Dirs {
    pub fn from_process() -> Option<Dirs> {
        let home = std::env::home_dir()?;
        Some(resolve(|var| std::env::var_os(var), &home))
    }

    pub fn store_root(&self) -> PathBuf {
        self.data.join("store")
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
        data: base(&env, home, "XDG_DATA_HOME", ".local/share"),
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

    fn assert_follows_spec(
        var: &str,
        fallback: &str,
        dir_of: fn(&Dirs) -> &Path,
    ) {
        let fallen = format!("/home/q/{fallback}/antiphon");
        let cases = [
            ("set", Some("/xdg/base"), "/xdg/base/antiphon"),
            ("unset", None, fallen.as_str()),
            ("empty", Some(""), fallen.as_str()),
            ("relative", Some("rel"), fallen.as_str()),
        ];
        for (name, value, expected) in cases {
            let env = |candidate: &str| {
                if candidate != var {
                    return None;
                }
                value.map(OsString::from)
            };
            let dirs = resolve(env, Path::new("/home/q"));
            assert_eq!(
                dir_of(&dirs),
                Path::new(expected),
                "{var} case {name}"
            );
        }
    }

    #[test]
    fn config_dir_follows_the_spec() {
        assert_follows_spec("XDG_CONFIG_HOME", ".config", |dirs| {
            dirs.config.as_path()
        });
    }

    #[test]
    fn data_dir_follows_the_spec() {
        assert_follows_spec("XDG_DATA_HOME", ".local/share", |dirs| {
            dirs.data.as_path()
        });
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

    #[test]
    fn store_root_hangs_off_the_data_dir() {
        let dirs = resolve(|_| None, Path::new("/home/q"));
        assert_eq!(
            dirs.store_root(),
            PathBuf::from("/home/q/.local/share/antiphon/store")
        );
    }
}
