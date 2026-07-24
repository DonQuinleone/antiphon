//! Drives the LUKS2 backend against the real system tools so
//! CI exercises exactly the command sequences the backend
//! issues (scripts/ci-luks-test.sh calls this rather than
//! duplicating them in shell). Linux only.

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("luks-cycle exercises LUKS2; Linux only");
    std::process::exit(1);
}

#[cfg(target_os = "linux")]
fn main() {
    linux::main()
}

#[cfg(target_os = "linux")]
mod linux {
    use std::io::Read;
    use std::path::PathBuf;
    use std::{env, io, process};

    use antiphon_vault::{Auth, CreateOptions, Luks2Vault, Vault};
    use secrecy::SecretString;

    const CONTAINER_BYTES: u64 = 128 * 1024 * 1024;
    const POSITIONAL_ARGS: usize = 4;
    const USAGE_EXIT: i32 = 2;
    const USAGE: &str = "usage: luks-cycle \
        <status|create|unlock|lock> <container> <mapper> \
        <mount> (passphrase on stdin for create and unlock)";

    pub fn main() {
        let args: Vec<String> = env::args().skip(1).collect();
        if args.len() != POSITIONAL_ARGS {
            die(USAGE);
        }
        let vault = Luks2Vault::new(
            PathBuf::from(&args[1]),
            args[2].clone(),
            PathBuf::from(&args[3]),
            owner(),
        );
        let outcome = match args[0].as_str() {
            "status" => {
                println!("{}", status_word(&vault));
                Ok(())
            }
            "create" => vault.create(&CreateOptions {
                auth: read_passphrase(),
                size_bytes: CONTAINER_BYTES,
            }),
            "unlock" => vault.unlock(&read_passphrase()).map(drop),
            "lock" => vault.lock(),
            _ => die(USAGE),
        };
        if let Err(error) = outcome {
            die(&error.to_string());
        }
    }

    fn status_word(vault: &Luks2Vault) -> &'static str {
        use antiphon_vault::VaultStatus;
        match vault.status() {
            VaultStatus::Absent => "absent",
            VaultStatus::Sealed => "sealed",
            VaultStatus::Open => "open",
        }
    }

    fn read_passphrase() -> Auth {
        let mut buffer = String::new();
        if io::stdin().read_to_string(&mut buffer).is_err() {
            die("cannot read the passphrase from stdin");
        }
        Auth::Passphrase(SecretString::from(buffer))
    }

    fn owner() -> String {
        env::var("USER").unwrap_or_else(|_| "root".to_owned())
    }

    fn die(message: &str) -> ! {
        eprintln!("{message}");
        process::exit(USAGE_EXIT);
    }
}
