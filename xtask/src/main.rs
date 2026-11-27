mod check;
mod comments;
mod lint;

use std::{
    env,
    path::{Path, PathBuf},
    process::ExitCode,
};

const USAGE: &str = "usage: cargo xtask <command>\n\ncommands:\n  lint-comments  fail on comments in source, config and markup files\n  check          run fmt, clippy, lint-comments, machete and deny in order";

pub enum Outcome {
    Passed,
    Failed,
}

impl From<Outcome> for ExitCode {
    fn from(outcome: Outcome) -> Self {
        match outcome {
            Outcome::Passed => Self::SUCCESS,
            Outcome::Failed => Self::FAILURE,
        }
    }
}

fn main() -> ExitCode {
    let arguments: Vec<String> = env::args().skip(1).collect();
    let root = workspace_root();
    let result = match arguments.as_slice() {
        [command] if command == "lint-comments" => Ok(lint::run(&root)),
        [command] if command == "check" => check::run(&root),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(outcome) => outcome.into(),
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives directly inside the workspace root")
        .to_path_buf()
}
