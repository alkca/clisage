use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use clisage::credentials::{NativeStore, check_backend, validate_key};
use clisage::{Backend, CREDENTIAL_ID, Config, Mode, Result};
use secrecy::{ExposeSecret, SecretString};
use std::{
    io::{self, IsTerminal, Read, Write},
    path::PathBuf,
    process::{Command, Stdio},
    time::Duration,
};

#[derive(Parser)]
#[command(
    version = env!("CLISAGE_BUILD_VERSION"),
    about = "A small shell AI assistant. Generated commands are only displayed."
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Action>,
    /// Request to send (quote multiword requests).
    prompt: Option<String>,
    /// Override the configured OpenAI model.
    #[arg(long, global = true)]
    model: Option<String>,
    /// Compose the prompt in $EDITOR (falls back to $VISUAL, then vi).
    #[arg(long, global = true)]
    editor: bool,
    /// Render Markdown and syntax-highlight fenced code, even when redirected.
    #[arg(long, global = true, conflicts_with = "no_markdown")]
    markdown: bool,
    /// Stream the original response without terminal formatting.
    #[arg(long, global = true)]
    no_markdown: bool,
}
#[derive(Subcommand)]
enum Action {
    /// Suggest a shell command without executing it.
    Shell {
        prompt: Option<String>,
        /// Return one plain command after completion, for shell integration.
        #[arg(long)]
        command_only: bool,
    },
    /// Explain a shell command without executing it.
    Explain { prompt: Option<String> },
    /// Manage encrypted credentials.
    Auth {
        /// Select a store even if configuration is invalid.
        #[arg(long, value_enum, global = true)]
        backend: Option<Backend>,
        #[command(subcommand)]
        action: Auth,
    },
    /// Inspect configuration without secrets.
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Check local configuration and dependencies without reading secrets.
    Doctor,
    /// Print a shell completion script.
    Completions {
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },
    /// Print the opt-in Alt-G integration script for your shell.
    Integration {
        #[arg(value_enum)]
        shell: IntegrationShell,
    },
}
#[derive(Clone, Copy, ValueEnum)]
enum IntegrationShell {
    Zsh,
    Bash,
}
#[derive(Subcommand)]
enum Auth {
    Login,
    Status,
    Logout,
}
#[derive(Subcommand)]
enum ConfigAction {
    Show,
}

fn config_path() -> Result<PathBuf> {
    let root = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .ok_or("set HOME or XDG_CONFIG_HOME to locate configuration")?;
    if !root.is_absolute() {
        return Err("configuration directory must be absolute".into());
    }
    Ok(root.join("clisage/config.toml"))
}
fn load_config() -> Result<Config> {
    match std::fs::read_to_string(config_path()?) {
        Ok(text) => toml::from_str(&text).map_err(|error: toml::de::Error| {
            let start = error.span().map(|span| span.start).unwrap_or(0).min(text.len());
            let prefix = &text.as_bytes()[..start];
            let line = prefix.iter().filter(|&&byte| byte == b'\n').count() + 1;
            let column = prefix.iter().rposition(|&byte| byte == b'\n').map_or(start + 1, |pos| start - pos);
            format!("invalid config.toml at line {line}, column {column}; expected config_version = 1, a model, and credential_backend = 'pass', 'keychain', or 'secret-service'; secret and endpoint fields are forbidden. Repair or rename the config file; run clisage doctor for setup checks").into()
        }),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Config::default()),
        Err(_) => Err("unable to read config.toml; check file permissions or rename the file; run clisage doctor".into()),
    }
}
fn env_key() -> Result<Option<SecretString>> {
    match std::env::var("CLISAGE_API_KEY") {
        Ok(key) => validate_key(key).map(Some),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(_) => Err("CLISAGE_API_KEY must be valid Unicode".into()),
    }
}
fn pass_read() -> Result<SecretString> {
    let mut command = Command::new("pass");
    if !io::stderr().is_terminal() {
        // Cached credentials remain usable; unattended jobs must never open pinentry.
        let mut options = std::env::var_os("PASSWORD_STORE_GPG_OPTS").unwrap_or_default();
        options.push(" --pinentry-mode error");
        command.env("PASSWORD_STORE_GPG_OPTS", options);
    }
    let out = command.args(["show", CREDENTIAL_ID]).stdin(Stdio::null()).stderr(Stdio::null()).output()
        .map_err(|_| "pass unavailable; install and initialize pass/GPG, then run clisage auth login; automation can supply CLISAGE_API_KEY")?;
    if !out.status.success() {
        return Err("credential missing, locked, or unavailable; unlock/initialize pass and run clisage auth login".into());
    }
    let key = String::from_utf8(out.stdout).map_err(|_| "stored credential is invalid")?;
    validate_key(key.trim_end_matches(['\n', '\r']).to_owned())
}
fn pass_store_path() -> Result<PathBuf> {
    std::env::var_os("PASSWORD_STORE_DIR")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".password-store"))
        })
        .ok_or_else(|| "set HOME or PASSWORD_STORE_DIR to locate the pass store".into())
}

fn read_credential(backend: Backend) -> Result<SecretString> {
    if backend == Backend::Pass {
        return pass_read();
    }
    check_backend(backend)?;
    if !io::stderr().is_terminal() {
        return Err("native credential access requires an interactive terminal; supply CLISAGE_API_KEY for automation".into());
    }
    NativeStore::new(backend)?.read()
}

fn login(backend: Backend) -> Result<SecretString> {
    if backend == Backend::Pass {
        return pass_login();
    }
    check_backend(backend)?;
    if !io::stdin().is_terminal() || !io::stderr().is_terminal() {
        return Err("login requires an interactive terminal; use CLISAGE_API_KEY through your automation secret manager".into());
    }
    let store = NativeStore::new(backend)?;
    let key = validate_key(
        rpassword::prompt_password("OpenAI API key: ")
            .map_err(|_| "unable to read key from terminal")?,
    )?;
    store.store(&key)?;
    eprintln!(
        "Credential stored and read back successfully. Provider validity has not been checked."
    );
    Ok(key)
}

fn pass_login() -> Result<SecretString> {
    if !io::stdin().is_terminal() || !io::stderr().is_terminal() {
        return Err("login requires an interactive terminal; use CLISAGE_API_KEY through your automation secret manager".into());
    }
    let store = pass_store_path()?;
    let recipient_override =
        std::env::var_os("PASSWORD_STORE_KEY").is_some_and(|key| !key.is_empty());
    clisage::check_pass_store(&store, recipient_override)?;
    let available = Command::new("pass")
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|_| "pass unavailable; install pass/GPG before running clisage auth login")?;
    if !available.success() {
        return Err("pass could not start; check your pass/GPG installation".into());
    }
    eprintln!("Store an OpenAI API key in pass/GPG (requires an initialized store).");
    let key = validate_key(
        rpassword::prompt_password("OpenAI API key: ")
            .map_err(|_| "unable to read key from terminal")?,
    )?;
    let mut child = Command::new("pass")
        .args(["insert", "--multiline", "--force", CREDENTIAL_ID])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "pass unavailable; install and initialize pass/GPG first")?;
    let write_result = (|| -> io::Result<()> {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("missing credential pipe"))?;
        writeln!(stdin, "{}", key.expose_secret())
    })();
    let status = child
        .wait()
        .map_err(|_| "unable to wait for credential storage")?;
    if write_result.is_err() || !status.success() {
        return Err("could not store credential; check pass/GPG and pinentry".into());
    }
    let stored = pass_read()?;
    if stored.expose_secret() != key.expose_secret() {
        return Err("credential storage verification failed".into());
    }
    eprintln!(
        "Credential stored and read back successfully. Provider validity has not been checked."
    );
    Ok(key)
}
fn auth(action: Auth, backend: Backend) -> Result<()> {
    match action {
        Auth::Login => {
            login(backend)?;
        }
        Auth::Status => {
            if env_key()?.is_some() {
                writeln!(
                    io::stdout().lock(),
                    "Credential available from environment (not validated with provider)."
                )?;
            } else {
                read_credential(backend)?;
                writeln!(
                    io::stdout().lock(),
                    "Credential available from {backend:?} (not validated with provider)."
                )?;
            }
        }
        Auth::Logout => {
            if backend != Backend::Pass {
                check_backend(backend)?;
                if !io::stderr().is_terminal() {
                    return Err("native credential removal requires an interactive terminal".into());
                }
                NativeStore::new(backend)?.remove()?;
            } else {
                let status = Command::new("pass")
                    .args(["rm", "--force", CREDENTIAL_ID])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .map_err(|_| "pass unavailable; unable to remove stored credential")?;
                if !status.success() {
                    return Err("unable to remove credential; check pass store".into());
                }
            }
            writeln!(
                io::stdout().lock(),
                "Stored credential removed. Unset CLISAGE_API_KEY separately if configured."
            )?;
        }
    }
    Ok(())
}

/// This command never reads stored credentials, unlocks a store, or uses HTTP.
fn doctor() -> Result<()> {
    let mut output = io::stdout().lock();
    let mut healthy = true;
    let backend = match load_config() {
        Ok(config) if !config.model.trim().is_empty() => {
            writeln!(
                output,
                "Configuration: OK (config.toml or built-in defaults)"
            )?;
            config.credential_backend
        }
        Ok(_) => {
            writeln!(
                output,
                "Configuration: ERROR (model must not be empty; repair config.toml)"
            )?;
            healthy = false;
            Backend::Pass
        }
        Err(error) => {
            writeln!(output, "Configuration: ERROR ({error})")?;
            healthy = false;
            Backend::Pass
        }
    };
    match env_key() {
        Ok(Some(_)) => writeln!(
            output,
            "Credential: environment (provider validity not checked)"
        )?,
        Ok(None) => writeln!(
            output,
            "Credential: stored value not read; use clisage auth status to check availability"
        )?,
        Err(_) => {
            writeln!(
                output,
                "Credential: ERROR (CLISAGE_API_KEY is invalid; unset it or replace it)"
            )?;
            healthy = false;
        }
    }
    writeln!(output, "Backend: {backend:?}")?;
    if backend == Backend::Pass {
        for program in ["pass", "gpg"] {
            let available = Command::new(program)
                .arg("--version")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|status| status.success());
            writeln!(
                output,
                "{program}: {}",
                if available {
                    "OK"
                } else {
                    "unavailable; install pass/GPG"
                }
            )?;
            healthy &= available;
        }
        let initialized = pass_store_path().and_then(|path| {
            clisage::check_pass_store(
                &path,
                std::env::var_os("PASSWORD_STORE_KEY").is_some_and(|key| !key.is_empty()),
            )
        });
        match initialized {
            Ok(()) => writeln!(
                output,
                "Pass recipients: OK (encryption keys and pinentry not tested)"
            )?,
            Err(error) => {
                writeln!(output, "Pass recipients: ERROR ({error})")?;
                healthy = false;
            }
        }
    } else {
        match check_backend(backend) {
            Err(error) => {
                writeln!(output, "Native store: ERROR ({error})")?;
                healthy = false;
            }
            Ok(()) => {
                if backend == Backend::SecretService
                    && std::env::var_os("DBUS_SESSION_BUS_ADDRESS")
                        .is_none_or(|value| value.is_empty())
                {
                    writeln!(
                        output,
                        "Secret Service: session bus unavailable; start a desktop session and unlock your keyring"
                    )?;
                    healthy = false;
                } else {
                    writeln!(
                        output,
                        "Native store: platform supported; availability and unlock state not tested"
                    )?;
                }
            }
        }
    }
    let shell = std::env::var("CLISAGE_SHELL")
        .or_else(|_| std::env::var("SHELL"))
        .unwrap_or_default();
    let shell = match shell.rsplit('/').next() {
        Some("bash") => "bash",
        Some("zsh") => "zsh",
        _ => "unknown (portable POSIX suggestions; set SHELL or CLISAGE_SHELL)",
    };
    writeln!(output, "Shell: {shell}")?;
    writeln!(
        output,
        "Integration: opt in with clisage integration bash or zsh; bindings and terminal Meta settings are not tested"
    )?;
    if healthy {
        Ok(())
    } else {
        Err("setup checks found problems; see doctor output above".into())
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    if cli.editor
        && matches!(
            cli.command,
            Some(
                Action::Auth { .. }
                    | Action::Config { .. }
                    | Action::Integration { .. }
                    | Action::Doctor
                    | Action::Completions { .. }
            )
        )
    {
        return Err("--editor is only available for prompts, shell, and explain".into());
    }
    if let Some(Action::Integration { shell }) = &cli.command {
        write!(
            io::stdout().lock(),
            "{}",
            match shell {
                IntegrationShell::Zsh => include_str!("../shell/clisage.zsh"),
                IntegrationShell::Bash => include_str!("../shell/clisage.bash"),
            }
        )?;
        return Ok(());
    }
    match cli.command {
        Some(Action::Completions { shell }) => {
            // Generators can panic on write failures; generate in memory first.
            let mut buffer = Vec::new();
            clap_complete::generate(shell, &mut Cli::command(), "clisage", &mut buffer);
            io::stdout().lock().write_all(&buffer)?;
            return Ok(());
        }
        Some(Action::Doctor) => return doctor(),
        Some(Action::Auth { action, backend }) => {
            let backend = match backend {
                Some(backend) => backend,
                None => match load_config() {
                    Ok(config) => config.credential_backend,
                    Err(_) => {
                        eprintln!(
                            "Configuration unavailable; using pass. Use auth --backend to select another store."
                        );
                        Backend::Pass
                    }
                },
            };
            return auth(action, backend);
        }
        _ => {}
    }
    let mut config = load_config()?;
    if let Some(model) = cli.model {
        config.model = model;
    }
    if config.model.trim().is_empty() {
        return Err("model must not be empty".into());
    }
    let (mode, mut prompt, command_only) = match cli.command {
        Some(Action::Config { .. }) => {
            write!(io::stdout().lock(), "{}", toml::to_string_pretty(&config)?)?;
            return Ok(());
        }
        Some(Action::Auth { .. } | Action::Doctor | Action::Completions { .. }) => {
            unreachable!("utility handled before configuration")
        }
        Some(Action::Shell {
            prompt,
            command_only,
        }) => (
            if command_only {
                Mode::ShellCommand
            } else {
                Mode::Shell
            },
            prompt.unwrap_or_default(),
            command_only,
        ),
        Some(Action::Explain { prompt }) => (Mode::Explain, prompt.unwrap_or_default(), false),
        Some(Action::Integration { .. }) => {
            unreachable!("integration handled before configuration")
        }
        None => (Mode::Prompt, cli.prompt.unwrap_or_default(), false),
    };
    if cli.editor {
        if !io::stdin().is_terminal() || !io::stderr().is_terminal() {
            return Err(
                "--editor requires a terminal and cannot be combined with piped input".into(),
            );
        }
        let editor = clisage::resolve_editor(
            std::env::var("EDITOR").ok().as_deref(),
            std::env::var("VISUAL").ok().as_deref(),
        );
        prompt = clisage::edit_prompt(&editor, &prompt)?;
    }
    let mut input = String::new();
    if !io::stdin().is_terminal() {
        io::stdin()
            .take(1_048_577)
            .read_to_string(&mut input)
            .map_err(|_| "stdin must contain UTF-8 text")?;
        if input.len() > 1_048_576 {
            return Err("piped input exceeds 1 MiB; narrow the input".into());
        }
    }
    let messages = clisage::build_messages(mode, &prompt, &input)?;
    let key = match env_key()? {
        Some(key) => key,
        None => match read_credential(config.credential_backend) {
            Ok(key) => key,
            Err(error) if io::stdin().is_terminal() && io::stderr().is_terminal() => {
                eprintln!("{error}");
                login(config.credential_backend)?
            }
            Err(error) => return Err(error),
        },
    };
    // pass/GPG can open a tty pinentry even with stdin redirected. Keep all
    // credential interaction free of background terminal output.
    let progress_enabled = io::stderr().is_terminal()
        && (command_only || io::stdout().is_terminal())
        && !std::env::var("TERM").is_ok_and(|term| term == "dumb");
    let progress_icon = std::env::var("CLISAGE_THINKING_ICON").ok();
    let mut thinking =
        clisage::ThinkingIndicator::start(progress_enabled, progress_icon.as_deref(), io::stderr());
    let client = reqwest::blocking::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|_| "unable to initialize HTTPS client")?;
    let render = clisage::should_render_markdown(
        io::stdout().is_terminal(),
        cli.markdown,
        cli.no_markdown,
        std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty()),
        std::env::var("TERM").is_ok_and(|v| v == "dumb"),
        command_only,
    );
    let mut output = io::stdout().lock();
    // Shell widgets must receive either a complete validated command or nothing.
    let mut response_buffer = Vec::new();
    let mut terminal_output =
        clisage::TerminalOutput::with_policy(&mut output, io::stdout().is_terminal());
    let response_output: &mut dyn Write = if command_only || render {
        &mut response_buffer
    } else {
        &mut terminal_output
    };
    let mut response_output =
        clisage::ThinkingOutput::new(response_output, &mut thinking, !(command_only || render));
    let result = clisage::request(
        &client,
        clisage::ENDPOINT,
        key.expose_secret(),
        &config,
        messages,
        &mut response_output,
    );
    thinking.stop();
    result?;
    if command_only {
        let text = std::str::from_utf8(&response_buffer).map_err(|_| "invalid command encoding")?;
        output.write_all(clisage::command_suggestion(text)?.as_bytes())?;
    } else if render {
        let text =
            std::str::from_utf8(&response_buffer).map_err(|_| "invalid response encoding")?;
        output.write_all(
            clisage::render_markdown(text)
                .trim_end_matches('\n')
                .as_bytes(),
        )?;
    }
    writeln!(output)?;
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        if error
            .downcast_ref::<io::Error>()
            .is_some_and(|e| e.kind() == io::ErrorKind::BrokenPipe)
        {
            return;
        }
        eprintln!("clisage: {error}");
        std::process::exit(1);
    }
}
