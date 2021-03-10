use std::process::Command;
use std::sync::Arc;

use actix_web::HttpResponse;

use crate::config::Config;
use crate::events::{Event, EventVariant, TimeseriesQueue};
use crate::git;

#[derive(Debug, Deserialize)]
pub struct Push {
    #[serde(rename = "ref")]
    refname: String,
    before: String,
    after: String,
    repository: Repository,
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
    fn trigger_pull(
        &self,
        config: &Arc<Config>,
        events: &TimeseriesQueue,
    ) -> Result<(), git2::Error> {
        let path = config.default.repo_root.join(&self.repository.name);
        let repo = git2::Repository::open(&path)?;
        let branch = config.resolve_follow_branch(&self.repository.full_name);

        log::info!(
            "Fetching changes for the project at `{:?}` on branch `{}`",
            path,
            branch
        );

        events.push(EventVariant::Pull.into());

        let mut remote = repo.find_remote("origin")?;

        let fetch_commit = git::fetch(
            &repo,
            &[branch],
            &mut remote,
            &config.default.ssh_private_key,
        )?;

        git::merge(&repo, branch, &fetch_commit)
    }

    /// Triggers the recompilation of a repository associated with the webhook.
    ///
    /// This should be run after pulling the new changes to update the repository. After being
    /// rebuilt, it can be restarted in `supervisor` and the new changes will go live.
    fn trigger_build(&self, config: &Arc<Config>, events: &TimeseriesQueue) -> std::io::Result<()> {
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

            events.push(Event::with_message(EventVariant::Build, binary.clone()));

            Command::new(config.default.cargo_path.clone())
                .args(&["build", "--release", "--bin", &binary])
                .current_dir(&path)
                .spawn()?
                .wait()?;
        }

        Ok(())
    }

    /// Triggers a process restart by `supervisor`.
    ///
    /// Restarts the process within `supervisor`, allowing a new version to supersede the existing
    /// version.
    fn trigger_restart(
        &self,
        config: &Arc<Config>,
        events: &TimeseriesQueue,
    ) -> std::io::Result<()> {
        let binaries = config.resolve_binaries(&self.repository.full_name);

        for binary in binaries {
            log::info!("Allowing `supervisor` to restart: {}", binary);

            events.push(Event::with_message(EventVariant::Restart, binary.clone()));

            Command::new("supervisorctl")
                .args(&["restart", &binary])
                .spawn()?
                .wait()?;
        }

        Ok(())
    }

    /// Runs any additional commands specified in the config.
    ///
    /// Commands will be run in the `code_root` directory and will simply be executed by the shell.
    fn run_additional_commands(&self, config: &Arc<Config>) -> std::io::Result<()> {
        if let Some(commands) = config.resolve_commands(&self.repository.full_name) {
            let repo_path = config.default.repo_root.join(&self.repository.name);

            for command in commands {
                let working_dir = repo_path.join(command.working_dir.clone().unwrap_or_default());

                log::info!("Executing: {:?} at {:?}", command, working_dir);

                let mut to_execute = Command::new(&command.program);

                if let Some(args) = command.args.as_ref() {
                    to_execute.args(args);
                }

                to_execute.current_dir(&working_dir).spawn()?.wait()?;
            }
        }

        Ok(())
    }

    /// Retrieves the full name of the repository this webhook relates to.
    pub fn get_full_name(&self) -> &str {
        &self.repository.full_name
    }

    pub fn handle(&self, config: &Arc<Config>, events: &TimeseriesQueue) -> HttpResponse {
        // Get the branch that this repository follows
        let follow_branch = config.resolve_follow_branch(self.get_full_name());

        if self.changes_follow_branch(&follow_branch) {
            log::info!("Commits were pushed to `{}` in this event", follow_branch);

            // Pull the new changes
            self.trigger_pull(config, events)
                .expect("Failed to execute the pull.");

            // Build the updated binary
            self.trigger_build(config, events)
                .expect("Failed to rebuild the binary");

            // Restart in `supervisor`
            self.trigger_restart(config, events)
                .expect("Failed to restart the process");

            // Run any additional commands
            self.run_additional_commands(config)
                .expect("Failed to run additional commands");
        }

        HttpResponse::Ok().finish()
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

    pub fn handle(&self, _config: &Arc<Config>, events: &TimeseriesQueue) -> HttpResponse {
        let body = format!(
            "Setup tracking of `{}` at url: {}",
            self.repository.full_name, self.hook.config.url
        );

        events.push(EventVariant::Ping.into());

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
