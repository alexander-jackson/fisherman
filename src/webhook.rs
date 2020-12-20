use std::io::{self, Write};
use std::path::Path;

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
        let repo = git2::Repository::open(path)?;
        let master_branch = &self.repository.master_branch;

        let mut remote = repo.find_remote("origin")?;
        let fetch_commit = do_fetch(&repo, &[master_branch], &mut remote)?;
        do_merge(&repo, master_branch, fetch_commit)
    }
}

fn do_fetch<'a>(
    repo: &'a git2::Repository,
    refs: &[&str],
    remote: &'a mut git2::Remote,
) -> Result<git2::AnnotatedCommit<'a>, git2::Error> {
    let mut cb = git2::RemoteCallbacks::new();

    // Print out our transfer progress.
    cb.transfer_progress(|stats| {
        if stats.received_objects() == stats.total_objects() {
            print!(
                "Resolving deltas {}/{}\r",
                stats.indexed_deltas(),
                stats.total_deltas()
            );
        } else if stats.total_objects() > 0 {
            print!(
                "Received {}/{} objects ({}) in {} bytes\r",
                stats.received_objects(),
                stats.total_objects(),
                stats.indexed_objects(),
                stats.received_bytes()
            );
        }

        io::stdout().flush().is_ok()
    });

    // Use SSH credentials for authentication
    cb.credentials(|_url, username_from_url, _allowed_types| {
        git2::Cred::ssh_key(
            username_from_url.unwrap(),
            None,
            std::path::Path::new(&format!("{}/.ssh/id_rsa", std::env::var("HOME").unwrap())),
            None,
        )
    });

    let mut fo = git2::FetchOptions::new();
    fo.remote_callbacks(cb);
    fo.download_tags(git2::AutotagOption::All);

    println!("Fetching {} for repo", remote.name().unwrap());
    remote.fetch(refs, Some(&mut fo), None)?;

    // If there are local objects (we got a thin pack), then tell the user
    // how many objects we saved from having to cross the network.
    let stats = remote.stats();

    if stats.local_objects() > 0 {
        println!(
            "\rReceived {}/{} objects in {} bytes (used {} local \
             objects)",
            stats.indexed_objects(),
            stats.total_objects(),
            stats.received_bytes(),
            stats.local_objects()
        );
    } else {
        println!(
            "\rReceived {}/{} objects in {} bytes",
            stats.indexed_objects(),
            stats.total_objects(),
            stats.received_bytes()
        );
    }

    let fetch_head = repo.find_reference("FETCH_HEAD")?;
    Ok(repo.reference_to_annotated_commit(&fetch_head)?)
}

fn fast_forward(
    repo: &git2::Repository,
    lb: &mut git2::Reference,
    rc: &git2::AnnotatedCommit,
) -> Result<(), git2::Error> {
    let name = lb.name().expect("Reference was invalid UTF-8");
    let msg = format!("Fast-Forward: Setting {} to id: {}", name, rc.id());
    println!("{}", msg);

    repo.set_head(&name)?;
    lb.set_target(rc.id(), &msg)?;
    repo.checkout_head(Some(git2::build::CheckoutBuilder::default().force()))?;

    Ok(())
}

fn normal_merge(
    repo: &git2::Repository,
    local: &git2::AnnotatedCommit,
    remote: &git2::AnnotatedCommit,
) -> Result<(), git2::Error> {
    let local_tree = repo.find_commit(local.id())?.tree()?;
    let remote_tree = repo.find_commit(remote.id())?.tree()?;

    let ancestor = repo
        .find_commit(repo.merge_base(local.id(), remote.id())?)?
        .tree()?;

    let mut idx = repo.merge_trees(&ancestor, &local_tree, &remote_tree, None)?;

    if idx.has_conflicts() {
        println!("Merge conficts detected...");
        repo.checkout_index(Some(&mut idx), None)?;
        return Ok(());
    }

    let result_tree = repo.find_tree(idx.write_tree_to(repo)?)?;

    // now create the merge commit
    let msg = format!("Merge: {} into {}", remote.id(), local.id());
    let sig = repo.signature()?;
    let local_commit = repo.find_commit(local.id())?;
    let remote_commit = repo.find_commit(remote.id())?;

    // Do our merge commit and set current branch head to that commit.
    let _merge_commit = repo.commit(
        Some("HEAD"),
        &sig,
        &sig,
        &msg,
        &result_tree,
        &[&local_commit, &remote_commit],
    )?;

    // Set working tree to match head.
    repo.checkout_head(None)?;

    Ok(())
}

fn do_merge<'a>(
    repo: &'a git2::Repository,
    remote_branch: &str,
    fetch_commit: git2::AnnotatedCommit<'a>,
) -> Result<(), git2::Error> {
    // 1. do a merge analysis
    let analysis = repo.merge_analysis(&[&fetch_commit])?;

    // 2. Do the appopriate merge
    if analysis.0.is_fast_forward() {
        println!("Doing a fast forward");
        // do a fast forward
        let refname = format!("refs/heads/{}", remote_branch);

        if let Ok(mut r) = repo.find_reference(&refname) {
            fast_forward(repo, &mut r, &fetch_commit)?;
        } else {
            // The branch doesn't exist so just set the reference to the
            // commit directly. Usually this is because you are pulling
            // into an empty repository.
            repo.reference(
                &refname,
                fetch_commit.id(),
                true,
                &format!("Setting {} to {}", remote_branch, fetch_commit.id()),
            )?;
            repo.set_head(&refname)?;
            repo.checkout_head(Some(
                git2::build::CheckoutBuilder::default()
                    .allow_conflicts(true)
                    .conflict_style_merge(true)
                    .force(),
            ))?;
        }
    } else if analysis.0.is_normal() {
        // do a normal merge
        let head_commit = repo.reference_to_annotated_commit(&repo.head()?)?;
        normal_merge(&repo, &head_commit, &fetch_commit)?;
    } else {
        println!("Nothing to do...");
    }

    Ok(())
}

#[derive(Debug, Deserialize)]
pub struct Repository {
    id: u32,
    name: String,
    full_name: String,
    master_branch: String,
}
