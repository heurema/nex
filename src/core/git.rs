use git2::Repository;

pub fn resolve_push_branch(repo: &Repository, remote: &str) -> String {
    if let Ok(head) = repo.head() {
        if head.is_branch() {
            if let Some(name) = head.shorthand() {
                if !name.is_empty() && name != "HEAD" {
                    return name.to_string();
                }
            }
        }
    }

    let ref_name = format!("refs/remotes/{remote}/HEAD");
    if let Ok(reference) = repo.find_reference(&ref_name) {
        if let Some(target) = reference.symbolic_target() {
            if let Some(branch) = target.rsplit('/').next() {
                if !branch.is_empty() {
                    return branch.to_string();
                }
            }
        }
    }

    "main".to_string()
}

#[cfg(test)]
mod tests {
    use super::resolve_push_branch;
    use git2::{Oid, Repository, Signature};
    use tempfile::tempdir;

    fn init_repo() -> (tempfile::TempDir, Repository, Oid) {
        let dir = tempdir().expect("temp dir");
        let repo = Repository::init(dir.path()).expect("init repo");
        let sig = Signature::now("Test User", "test@example.com").expect("signature");

        std::fs::write(dir.path().join("README.md"), "hello\n").expect("write file");

        let mut index = repo.index().expect("index");
        index
            .add_path(std::path::Path::new("README.md"))
            .expect("add path");
        let tree_id = index.write_tree().expect("write tree");
        let tree = repo.find_tree(tree_id).expect("find tree");
        let oid = repo
            .commit(Some("HEAD"), &sig, &sig, "initial", &tree, &[])
            .expect("commit");

        drop(tree);

        (dir, repo, oid)
    }

    #[test]
    fn prefers_current_checked_out_branch_over_remote_head() {
        let (_dir, repo, oid) = init_repo();
        let commit = repo.find_commit(oid).expect("commit");

        repo.branch("release/v0.12.0", &commit, false)
            .expect("release branch");
        repo.reference("refs/remotes/origin/master", oid, true, "create remote master ref")
            .expect("remote master ref");
        repo.reference_symbolic(
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/master",
            true,
            "create remote head",
        )
        .expect("remote head");
        repo.set_head("refs/heads/release/v0.12.0")
            .expect("set head");

        assert_eq!(resolve_push_branch(&repo, "origin"), "release/v0.12.0");
    }

    #[test]
    fn falls_back_to_remote_head_when_head_is_detached() {
        let (_dir, repo, oid) = init_repo();

        repo.reference("refs/remotes/origin/master", oid, true, "create remote master ref")
            .expect("remote master ref");
        repo.reference_symbolic(
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/master",
            true,
            "create remote head",
        )
        .expect("remote head");
        repo.set_head_detached(oid).expect("detach head");

        assert_eq!(resolve_push_branch(&repo, "origin"), "master");
    }
}
