use std::{
    env,
    ffi::{OsStr, OsString},
    io,
    path::Path,
    process::Command,
};

use crate::{Outcome, lint};

enum GateKind {
    Cargo(&'static [&'static str]),
    LintComments,
}

struct Gate {
    name: &'static str,
    kind: GateKind,
}

const GATES: [Gate; 5] = [
    Gate {
        name: "cargo fmt --check",
        kind: GateKind::Cargo(&["fmt", "--check"]),
    },
    Gate {
        name: "cargo clippy --locked --workspace --all-targets -- -D warnings",
        kind: GateKind::Cargo(&[
            "clippy",
            "--locked",
            "--workspace",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ]),
    },
    Gate {
        name: "lint-comments",
        kind: GateKind::LintComments,
    },
    Gate {
        name: "cargo machete",
        kind: GateKind::Cargo(&["machete"]),
    },
    Gate {
        name: "cargo deny --locked check",
        kind: GateKind::Cargo(&["deny", "--locked", "check"]),
    },
];

const CARGO_RUN_VARIABLES: [&str; 5] = [
    "CARGO_MANIFEST_DIR",
    "CARGO_MANIFEST_PATH",
    "CARGO_CRATE_NAME",
    "CARGO_BIN_NAME",
    "CARGO_PRIMARY_PACKAGE",
];

pub fn run(root: &Path) -> io::Result<Outcome> {
    let cargo = env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"));
    for gate in &GATES {
        println!("==> {}", gate.name);
        let outcome = match gate.kind {
            GateKind::Cargo(arguments) => run_cargo(&cargo, root, arguments)?,
            GateKind::LintComments => lint::run(root),
        };
        if let Outcome::Failed = outcome {
            eprintln!("gate failed: {}", gate.name);
            return Ok(Outcome::Failed);
        }
    }
    println!("all gates passed");
    Ok(Outcome::Passed)
}

fn run_cargo(cargo: &OsStr, root: &Path, arguments: &[&str]) -> io::Result<Outcome> {
    let mut command = Command::new(cargo);
    command.args(arguments).current_dir(root);
    for (key, _) in env::vars_os() {
        if is_cargo_run_variable(&key) {
            command.env_remove(key);
        }
    }
    let status = command.status().map_err(|error| {
        io::Error::new(
            error.kind(),
            format!(
                "failed to start {} {}: {error}",
                cargo.display(),
                arguments.join(" ")
            ),
        )
    })?;
    Ok(if status.success() {
        Outcome::Passed
    } else {
        Outcome::Failed
    })
}

fn is_cargo_run_variable(key: &OsStr) -> bool {
    key.to_str()
        .is_some_and(|key| key.starts_with("CARGO_PKG_") || CARGO_RUN_VARIABLES.contains(&key))
}
