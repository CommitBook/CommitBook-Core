use super::operations::GitRepo;
use git2::{Repository, Signature};
use std::path::Path;
use std::process::Command;
use tempfile::{tempdir, TempDir};

pub struct RepoFixture {
    pub remote_dir: TempDir,
    pub repo_dir: TempDir,
    pub repo: GitRepo,
    pub branch: String,
}

/// Create a working repo + bare remote on disk, with one initial commit pushed.
pub fn setup_repo_with_bare_remote() -> RepoFixture {
    let remote_dir = tempdir().unwrap();
    Repository::init_bare(remote_dir.path()).unwrap();

    let repo_dir = tempdir().unwrap();
    let branch = {
        let repo = Repository::init(repo_dir.path()).unwrap();

        {
            let mut config = repo.config().unwrap();
            config.set_str("user.name", "Test User").unwrap();
            config.set_str("user.email", "test@example.com").unwrap();
        }

        std::fs::write(repo_dir.path().join("init.md"), "# init\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("init.md")).unwrap();
        index.write().unwrap();
        let tree_oid = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_oid).unwrap();
        let sig = Signature::now("Test User", "test@example.com").unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
            .unwrap();

        let branch = repo
            .head()
            .unwrap()
            .shorthand()
            .unwrap_or("main")
            .to_string();

        repo.remote(
            "origin",
            &format!("file://{}", remote_dir.path().display()),
        )
        .unwrap();

        branch
    };

    let output = Command::new("git")
        .args(["push", "origin", &branch])
        .current_dir(repo_dir.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "initial push failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let repo = GitRepo::open(repo_dir.path()).unwrap();

    RepoFixture {
        remote_dir,
        repo_dir,
        repo,
        branch,
    }
}

/// Clone the given bare remote into a fresh working directory.
pub fn clone_second_workdir(remote_dir: &Path, branch: &str) -> TempDir {
    let tmp = tempdir().unwrap();
    let output = Command::new("git")
        .args([
            "clone",
            "--branch",
            branch,
            &format!("file://{}", remote_dir.display()),
            &tmp.path().to_string_lossy(),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "clone failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let _ = Command::new("git")
        .args(["config", "user.name", "Second User"])
        .current_dir(tmp.path())
        .output();
    let _ = Command::new("git")
        .args(["config", "user.email", "second@example.com"])
        .current_dir(tmp.path())
        .output();
    tmp
}

/// Write + stage + commit + push a single file on the second workdir.
pub fn commit_and_push_from(workdir: &Path, branch: &str, path: &str, content: &str) {
    let full = workdir.join(path);
    if let Some(parent) = full.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(&full, content).unwrap();

    let add = Command::new("git")
        .args(["add", path])
        .current_dir(workdir)
        .output()
        .unwrap();
    assert!(add.status.success());

    let commit = Command::new("git")
        .args(["commit", "-m", &format!("add {path}")])
        .current_dir(workdir)
        .output()
        .unwrap();
    assert!(
        commit.status.success(),
        "commit failed: {}",
        String::from_utf8_lossy(&commit.stderr)
    );

    let push = Command::new("git")
        .args(["push", "origin", branch])
        .current_dir(workdir)
        .output()
        .unwrap();
    assert!(
        push.status.success(),
        "push failed: {}",
        String::from_utf8_lossy(&push.stderr)
    );
}
