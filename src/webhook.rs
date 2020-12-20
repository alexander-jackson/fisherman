use std::path::Path;
use std::process::Command;

use crate::git;

#[derive(Debug, Deserialize)]
pub struct Webhook {
    #[serde(rename = "ref")]
    refname: String,
    before: String,
    after: String,
    repository: Repository,
}

impl Webhook {
    /// Checks whether the push request is to the master branch of a repository.
    pub fn is_master_push(&self) -> bool {
        let master = &self.repository.master_branch;
        let formatted = format!("refs/heads/{}", master);

        formatted == self.refname
    }

    /// Triggers a `git pull` for the repository associated with the webhook.
    ///
    /// This will open the repository, which is assumed to be at `/root/<name>` and fetch the
    /// contents of its master branch (which can be `master`, `main` or whatever the default is set
    /// to). It will then merge the contents of the fetch.
    pub fn trigger_pull(&self) -> Result<(), git2::Error> {
        let path = Path::new("/root").join(&self.repository.name);
        let repo = git2::Repository::open(&path)?;
        let master_branch = &self.repository.master_branch;

        log::info!("Fetching changes for the project at: {:?}", path);

        let mut remote = repo.find_remote("origin")?;
        let fetch_commit = git::fetch(&repo, &[master_branch], &mut remote)?;
        git::merge(&repo, master_branch, fetch_commit)
    }

    /// Triggers the recompilation of a repository associated with the webhook.
    ///
    /// This should be run after pulling the new changes to update the repository. After being
    /// rebuilt, it can be restarted in `supervisor` and the new changes will go live.
    pub fn trigger_build(&self) -> std::io::Result<()> {
        let path = Path::new("/root").join(&self.repository.name);

        log::info!("Building a release binary for the project at: {:?}", path);

        Command::new("cargo")
            .args(&["build", "--release"])
            .current_dir(path)
            .spawn()?
            .wait()?;

        Ok(())
    }

    /// Triggers a process restart by `supervisor`.
    ///
    /// Restarts the process within `supervisor`, allowing a new version to supersede the existing
    /// version.
    pub fn trigger_restart(&self) -> std::io::Result<()> {
        let binary_name = &self.repository.name;

        log::info!("Allowing `supervisor` to restart: {}", binary_name);

        Command::new("supervisorctl")
            .args(&["restart", binary_name])
            .spawn()?
            .wait()?;

        Ok(())
    }
}

#[derive(Debug, Deserialize)]
pub struct Repository {
    id: u32,
    name: String,
    full_name: String,
    master_branch: String,
}
