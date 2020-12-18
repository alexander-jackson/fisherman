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
}

#[derive(Debug, Deserialize)]
pub struct Repository {
    id: u32,
    name: String,
    full_name: String,
    master_branch: String,
}
