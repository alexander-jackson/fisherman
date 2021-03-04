use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;

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
}

/// Represents the structure of the configuration file.
#[derive(Debug, Deserialize)]
pub struct Config {
    pub default: Options,
    pub specific: Option<HashMap<String, SpecificOptions>>,
}

impl Config {
    fn get_specific_config(&self, repository: &str) -> Option<&SpecificOptions> {
        self.specific.as_ref().and_then(|s| s.get(repository))
    }

    pub fn resolve_code_root(&self, repository: &str) -> PathBuf {
        self.get_specific_config(repository)
            .and_then(|s| s.code_root.clone())
            .unwrap_or_default()
    }

    pub fn resolve_binaries(&self, repository: &str) -> Vec<String> {
        self.get_specific_config(repository)
            .and_then(|s| s.binaries.clone())
            .unwrap_or_else(|| vec![String::from(repository.split('/').nth(1).unwrap())])
    }

    pub fn resolve_secret(&self, repository: &str) -> Option<&str> {
        self.get_specific_config(repository)
            .and_then(|s| s.secret.as_deref())
            .or_else(|| self.default.secret.as_deref())
    }

    pub fn resolve_follow_branch(&self, repository: &str) -> &str {
        let specific = self
            .get_specific_config(repository)
            .and_then(|s| s.follow.as_deref());

        specific.unwrap_or("master")
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
}
