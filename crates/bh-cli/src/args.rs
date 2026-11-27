use std::ffi::OsString;

use bh_types::Effort;

pub(crate) const DEFAULT_MODEL: &str = "claude-opus-5-5";

pub(crate) const USAGE: &str = "\
Usage: bh chat [--model <id>] [--effort low|medium|high] [--show-thinking] [--] [prompt...]

Streams a single reply from the Anthropic Messages API.
The prompt is read from the arguments, or from stdin when no prompt argument is given.
The API key is read from the ANTHROPIC_API_KEY environment variable.

Options:
  --model <id>       Model id (default: claude-opus-5-5)
  --effort <level>   Reasoning effort: low, medium or high (default: medium)
  --show-thinking    Print summarized thinking to stderr
  -h, --help         Print this help
";

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Command {
    Help,
    Chat(ChatOptions),
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ChatOptions {
    pub(crate) model: String,
    pub(crate) effort: Effort,
    pub(crate) show_thinking: bool,
    pub(crate) prompt: Option<String>,
}

pub(crate) fn unicode_arguments(
    arguments: impl IntoIterator<Item = OsString>,
) -> Result<Vec<String>, String> {
    arguments
        .into_iter()
        .enumerate()
        .map(|(index, argument)| {
            argument.into_string().map_err(|argument| {
                format!(
                    "argument {} is not valid Unicode: '{}'",
                    index + 1,
                    argument.to_string_lossy()
                )
            })
        })
        .collect()
}

pub(crate) fn parse(arguments: impl IntoIterator<Item = String>) -> Result<Command, String> {
    let mut arguments = arguments.into_iter();
    match arguments.next().as_deref() {
        Some("chat") => parse_chat(arguments),
        Some("-h" | "--help" | "help") => Ok(Command::Help),
        Some(other) => Err(format!("unknown command '{other}'")),
        None => Err("missing command".to_owned()),
    }
}

fn parse_chat(mut arguments: impl Iterator<Item = String>) -> Result<Command, String> {
    let mut options = ChatOptions {
        model: DEFAULT_MODEL.to_owned(),
        effort: Effort::Medium,
        show_thinking: false,
        prompt: None,
    };
    let mut words = Vec::new();
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "-h" | "--help" => return Ok(Command::Help),
            "--model" => {
                let model = value_for("--model", arguments.next())?;
                if model.trim().is_empty() {
                    return Err("--model must not be empty".to_owned());
                }
                options.model = model;
            }
            "--effort" => options.effort = parse_effort(&value_for("--effort", arguments.next())?)?,
            "--show-thinking" => options.show_thinking = true,
            "--" => {
                words.extend(arguments.by_ref());
            }
            option if option.starts_with("--") => return Err(format!("unknown option '{option}'")),
            _ => words.push(argument),
        }
    }
    if !words.is_empty() {
        options.prompt = Some(words.join(" "));
    }
    Ok(Command::Chat(options))
}

fn value_for(option: &str, value: Option<String>) -> Result<String, String> {
    value.ok_or_else(|| format!("{option} requires a value"))
}

fn parse_effort(value: &str) -> Result<Effort, String> {
    match value {
        "low" => Ok(Effort::Low),
        "medium" => Ok(Effort::Medium),
        "high" => Ok(Effort::High),
        other => Err(format!(
            "invalid effort '{other}', expected low, medium or high"
        )),
    }
}
