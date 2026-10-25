/// Minimal platform context; never collects usernames, paths, history, or files
/// other than the standard Linux distribution metadata.
pub struct RuntimeContext {
    os: String,
    shell: String,
}

impl RuntimeContext {
    pub fn new(os: &str, os_release: &str, shell: &str) -> Self {
        let distribution = if os.eq_ignore_ascii_case("linux") {
            os_release.lines().find_map(|line| {
                line.strip_prefix("PRETTY_NAME=")
                    .map(|name| name.trim().trim_matches(['\'', '"']))
            })
        } else {
            None
        };
        // Invalid shell names are discarded rather than letting environment
        // values introduce extra instructions or local paths into the prompt.
        let shell = shell.rsplit('/').next().unwrap_or_default();
        let shell = if !shell.is_empty()
            && shell.len() <= 64
            && shell
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
        {
            shell
        } else {
            "unknown"
        };
        Self {
            os: distribution
                .filter(|name| !name.is_empty())
                .unwrap_or(os)
                .chars()
                .filter(|c| !c.is_control())
                .take(128)
                .collect(),
            shell: shell.into(),
        }
    }

    pub fn detect() -> Self {
        let os_release = if cfg!(target_os = "linux") {
            std::fs::read_to_string("/etc/os-release").unwrap_or_default()
        } else {
            String::new()
        };
        // Widgets know their actual shell, which can differ from the login shell.
        let shell = std::env::var("CLISAGE_SHELL")
            .or_else(|_| std::env::var("SHELL"))
            .unwrap_or_default();
        Self::new(std::env::consts::OS, &os_release, &shell)
    }

    pub(crate) fn instruction(&self) -> String {
        format!(
            "Platform context (data): OS: {}; Shell: {}. Use commands and syntax appropriate for this platform. If the shell is unknown, prefer portable POSIX syntax.",
            self.os, self.shell
        )
    }
}
