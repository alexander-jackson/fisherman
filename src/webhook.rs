use std::sync::Arc;

use actix_web::HttpResponse;
use anyhow::{bail, Result};
use tokio::process::Command;

use crate::config::Config;
use crate::git;

#[derive(Debug, Deserialize)]
pub struct User {
    name: String,
    email: String,
    username: String,
}

#[derive(Debug, Deserialize)]
pub struct Commit {
    id: String,
    message: String,
    author: User,
}

#[derive(Debug, Deserialize)]
pub struct Push {
    #[serde(rename = "ref")]
    refname: String,
    before: String,
    after: String,
    repository: Repository,
    head_commit: Commit,
}

impl Push {
    /// Checks whether the push request is to the followed branch of a repository.
    fn changes_follow_branch(&self, follow: &str) -> bool {
        let formatted = format!("refs/heads/{}", follow);

        formatted == self.refname
    }

    /// Triggers a `git pull` for the repository associated with the webhook.
    ///
    /// This will open the repository, which is assumed to be at `repo_root` and fetch the contents
    /// of its default branch (which can be `master`, `main` or whatever the default is set to). It
    /// will then merge the contents of the fetch.
    fn trigger_pull(&self, config: &Arc<Config>) -> Result<()> {
        let path = config.default.repo_root.join(&self.repository.name);
        let repo = git2::Repository::open(&path)?;
        let branch = config.resolve_follow_branch(&self.repository.full_name);

        log::info!(
            "Fetching changes for the project at `{:?}` on branch `{}`",
            path,
            branch
        );

        let mut remote = repo.find_remote("origin")?;

        let fetch_commit = git::fetch(
            &repo,
            &[branch],
            &mut remote,
            &config.default.ssh_private_key,
        )?;

        Ok(git::merge(&repo, branch, &fetch_commit)?)
    }

    /// Triggers the recompilation of a repository associated with the webhook.
    ///
    /// This should be run after pulling the new changes to update the repository. After being
    /// rebuilt, it can be restarted in `supervisor` and the new changes will go live.
    async fn trigger_build(&self, config: &Arc<Config>) -> Result<()> {
        let code_root = config.resolve_code_root(&self.repository.full_name);
        let binaries = config.resolve_binaries(&self.repository.full_name);

        let path = &config
            .default
            .repo_root
            .join(&self.repository.name)
            .join(&code_root);

        log::info!("Building release binaries with root at: {:?}", path);

        for binary in binaries {
            log::info!("Building the binary called: {}", binary);

            let status = Command::new(config.default.cargo_path.clone())
                .args(&["build", "--release", "--bin", &binary])
                .current_dir(&path)
                .spawn()?
                .wait()
                .await?;

            if !status.success() {
                bail!("Failed to build binary: {}", binary);
            }
        }

        Ok(())
    }

    /// Triggers a process restart by `supervisor`.
    ///
    /// Restarts the process within `supervisor`, allowing a new version to supersede the existing
    /// version.
    async fn trigger_restart(&self, config: &Arc<Config>) -> Result<()> {
        let binaries = config.resolve_binaries(&self.repository.full_name);

        for binary in binaries {
            log::info!("Allowing `supervisor` to restart: {}", binary);

            let status = Command::new("supervisorctl")
                .args(&["restart", &binary])
                .spawn()?
                .wait()
                .await?;

            if !status.success() {
                bail!("Failed to restart binary: {}", binary);
            }
        }

        Ok(())
    }

    /// Runs any additional commands specified in the config.
    ///
    /// Commands will be run in the `code_root` directory and will simply be executed by the shell.
    async fn run_additional_commands(&self, config: &Arc<Config>) -> Result<()> {
        if let Some(commands) = config.resolve_commands(&self.repository.full_name) {
            let repo_path = config.default.repo_root.join(&self.repository.name);

            for command in commands {
                let working_dir = repo_path.join(command.working_dir.clone().unwrap_or_default());

                log::info!("Executing: {:?} at {:?}", command, working_dir);

                let mut to_execute = Command::new(&command.program);

                if let Some(args) = command.args.as_ref() {
                    to_execute.args(args);
                }

                let status = to_execute.current_dir(&working_dir).spawn()?.wait().await?;

                if !status.success() {
                    bail!("Failed to execute command: {:?}", command);
                }
            }
        }

        Ok(())
    }

    /// Notifies a Discord channel of the changes if a configuration exists.
    async fn notify_discord_channel(&self, config: &Arc<Config>) {
        let (client, channel_id) = match config.get_client_and_channel_id() {
            Some((client, channel_id)) => (client, channel_id),
            None => return,
        };

        // Generate the message to send
        let brief = self.head_commit.message.lines().next().unwrap_or_default();

        let repository = &self.repository.full_name;
        let author = &self.head_commit.author.name;
        let commit_id = &self.head_commit.id[..8];

        let message = format!(
            "Production instance of `{}` has been successfully updated to `commit_id={}` (`{}`), authored by {}",
            repository, commit_id, brief, author
        );

        channel_id
            .send_message(&client, |m| m.content(message))
            .await
            .expect("Failed to send the message to the channel");
    }

    /// Notifies a Discord channel of a failure in the handling of a webhook.
    async fn notify_of_failure(&self, config: &Arc<Config>, error: &str) {
        let (client, channel_id) = match config.get_client_and_channel_id() {
            Some((client, channel_id)) => (client, channel_id),
            None => return,
        };

        let message = format!(
            "Production instance of `{}` failed to be updated, error: {}",
            self.repository.full_name, error
        );

        channel_id
            .send_message(&client, |m| m.content(message))
            .await
            .expect("Failed to send the message to the channel");
    }

    /// Handles the webhook message for push messages.
    ///
    /// Checks whether the message updates the followed branch before pulling the changes,
    /// rebuilding all binaries, restarting them and running any additional commands provided in
    /// the configuration. If this all succeeds, informs the Discord channel if this is specified
    /// in the configuration as well.
    async fn handle_inner(
        &self,
        config: &Arc<Config>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
        // Get the branch that this repository follows
        let follow_branch = config.resolve_follow_branch(self.get_full_name());

        if self.changes_follow_branch(&follow_branch) {
            log::info!("Commits were pushed to `{}` in this event", follow_branch);

            // Pull the new changes
            self.trigger_pull(config)?;

            // Build the updated binary
            self.trigger_build(config).await?;

            // Restart in `supervisor`
            self.trigger_restart(config).await?;

            // Run any additional commands
            self.run_additional_commands(config).await?;

            // Everything worked, so update the Discord channel if there is one
            self.notify_discord_channel(config).await;
        }

        Ok(())
    }

    /// Wraps the [`handle_inner`] method by propagating errors correctly.
    pub async fn handle(&self, config: &Arc<Config>) -> HttpResponse {
        match self.handle_inner(config).await {
            Ok(()) => HttpResponse::Ok().finish(),
            Err(e) => {
                let error = e.to_string();
                self.notify_of_failure(config, &error).await;
                HttpResponse::InternalServerError().body(error)
            }
        }
    }

    /// Retrieves the full name of the repository this webhook relates to.
    pub fn get_full_name(&self) -> &str {
        &self.repository.full_name
    }
}

#[derive(Debug, Deserialize)]
pub struct Ping {
    zen: String,
    hook_id: u32,
    hook: Hook,
    repository: Repository,
}

impl Ping {
    pub fn get_full_name(&self) -> &str {
        &self.repository.full_name
    }

    pub async fn handle(&self, _config: &Arc<Config>) -> HttpResponse {
        let body = format!(
            "Setup tracking of `{}` at url: {}",
            self.repository.full_name, self.hook.config.url
        );

        HttpResponse::Ok().body(body)
    }
}

#[derive(Debug, Deserialize)]
pub struct Repository {
    id: u32,
    name: String,
    full_name: String,
    default_branch: String,
}

#[derive(Debug, Deserialize)]
pub struct Hook {
    #[serde(rename = "type")]
    ty: String,
    id: u32,
    name: String,
    active: bool,
    events: Vec<String>,
    config: HookConfig,
}

#[derive(Debug, Deserialize)]
pub struct HookConfig {
    content_type: String,
    url: String,
}
