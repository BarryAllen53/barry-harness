mod args;

use std::{
    env::{self, VarError},
    fmt,
    io::{self, IsTerminal, Read, Write},
    process::ExitCode,
};

use bh_anthropic::{AnthropicConfig, AnthropicProvider, capabilities_for, plan_cache};
use bh_provider::{
    ApiKey, BlockKind, Provider, ProviderError, ProviderEvent, ProviderRequest, ReasoningConfig,
    ReasoningDisplay,
};
use bh_types::{ContentBlock, Message, Role, StopReason, SystemPrompt, TextBlock, Usage};
use futures_util::StreamExt;
use tokio_util::sync::CancellationToken;

use crate::args::{ChatOptions, Command, USAGE};

const API_KEY_VARIABLE: &str = "ANTHROPIC_API_KEY";
const EXIT_FAILURE: u8 = 1;
const EXIT_USAGE: u8 = 2;
const EXIT_CANCELLED: u8 = 130;

enum CliError {
    Usage(String),
    Provider(ProviderError),
    Io(io::Error),
}

impl From<ProviderError> for CliError {
    fn from(error: ProviderError) -> Self {
        Self::Provider(error)
    }
}

impl From<io::Error> for CliError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(message) => write!(formatter, "error: {message}"),
            Self::Provider(error) => {
                let code = error.code();
                write!(formatter, "error [{}]: {error}", code.as_str())?;
                if let Some(action) = code.user_action() {
                    write!(formatter, " (suggested action: {})", action.as_str())?;
                }
                Ok(())
            }
            Self::Io(error) => write!(formatter, "error: I/O failure: {error}"),
        }
    }
}

struct Reply {
    stop_reason: StopReason,
    raw_stop: Option<String>,
    usage: Usage,
}

fn main() -> ExitCode {
    let command = match args::unicode_arguments(env::args_os().skip(1)).and_then(args::parse) {
        Ok(command) => command,
        Err(message) => {
            eprintln!("error: {message}\n\n{USAGE}");
            return ExitCode::from(EXIT_USAGE);
        }
    };
    let options = match command {
        Command::Help => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Command::Chat(options) => options,
    };
    match run_chat(&options) {
        Ok(reply) => {
            report_reply(&reply);
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(match error {
                CliError::Usage(_) => EXIT_USAGE,
                CliError::Provider(ProviderError::Cancelled) => EXIT_CANCELLED,
                CliError::Provider(_) | CliError::Io(_) => EXIT_FAILURE,
            })
        }
    }
}

fn run_chat(options: &ChatOptions) -> Result<Reply, CliError> {
    let prompt = match options.prompt.clone() {
        Some(prompt) => prompt,
        None => read_prompt_from_stdin()?,
    };
    if prompt.trim().is_empty() {
        return Err(CliError::Usage("the prompt is empty".to_owned()));
    }
    let api_key = api_key_from(env::var(API_KEY_VARIABLE))?;
    let provider = AnthropicProvider::new(&api_key, &AnthropicConfig::default())?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(chat(&provider, options, prompt))
}

fn api_key_from(variable: Result<String, VarError>) -> Result<ApiKey, ProviderError> {
    let reason = match variable {
        Ok(value) => return ApiKey::new(value),
        Err(VarError::NotPresent) => "is not set",
        Err(VarError::NotUnicode(_)) => "is not valid Unicode",
    };
    Err(ProviderError::Auth {
        message: format!("{API_KEY_VARIABLE} {reason}"),
    })
}

fn read_prompt_from_stdin() -> Result<String, CliError> {
    let mut stdin = io::stdin();
    if stdin.is_terminal() {
        return Err(CliError::Usage(
            "provide a prompt as arguments or pipe it through stdin".to_owned(),
        ));
    }
    let mut prompt = String::new();
    stdin.read_to_string(&mut prompt)?;
    Ok(prompt)
}

async fn chat(
    provider: &AnthropicProvider,
    options: &ChatOptions,
    prompt: String,
) -> Result<Reply, CliError> {
    let cancel = CancellationToken::new();
    let watcher = tokio::spawn(cancel_on_ctrl_c(cancel.clone()));
    let capabilities = capabilities_for(&options.model);
    let system = SystemPrompt::default();
    let messages = vec![Message {
        role: Role::User,
        blocks: vec![ContentBlock::Text(TextBlock { text: prompt })],
        origin: None,
    }];
    let cache = plan_cache(&[], &system, &messages, &capabilities.cache);
    let request = ProviderRequest {
        model: options.model.clone(),
        system,
        messages,
        tools: Vec::new(),
        max_output_tokens: capabilities.max_output_tokens,
        reasoning: ReasoningConfig {
            effort: options.effort,
            display: if options.show_thinking {
                ReasoningDisplay::Summarized
            } else {
                ReasoningDisplay::Omitted
            },
        },
        cache,
    };
    let reply = stream_reply(provider, request, cancel, options.show_thinking).await;
    watcher.abort();
    reply
}

async fn cancel_on_ctrl_c(cancel: CancellationToken) {
    match tokio::signal::ctrl_c().await {
        Ok(()) => cancel.cancel(),
        Err(error) => eprintln!("warning: Ctrl+C cannot be handled: {error}"),
    }
}

async fn stream_reply(
    provider: &AnthropicProvider,
    request: ProviderRequest,
    cancel: CancellationToken,
    show_thinking: bool,
) -> Result<Reply, CliError> {
    let mut events = provider.stream(request, cancel).await?;
    let mut stdout = io::stdout().lock();
    let mut stderr = io::stderr().lock();
    let mut ends_with_newline = true;
    let mut reply = None;
    while let Some(event) = events.next().await {
        match event? {
            ProviderEvent::TextDelta { text, .. } => {
                if let Some(last) = text.chars().last() {
                    ends_with_newline = last == '\n';
                }
                stdout.write_all(text.as_bytes())?;
                stdout.flush()?;
            }
            ProviderEvent::BlockStart {
                kind: BlockKind::Reasoning,
                ..
            } if show_thinking => write!(stderr, "[thinking] ")?,
            ProviderEvent::ReasoningSummaryDelta { text, .. } if show_thinking => {
                stderr.write_all(text.as_bytes())?;
                stderr.flush()?;
            }
            ProviderEvent::BlockDone {
                block: ContentBlock::Reasoning(_),
                ..
            } if show_thinking => writeln!(stderr)?,
            ProviderEvent::Warning { code, message } => {
                writeln!(stderr, "warning [{code}]: {message}")?;
            }
            ProviderEvent::MessageStop {
                stop_reason,
                raw_stop,
                usage,
            } => {
                reply = Some(Reply {
                    stop_reason,
                    raw_stop,
                    usage,
                });
            }
            _ => {}
        }
    }
    if !ends_with_newline {
        writeln!(stdout)?;
    }
    stdout.flush()?;
    reply.ok_or(CliError::Provider(ProviderError::TruncatedStream))
}

fn report_reply(reply: &Reply) {
    let usage = &reply.usage;
    let stop = reply
        .raw_stop
        .clone()
        .unwrap_or_else(|| format!("{:?}", reply.stop_reason));
    eprintln!(
        "stop: {stop} | input {} | output {} | cache read {} | cache write 5m {} 1h {} | hit ratio {:.1}%",
        usage.input_tokens,
        usage.output_tokens,
        usage.cache_read_tokens,
        usage.cache_write_5m_tokens,
        usage.cache_write_1h_tokens,
        usage.cache_hit_ratio() * 100.0,
    );
}
