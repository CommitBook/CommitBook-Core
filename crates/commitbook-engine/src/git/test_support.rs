use super::operations::GitRepo;
use git2::{Repository, Signature};
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::{tempdir, TempDir};

pub struct RepoFixture {
    pub remote_dir: TempDir,
    pub repo_dir: TempDir,
    pub repo: GitRepo,
    pub branch: String,
}

/// Test fixture with `files` committed as the base state and a
/// `.CommitBook/local/` directory ready for state writes.
pub struct BaseRepoFixture {
    // Held to keep the temp dir alive for the duration of the test.
    #[allow(dead_code)]
    pub tmp: TempDir,
    pub repo_root: PathBuf,
    #[allow(dead_code)]
    pub branch: String,
}

impl BaseRepoFixture {
    /// Open a fresh `GitRepo` handle on the fixture's working tree.
    pub fn repo(&self) -> GitRepo {
        GitRepo::open(&self.repo_root).expect("fixture repo should open")
    }
}

/// Initialize a git repo at a temp dir, commit the provided files, and return
/// a ready-to-use fixture.
pub fn setup_repo_with_base(files: &[(&str, &str)]) -> BaseRepoFixture {
    let tmp = tempdir().unwrap();
    let repo_root = tmp.path().to_path_buf();
    let cb_dir = repo_root.join(".CommitBook");
    std::fs::create_dir_all(cb_dir.join("local")).unwrap();

    let branch = {
        let repo = Repository::init(&repo_root).unwrap();
        {
            let mut config = repo.config().unwrap();
            config.set_str("user.name", "Test User").unwrap();
            config.set_str("user.email", "test@example.com").unwrap();
            // Never require a signing key in tests, regardless of global config.
            config.set_bool("commit.gpgsign", false).unwrap();
        }

        let mut index = repo.index().unwrap();
        for (rel_path, content) in files {
            let full = repo_root.join(rel_path);
            if let Some(parent) = full.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(&full, content).unwrap();
            index.add_path(Path::new(rel_path)).unwrap();
        }

        // If no files were given, commit an empty tree so there's always a HEAD.
        index.write().unwrap();
        let tree_oid = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_oid).unwrap();
        let sig = Signature::now("Test User", "test@example.com").unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "base", &tree, &[])
            .unwrap();

        let branch = repo.head().unwrap().shorthand().unwrap().to_string();
        branch
    };

    BaseRepoFixture {
        tmp,
        repo_root,
        branch,
    }
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
            // Never require a signing key in tests, regardless of global config.
            config.set_bool("commit.gpgsign", false).unwrap();
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

        let branch = repo.head().unwrap().shorthand().unwrap().to_string();

        repo.remote("origin", &format!("file://{}", remote_dir.path().display()))
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
    let _ = Command::new("git")
        .args(["config", "commit.gpgsign", "false"])
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

/// Point the repository's `core.excludesFile` at a private file holding
/// `content`, so tests never depend on the developer's global ignore rules
/// (many Macs ignore `.DS_Store` there). Open repository handles after this
/// call: libgit2 reads the setting when a handle first checks ignores.
pub fn set_repo_excludes(repo_root: &Path, content: &str) {
    let path = repo_root.join(".git/commitbook-test-excludes");
    std::fs::write(&path, content).unwrap();
    Repository::open(repo_root)
        .unwrap()
        .config()
        .unwrap()
        .set_str("core.excludesFile", path.to_str().unwrap())
        .unwrap();
}
