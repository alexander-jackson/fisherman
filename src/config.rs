use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::{bail, Result};
use serenity::http::client::Http;
use serenity::model::id::ChannelId;

/// Represents any commands that should be run by the shell.
#[derive(Debug, Deserialize)]
pub struct Commands(Vec<Command>);

impl Commands {
    pub async fn execute(&self, repo_path: &Path) -> Result<()> {
        for command in &self.0 {
            let working_dir = repo_path.join(command.working_dir.clone().unwrap_or_default());

            tracing::info!(?command, ?working_dir, "Executing a user specified command");

            let mut to_execute = tokio::process::Command::new(&command.program);

            if let Some(args) = command.args.as_ref() {
                to_execute.args(args);
            }

            let status = to_execute.current_dir(&working_dir).spawn()?.wait().await?;

            if !status.success() {
                bail!("Failed to execute command: {:?}", command);
            }
        }

        Ok(())
    }
}

/// Represents the configuration for Discord notifications
#[derive(Debug, Deserialize)]
pub struct DiscordConfig {
    /// The bot token to use for messages
    pub token: String,
    /// The channel identifier to send messages to
    pub channel_id: u64,
}

/// Represents the available options that can be configured.
#[derive(Debug, Deserialize)]
pub struct Options {
    /// The port to listen for messages on, defaulting to 5000 if not specified
    pub port: Option<u16>,
    /// The path to the SSH private key to use for authentication
    pub ssh_private_key: PathBuf,
    /// The path that contains the repositories
    pub repo_root: PathBuf,
    /// The path to find `cargo` at
    pub cargo_path: PathBuf,
    /// The secret to use for validating payloads
    pub secret: Option<String>,
    /// The configuration to use for Discord notifications
    pub discord: Option<DiscordConfig>,
}

/// Components of a command to be run after restarting binaries.
#[derive(Debug, Deserialize)]
pub struct Command {
    /// The program name
    pub program: String,
    /// The arguments to the program, if there are any
    pub args: Option<Vec<String>>,
    /// The working directory for the command, relative to the base of the repository
    pub working_dir: Option<PathBuf>,
}

/// Repository specific options such as having multiple binaries
#[derive(Debug, Deserialize)]
pub struct SpecificOptions {
    /// The top-level directory where `cargo build --bin <name>` can be run
    pub code_root: Option<PathBuf>,
    /// The names of the binaries
    pub binaries: Option<Vec<String>>,
    /// The secret to use for validating payloads
    pub secret: Option<String>,
    /// The branch to follow for this repository
    pub follow: Option<String>,
    /// The commands to execute before processing
    pub precommands: Option<Commands>,
    /// Whether to build binaries with `cargo`.
    pub should_build_binaries: Option<bool>,
    /// The commands to execute at the end of processing
    pub commands: Option<Commands>,
}

impl SpecificOptions {
    /// Checks whether there are any likely mistakes in the config.
    pub fn check_for_potential_mistakes(&self, key: &str) {
        if matches!(self.code_root.as_ref(), Some(path) if path.is_absolute()) {
            tracing::warn!(?self.code_root, %key, "`code_root` values should be relative, encountered an absolute one");
        }
    }
}

/// Represents the structure of the configuration file.
#[derive(Debug, Deserialize)]
pub struct Config {
    pub default: Options,
    pub specific: Option<HashMap<String, SpecificOptions>>,
}

impl Config {
    /// Gets a specific configuration for a repository if it exists.
    fn get_specific_config(&self, repository: &str) -> Option<&SpecificOptions> {
        self.specific.as_ref().and_then(|s| s.get(repository))
    }

    /// Checks whether there are any likely mistakes in the config.
    pub fn check_for_potential_mistakes(&self) {
        let default = &self.default;

        // Check the key, root and Cargo binary exist
        if !default.ssh_private_key.is_file() {
            tracing::warn!(?default.ssh_private_key, "`ssh_private_key` either does not exist or is not a file");
        }

        if !default.repo_root.is_dir() {
            tracing::warn!(?default.repo_root, "`repo_root` either does not exist or is not a directory");
        }

        if !default.cargo_path.is_file() {
            tracing::warn!(?default.cargo_path, "`cargo_path` either does not exist or is not a file");
        }

        if let Some(specific) = self.specific.as_ref() {
            for (key, options) in specific {
                options.check_for_potential_mistakes(key);
            }
        }
    }

    /// Creates a new client and gets the channel identifier from the config, if it exists.
    pub fn get_client_and_channel_id(&self) -> Option<(Http, ChannelId)> {
        let discord = self.default.discord.as_ref()?;

        // Create a new instance of the client
        let client = Http::new(&discord.token);
        let channel_id = ChannelId(discord.channel_id);

        Some((client, channel_id))
    }

    /// Checks whether this repository should be built with `cargo`.
    pub fn should_build_binaries(&self, repository: &str) -> bool {
        self.get_specific_config(repository)
            .and_then(|s| s.should_build_binaries)
            .unwrap_or(true)
    }

    /// Resolves the value of the `code_root` directive.
    ///
    /// If a specific value exists for the given repository, that will be used, otherwise the root
    /// directory of the project will be used, as denoted by an empty [`PathBuf`].
    pub fn resolve_code_root(&self, repository: &str) -> PathBuf {
        self.get_specific_config(repository)
            .and_then(|s| s.code_root.clone())
            .unwrap_or_default()
    }

    /// Resolves the value of the `binaries` directive.
    ///
    /// If a specific value exists for the given repository, that will be used, otherwise the name
    /// of the repository itself will be used.
    pub fn resolve_binaries(&self, repository: &str) -> Vec<String> {
        self.get_specific_config(repository)
            .and_then(|s| s.binaries.clone())
            .unwrap_or_else(|| vec![String::from(repository.split('/').nth(1).unwrap())])
    }

    /// Resolves the value of the `secret` directive.
    ///
    /// If a specific value exists for the given repository, that will be used, otherwise no secret
    /// will be used (as webhooks do not have to have this).
    pub fn resolve_secret(&self, repository: &str) -> Option<&str> {
        self.get_specific_config(repository)
            .and_then(|s| s.secret.as_deref())
            .or(self.default.secret.as_deref())
    }

    /// Resolves the value of the `follow` directive.
    ///
    /// If a specific value exists for the given repository, that will be used, otherwise the
    /// `master` branch will be used.
    pub fn resolve_follow_branch(&self, repository: &str) -> &str {
        let specific = self
            .get_specific_config(repository)
            .and_then(|s| s.follow.as_deref());

        specific.unwrap_or("master")
    }

    /// Resolves the value of the `precommands` directive.
    ///
    /// If a specific value exists, it will be returned, otherwise nothing will be returned.
    pub fn resolve_precommands(&self, repository: &str) -> Option<&Commands> {
        self.get_specific_config(repository)
            .and_then(|s| s.precommands.as_ref())
    }

    /// Resolves the value of the `commands` directive.
    ///
    /// If a specific value exists, it will be returned, otherwise nothing will be returned.
    pub fn resolve_commands(&self, repository: &str) -> Option<&Commands> {
        self.get_specific_config(repository)
            .and_then(|s| s.commands.as_ref())
    }
}

impl FromStr for Config {
    type Err = serde_yaml::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        serde_yaml::from_str(s)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::str::FromStr;

    use crate::config::Config;

    static CONFIG: &str = r#"
default:
    ssh_private_key: "/root/.ssh/id_rsa"
    repo_root: "/root"
    cargo_path: "/root/.cargo/bin/cargo"

specific:
    FreddieBrown/dodona:
        follow: "develop"
        code_root: "/backend"
        binaries: ["api-server", "dcl"]

    alexander-jackson/locker:
        binaries: ["locker", "zipper"]

    alexander-jackson/ptc:
        code_root: "/ptc"

    alexander-jackson/se-powerlifting-website:
        should_build_binaries: false
"#;

    #[test]
    fn config_can_be_parsed_from_a_string() {
        let config = Config::from_str(CONFIG).unwrap();

        assert_eq!(
            config.default.ssh_private_key,
            PathBuf::from("/root/.ssh/id_rsa")
        );

        assert_eq!(config.default.repo_root, PathBuf::from("/root"));

        assert_eq!(
            config.default.cargo_path,
            PathBuf::from("/root/.cargo/bin/cargo")
        );

        assert!(config.specific.is_some());
    }

    #[test]
    fn repository_specific_settings_can_be_fetched() {
        let config = Config::from_str(CONFIG).unwrap();
        assert!(config.get_specific_config("FreddieBrown/dodona").is_some());
    }

    #[test]
    fn no_specific_settings_exist_if_not_defined() {
        let config = Config::from_str(CONFIG).unwrap();
        assert!(config
            .get_specific_config("FreddieBrown/not-dodona")
            .is_none());
    }

    #[test]
    fn code_root_can_be_fetched_if_it_exists() {
        let config = Config::from_str(CONFIG).unwrap();
        let code_root = config.resolve_code_root("FreddieBrown/dodona");

        assert_eq!(code_root, PathBuf::from("/backend"));
    }

    #[test]
    fn default_code_root_is_the_repository_root() {
        let config = Config::from_str(CONFIG).unwrap();
        let code_root = config.resolve_code_root("alexander-jackson/locker");

        assert_eq!(code_root, PathBuf::new());
    }

    #[test]
    fn binaries_resolve_correctly() {
        let config = Config::from_str(CONFIG).unwrap();
        let binaries = config.resolve_binaries("FreddieBrown/dodona");

        assert_eq!(binaries, vec!["api-server", "dcl"]);
    }

    #[test]
    fn binaries_are_assumed_to_be_repository_name_if_not_specified() {
        let config = Config::from_str(CONFIG).unwrap();
        let binaries = config.resolve_binaries("alexander-jackson/ptc");

        assert_eq!(binaries, vec!["ptc"]);
    }

    #[test]
    fn config_with_no_secret_assumes_no_security() {
        let config = Config::from_str(CONFIG).unwrap();
        let secret = config.resolve_secret("alexander-jackson/ptc");

        assert!(secret.is_none());
    }

    #[test]
    fn no_specific_secret_assumes_global_value() {
        let config = r#"
        default:
            ssh_private_key: "/root/.ssh/id_rsa"
            repo_root: "/root"
            cargo_path: "/root/.cargo/bin/cargo"
            secret: "<some secret value>"
        "#;

        let config = Config::from_str(config).unwrap();
        let secret = config.resolve_secret("alexander-jackson/ptc");

        assert_eq!(secret, Some("<some secret value>"));
    }

    #[test]
    fn specific_secrets_are_used_if_they_exist() {
        let config = r#"
        default:
            ssh_private_key: "/root/.ssh/id_rsa"
            repo_root: "/root"
            cargo_path: "/root/.cargo/bin/cargo"

        specific:
            alexander-jackson/ptc:
                secret: "<repository specific>"
        "#;

        let config = Config::from_str(config).unwrap();
        let secret = config.resolve_secret("alexander-jackson/ptc");

        assert_eq!(secret, Some("<repository specific>"));
    }

    #[test]
    fn master_is_followed_if_unspecified() {
        let config = Config::from_str(CONFIG).unwrap();
        let follow_branch = config.resolve_follow_branch("alexander-jackson/ptc");

        assert_eq!(follow_branch, "master");
    }

    #[test]
    fn specific_branches_can_be_followed() {
        let config = Config::from_str(CONFIG).unwrap();
        let follow_branch = config.resolve_follow_branch("FreddieBrown/dodona");

        assert_eq!(follow_branch, "develop");
    }

    #[test]
    fn binaries_are_built_if_not_specified() {
        let config = Config::from_str(CONFIG).unwrap();
        let should_build_binaries = config.should_build_binaries("FreddieBrown/dodona");

        assert!(should_build_binaries);
    }

    #[test]
    fn binaries_are_not_built_based_on_config() {
        let config = Config::from_str(CONFIG).unwrap();
        let should_build_binaries =
            config.should_build_binaries("alexander-jackson/se-powerlifting-website");

        assert!(!should_build_binaries);
    }
}
