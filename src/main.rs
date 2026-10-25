use clap::{Parser, Subcommand, ValueEnum};
use clisage::{CREDENTIAL_ID, Config, Mode, Result};
use secrecy::{ExposeSecret, SecretString};
use std::{
    io::{self, IsTerminal, Read, Write},
    path::PathBuf,
    process::{Command, Stdio},
    time::Duration,
};

#[derive(Parser)]
#[command(
    version,
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
}
#[derive(Subcommand)]
enum Action {
    /// Suggest a shell command without executing it.
    Shell {
        prompt: String,
        /// Return one plain command after completion, for shell integration.
        #[arg(long)]
        command_only: bool,
    },
    /// Explain a shell command without executing it.
    Explain { prompt: String },
    /// Manage encrypted credentials.
    Auth {
        #[command(subcommand)]
        action: Auth,
    },
    /// Inspect configuration without secrets.
    Config {
        #[command(subcommand)]
        action: ConfigAction,
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
        Ok(text) => toml::from_str(&text).map_err(|_| "invalid config: expected version 1, model, and credential_backend = 'pass'; no secret or endpoint fields allowed".into()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Config::default()),
        Err(_) => Err("unable to read configuration".into()),
    }
}
fn validate_key(key: String) -> Result<SecretString> {
    if key.trim().is_empty() || key.chars().any(char::is_control) {
        return Err("credential is empty or invalid; run clisage auth login".into());
    }
    Ok(SecretString::from(key))
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
fn login() -> Result<SecretString> {
    if !io::stdin().is_terminal() || !io::stderr().is_terminal() {
        return Err("login requires an interactive terminal; use CLISAGE_API_KEY through your automation secret manager".into());
    }
    let store = std::env::var_os("PASSWORD_STORE_DIR")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".password-store"))
        })
        .ok_or("set HOME or PASSWORD_STORE_DIR to locate the pass store")?;
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
fn run() -> Result<()> {
    let cli = Cli::parse();
    if let Some(Action::Integration { shell }) = &cli.command {
        print!(
            "{}",
            match shell {
                IntegrationShell::Zsh => include_str!("../shell/clisage.zsh"),
                IntegrationShell::Bash => include_str!("../shell/clisage.bash"),
            }
        );
        return Ok(());
    }
    let mut config = load_config()?;
    if let Some(model) = cli.model {
        config.model = model;
    }
    if config.model.trim().is_empty() {
        return Err("model must not be empty".into());
    }
    let (mode, prompt, command_only) = match cli.command {
        Some(Action::Config { .. }) => {
            print!("{}", toml::to_string_pretty(&config)?);
            return Ok(());
        }
        Some(Action::Auth { action }) => {
            match action {
                Auth::Login => {
                    login()?;
                }
                Auth::Status => {
                    if env_key()?.is_some() {
                        println!(
                            "Credential available from environment (not validated with provider)."
                        );
                    } else {
                        pass_read()?;
                        println!("Credential available from pass (not validated with provider).");
                    }
                }
                Auth::Logout => {
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
                    println!(
                        "Stored credential removed. Unset CLISAGE_API_KEY separately if configured."
                    );
                }
            }
            return Ok(());
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
            prompt,
            command_only,
        ),
        Some(Action::Explain { prompt }) => (Mode::Explain, prompt, false),
        Some(Action::Integration { .. }) => {
            unreachable!("integration handled before configuration")
        }
        None => (Mode::Prompt, cli.prompt.unwrap_or_default(), false),
    };
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
        None => match pass_read() {
            Ok(key) => key,
            Err(error) if io::stdin().is_terminal() && io::stderr().is_terminal() => {
                eprintln!("{error}");
                login()?
            }
            Err(error) => return Err(error),
        },
    };
    let client = reqwest::blocking::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|_| "unable to initialize HTTPS client")?;
    let mut output = io::stdout().lock();
    // Shell widgets must receive either a complete validated command or nothing.
    let mut command_buffer = Vec::new();
    let response_output: &mut dyn Write = if command_only {
        &mut command_buffer
    } else {
        &mut output
    };
    clisage::request(
        &client,
        clisage::ENDPOINT,
        key.expose_secret(),
        &config,
        messages,
        response_output,
    )?;
    if command_only {
        let text = std::str::from_utf8(&command_buffer).map_err(|_| "invalid command encoding")?;
        output.write_all(clisage::command_suggestion(text)?.as_bytes())?;
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
