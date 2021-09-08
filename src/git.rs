use std::path::Path;

/// Fetches the changes for a set of branches from a remote.
pub fn fetch<'a>(
    repo: &'a git2::Repository,
    refs: &[&str],
    remote: &'a mut git2::Remote,
    ssh_private_key_path: &'a Path,
) -> Result<git2::AnnotatedCommit<'a>, git2::Error> {
    let mut cb = git2::RemoteCallbacks::new();

    // Use SSH credentials for authentication
    cb.credentials(|_url, username_from_url, _allowed_types| {
        git2::Cred::ssh_key(username_from_url.unwrap(), None, ssh_private_key_path, None)
    });

    let mut fo = git2::FetchOptions::new();
    fo.remote_callbacks(cb);
    fo.download_tags(git2::AutotagOption::All);

    let remote_name = remote.name().unwrap();

    tracing::debug!(?remote_name, ?refs, "Fetching data for the repository");

    remote.fetch(refs, Some(&mut fo), None)?;

    // If there are local objects (we got a thin pack), then tell the user
    // how many objects we saved from having to cross the network.
    let stats = remote.stats();

    let indexed_objects = stats.indexed_objects();
    let total_objects = stats.total_objects();
    let received_bytes = stats.received_bytes();
    let local_objects = stats.local_objects();

    tracing::info!(%indexed_objects, %total_objects, %local_objects, %received_bytes, "Successfully updated using the remote");

    let fetch_head = repo.find_reference("FETCH_HEAD")?;
    repo.reference_to_annotated_commit(&fetch_head)
}

/// Performs a fast-forward merge on a repository.
fn fast_forward(
    repo: &git2::Repository,
    lb: &mut git2::Reference,
    rc: &git2::AnnotatedCommit,
) -> Result<(), git2::Error> {
    let name = lb.name().expect("Reference was invalid UTF-8");
    let msg = format!("Fast-Forward: Setting {} to id: {}", name, rc.id());

    repo.set_head(name)?;
    lb.set_target(rc.id(), &msg)?;
    repo.checkout_head(Some(git2::build::CheckoutBuilder::default().force()))?;

    Ok(())
}

/// Performs a normal merge on a repository.
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
        tracing::warn!(local_id = ?local.id(), remote_id = ?remote.id(), "Encountered conflicts between the two versions");
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

/// Performs a merge on a repository, whether that be a fast-forward or normal.
pub fn merge<'a>(
    repo: &'a git2::Repository,
    remote_branch: &str,
    fetch_commit: &git2::AnnotatedCommit<'a>,
) -> Result<(), git2::Error> {
    // 1. do a merge analysis
    let analysis = repo.merge_analysis(&[fetch_commit])?;

    // 2. Do the appopriate merge
    if analysis.0.is_fast_forward() {
        // do a fast forward
        let refname = format!("refs/heads/{}", remote_branch);

        tracing::debug!(%remote_branch, %refname, "Performing a fast-forward merge");

        if let Ok(mut r) = repo.find_reference(&refname) {
            fast_forward(repo, &mut r, fetch_commit)?;
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
        normal_merge(repo, &head_commit, fetch_commit)?;
    }

    Ok(())
}
