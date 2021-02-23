use std::path::PathBuf;
use std::str::FromStr;

/// Represents the available options that can be configured.
#[derive(Debug, Deserialize)]
pub struct Options {
    /// The path to the SSH private key to use for authentication
    pub ssh_private_key: PathBuf,
    /// The path that contains the repositories
    pub repo_root: PathBuf,
    /// The path to find `cargo` at
    pub cargo_path: PathBuf,
}

/// Represents the structure of the configuration file.
#[derive(Debug, Deserialize)]
pub struct Config {
    pub default: Options,
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

    #[test]
    fn config_can_be_parsed_from_a_string() {
        let content = r#"
default:
    ssh_private_key: "/root/.ssh/id_rsa"
    repo_root: "/root"
    cargo_path: "/root/.cargo/bin/cargo"
"#;

        let config = Config::from_str(content).unwrap();

        assert_eq!(
            config.default.ssh_private_key,
            PathBuf::from("/root/.ssh/id_rsa")
        );

        assert_eq!(config.default.repo_root, PathBuf::from("/root"));

        assert_eq!(
            config.default.cargo_path,
            PathBuf::from("/root/.cargo/bin/cargo")
        );
    }
}
