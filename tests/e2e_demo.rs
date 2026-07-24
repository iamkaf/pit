//! End-to-end first-demo flow against temporary local bare remotes.
//! Drives the real `pit` binary — no re-implementation oracle.

use assert_cmd::cargo::cargo_bin;
use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use std::path::Path;
use std::process::Command as StdCommand;

fn git() -> StdCommand {
    StdCommand::new("git")
}

fn git_in(dir: &Path, args: &[&str]) -> String {
    let out = git()
        .current_dir(dir)
        .args(args)
        .output()
        .expect("git");
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn pit() -> Command {
    let mut c = Command::new(cargo_bin("pit"));
    c.env("GIT_AUTHOR_NAME", "Pit Test");
    c.env("GIT_AUTHOR_EMAIL", "pit@test.local");
    c.env("GIT_COMMITTER_NAME", "Pit Test");
    c.env("GIT_COMMITTER_EMAIL", "pit@test.local");
    // Allow public push through hooks when pit itself pushes — set by push command ideally;
    // for tests, hooks call pit hook which blocks direct git push only.
    c
}

fn setup_identity(dir: &Path) {
    git_in(dir, &["config", "user.name", "Pit Test"]);
    git_in(dir, &["config", "user.email", "pit@test.local"]);
}

/// Scan all objects in a git dir for a path name and content string.
fn public_has_path_or_canary(git_dir: &Path, path: &str, canary: &str) -> (bool, bool) {
    let objects = StdCommand::new("git")
        .arg(format!("--git-dir={}", git_dir.display()))
        .args(["rev-list", "--all", "--objects"])
        .output()
        .expect("rev-list");
    let listing = String::from_utf8_lossy(&objects.stdout);
    let path_hit = listing.lines().any(|l| {
        l.split_once(' ')
            .map(|(_, p)| p == path || p.ends_with(path))
            .unwrap_or(false)
    });

    let mut content_hit = false;
    let commits = StdCommand::new("git")
        .arg(format!("--git-dir={}", git_dir.display()))
        .args(["rev-list", "--all"])
        .output()
        .expect("rev-list commits");
    let commits = String::from_utf8_lossy(&commits.stdout);
    for commit in commits.lines() {
        if commit.is_empty() {
            continue;
        }
        let g = StdCommand::new("git")
            .arg(format!("--git-dir={}", git_dir.display()))
            .args(["grep", "-a", "-F", "-e", canary, commit])
            .output()
            .expect("grep");
        if g.status.success() && !g.stdout.is_empty() {
            content_hit = true;
            break;
        }
    }
    if !content_hit {
        let log = StdCommand::new("git")
            .arg(format!("--git-dir={}", git_dir.display()))
            .args(["log", "-S", canary, "--all", "--oneline"])
            .output()
            .expect("log -S");
        content_hit = log.status.success() && !log.stdout.is_empty();
    }
    (path_hit, content_hit)
}

fn private_has_path(git_dir: &Path, path: &str) -> bool {
    let objects = StdCommand::new("git")
        .arg(format!("--git-dir={}", git_dir.display()))
        .args(["rev-list", "--all", "--objects"])
        .output()
        .expect("rev-list");
    let listing = String::from_utf8_lossy(&objects.stdout);
    listing.lines().any(|l| {
        l.split_once(' ')
            .map(|(_, p)| p == path || p.ends_with(path))
            .unwrap_or(false)
    })
}

fn run_demo_once(root: &Path, label: &str) {
    let public_bare = root.join(format!("public-{label}.git"));
    let private_bare = root.join(format!("private-{label}.git"));
    let work = root.join(format!("work-{label}"));

    // Bare remotes (default branch main so clones check out the pushed branch)
    assert!(git().args(["init", "--bare", "-b", "main", &public_bare.to_string_lossy()]).status().unwrap().success());
    assert!(git().args(["init", "--bare", "-b", "main", &private_bare.to_string_lossy()]).status().unwrap().success());

    // Working public clone/init
    fs::create_dir_all(&work).unwrap();
    git_in(&work, &["init", "-b", "main"]);
    setup_identity(&work);
    git_in(&work, &["remote", "add", "origin", &public_bare.to_string_lossy()]);

    // Initial public commit so branch exists
    fs::write(work.join("README.md"), "# Demo\n").unwrap();
    git_in(&work, &["add", "README.md"]);
    git_in(&work, &["commit", "-m", "initial"]);
    git_in(&work, &["push", "-u", "origin", "main"]);

    // pit setup
    pit()
        .current_dir(&work)
        .args([
            "setup",
            "--private",
            &private_bare.to_string_lossy(),
            "--yes",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Hooks installed"));

    // Mixed public/private files (first demo)
    fs::create_dir_all(work.join("src")).unwrap();
    fs::create_dir_all(work.join("private")).unwrap();
    fs::write(work.join("src/index.ts"), "export const answer = 42;\n").unwrap();
    fs::write(work.join("private/notes.txt"), "PIT-CANARY-7fca1b9d\n").unwrap();

    // Unclassified should fail closed
    fs::write(work.join("mystery.bin"), "secret?").unwrap();
    pit()
        .current_dir(&work)
        .args(["add", "."])
        .assert()
        .failure()
        .stderr(predicate::str::contains("unclassified"));
    fs::remove_file(work.join("mystery.bin")).unwrap();

    // add, status, commit, push
    pit()
        .current_dir(&work)
        .args(["add", "."])
        .assert()
        .success()
        .stdout(predicate::str::contains("public").or(predicate::str::contains("Private")));

    // git add . must not stage private paths
    let staged_after_git = {
        // reset and try plain git add
        let _ = git_in(&work, &["reset", "HEAD"]);
        // re-stage with pit
        pit().current_dir(&work).args(["add", "."]).assert().success();
        // ensure exclude works for a fresh private file
        fs::write(work.join("private/extra.txt"), "extra private\n").unwrap();
        let _ = git()
            .current_dir(&work)
            .args(["add", "."])
            .output()
            .unwrap();
        let staged = git_in(&work, &["diff", "--cached", "--name-only"]);
        assert!(
            !staged.contains("private/extra.txt"),
            "git add staged private path: {staged}"
        );
        // clean extra for commit cleanliness
        fs::remove_file(work.join("private/extra.txt")).unwrap();
        staged
    };
    let _ = staged_after_git;

    pit()
        .current_dir(&work)
        .args(["status"])
        .assert()
        .success();

    pit()
        .current_dir(&work)
        .args(["commit", "-m", "Add public implementation and private notes"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Transaction"));

    // Public commit must not list private paths
    let pub_show = git_in(&work, &["show", "--name-only", "--pretty=format:", "HEAD"]);
    assert!(
        pub_show.contains("src/index.ts"),
        "public commit missing src/index.ts: {pub_show}"
    );
    assert!(
        !pub_show.contains("private/notes.txt"),
        "public commit leaked private path: {pub_show}"
    );
    assert!(
        !pub_show.contains("PIT-CANARY"),
        "public commit contains canary"
    );

    // Public objects must not have canary before push either
    let pub_git = work.join(".git");
    let (ph, ch) = public_has_path_or_canary(&pub_git, "private/notes.txt", "PIT-CANARY-7fca1b9d");
    assert!(!ph, "private path in public object DB before push");
    assert!(!ch, "canary in public object DB before push");

    pit()
        .current_dir(&work)
        .args(["push"])
        .assert()
        .success()
        .stdout(predicate::str::contains("complete").or(predicate::str::contains("pushed")));

    // Public remote assertions
    let (path_hit, canary_hit) =
        public_has_path_or_canary(&public_bare, "private/notes.txt", "PIT-CANARY-7fca1b9d");
    assert!(!path_hit, "public remote has private path");
    assert!(!canary_hit, "public remote has canary content");

    // Public remote has public file
    let (src_hit, _) = public_has_path_or_canary(&public_bare, "src/index.ts", "answer = 42");
    assert!(src_hit, "public remote missing src/index.ts");

    // Private remote has private path and canary
    assert!(
        private_has_path(&private_bare, "private/notes.txt"),
        "private remote missing private/notes.txt"
    );
    let (_, priv_canary) =
        public_has_path_or_canary(&private_bare, "private/notes.txt", "PIT-CANARY-7fca1b9d");
    assert!(priv_canary, "private remote missing canary content");

    // Fresh public-only clone works without Pit
    let clone_dir = root.join(format!("clone-{label}"));
    assert!(git()
        .args([
            "clone",
            &public_bare.to_string_lossy(),
            &clone_dir.to_string_lossy()
        ])
        .status()
        .unwrap()
        .success());
    assert!(clone_dir.join("src/index.ts").exists());
    assert!(!clone_dir.join("private/notes.txt").exists());
    assert!(!clone_dir.join(".git/pit").exists());
    let readme = fs::read_to_string(clone_dir.join("README.md")).unwrap();
    assert!(readme.contains("Demo"));
}

#[test]
fn first_demo_flow_run_twice() {
    let root = tempfile::tempdir().unwrap();
    run_demo_once(root.path(), "a");
    run_demo_once(root.path(), "b");
}

#[test]
fn public_only_commit_no_empty_private() {
    let root = tempfile::tempdir().unwrap();
    let public_bare = root.path().join("pub.git");
    let private_bare = root.path().join("priv.git");
    let work = root.path().join("work");
    assert!(git().args(["init", "--bare", "-b", "main", &public_bare.to_string_lossy()]).status().unwrap().success());
    assert!(git().args(["init", "--bare", "-b", "main", &private_bare.to_string_lossy()]).status().unwrap().success());
    fs::create_dir_all(&work).unwrap();
    git_in(&work, &["init", "-b", "main"]);
    setup_identity(&work);
    git_in(&work, &["remote", "add", "origin", &public_bare.to_string_lossy()]);
    fs::write(work.join("README.md"), "x\n").unwrap();
    git_in(&work, &["add", "README.md"]);
    git_in(&work, &["commit", "-m", "init"]);
    git_in(&work, &["push", "-u", "origin", "main"]);

    pit()
        .current_dir(&work)
        .args(["setup", "--private", &private_bare.to_string_lossy(), "--yes"])
        .assert()
        .success();

    fs::create_dir_all(work.join("src")).unwrap();
    fs::write(work.join("src/only_public.rs"), "fn main() {}\n").unwrap();
    pit().current_dir(&work).args(["add", "src/only_public.rs"]).assert().success();
    pit()
        .current_dir(&work)
        .args(["commit", "-m", "public only"])
        .assert()
        .success()
        .stdout(predicate::str::contains("private: (none)"));
}

#[test]
fn private_only_commit_no_empty_public() {
    let root = tempfile::tempdir().unwrap();
    let public_bare = root.path().join("pub.git");
    let private_bare = root.path().join("priv.git");
    let work = root.path().join("work");
    assert!(git().args(["init", "--bare", "-b", "main", &public_bare.to_string_lossy()]).status().unwrap().success());
    assert!(git().args(["init", "--bare", "-b", "main", &private_bare.to_string_lossy()]).status().unwrap().success());
    fs::create_dir_all(&work).unwrap();
    git_in(&work, &["init", "-b", "main"]);
    setup_identity(&work);
    git_in(&work, &["remote", "add", "origin", &public_bare.to_string_lossy()]);
    fs::write(work.join("README.md"), "x\n").unwrap();
    git_in(&work, &["add", "README.md"]);
    git_in(&work, &["commit", "-m", "init"]);
    git_in(&work, &["push", "-u", "origin", "main"]);

    pit()
        .current_dir(&work)
        .args(["setup", "--private", &private_bare.to_string_lossy(), "--yes"])
        .assert()
        .success();

    fs::create_dir_all(work.join("private")).unwrap();
    fs::write(work.join("private/only.txt"), "only private\n").unwrap();
    pit()
        .current_dir(&work)
        .args(["add", "--private", "private/only.txt"])
        .assert()
        .success();
    pit()
        .current_dir(&work)
        .args(["commit", "-m", "private only"])
        .assert()
        .success()
        .stdout(predicate::str::contains("public:  (none)"));
}

#[test]
fn managed_exclude_preserves_user_lines() {
    let root = tempfile::tempdir().unwrap();
    let work = root.path().join("work");
    let private_bare = root.path().join("priv.git");
    assert!(git().args(["init", "--bare", "-b", "main", &private_bare.to_string_lossy()]).status().unwrap().success());
    fs::create_dir_all(&work).unwrap();
    git_in(&work, &["init", "-b", "main"]);
    setup_identity(&work);
    fs::write(work.join("README.md"), "x\n").unwrap();
    git_in(&work, &["add", "README.md"]);
    git_in(&work, &["commit", "-m", "init"]);

    let exclude = work.join(".git/info/exclude");
    fs::create_dir_all(exclude.parent().unwrap()).unwrap();
    fs::write(&exclude, "*.local-scratch\nmy-user-rule\n").unwrap();

    pit()
        .current_dir(&work)
        .args(["setup", "--private", &private_bare.to_string_lossy(), "--yes"])
        .assert()
        .success();

    let text = fs::read_to_string(&exclude).unwrap();
    assert!(text.contains("*.local-scratch"));
    assert!(text.contains("my-user-rule"));
    assert!(text.contains("BEGIN PIT MANAGED"));
    assert!(text.contains("private/**"));
}

#[test]
fn help_lists_phase1_commands() {
    pit()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("setup"))
        .stdout(predicate::str::contains("status"))
        .stdout(predicate::str::contains("add"))
        .stdout(predicate::str::contains("commit"))
        .stdout(predicate::str::contains("push"))
        .stdout(predicate::str::contains("doctor"));
}

/// Regression: after a private commit materializes `.pit/policy.toml`, plain
/// `git add .` must not stage it into the public index, and `pit push` must
/// reject it if it ever appears in public history.
#[test]
fn pit_meta_not_staged_public_and_push_rejects_leak() {
    let root = tempfile::tempdir().unwrap();
    let public_bare = root.path().join("pub.git");
    let private_bare = root.path().join("priv.git");
    let work = root.path().join("work");
    assert!(git()
        .args(["init", "--bare", "-b", "main", &public_bare.to_string_lossy()])
        .status()
        .unwrap()
        .success());
    assert!(git()
        .args(["init", "--bare", "-b", "main", &private_bare.to_string_lossy()])
        .status()
        .unwrap()
        .success());
    fs::create_dir_all(&work).unwrap();
    git_in(&work, &["init", "-b", "main"]);
    setup_identity(&work);
    git_in(
        &work,
        &["remote", "add", "origin", &public_bare.to_string_lossy()],
    );
    fs::write(work.join("README.md"), "x\n").unwrap();
    git_in(&work, &["add", "README.md"]);
    git_in(&work, &["commit", "-m", "init"]);
    git_in(&work, &["push", "-u", "origin", "main"]);

    pit()
        .current_dir(&work)
        .args([
            "setup",
            "--private",
            &private_bare.to_string_lossy(),
            "--yes",
        ])
        .assert()
        .success();

    // Private-only commit creates `.pit/policy.toml` in the work tree
    fs::create_dir_all(work.join("private")).unwrap();
    fs::write(work.join("private/notes.txt"), "secret\n").unwrap();
    pit()
        .current_dir(&work)
        .args(["add", "--private", "private/notes.txt"])
        .assert()
        .success();
    pit()
        .current_dir(&work)
        .args(["commit", "-m", "private notes"])
        .assert()
        .success();

    assert!(
        work.join(".pit/policy.toml").exists(),
        "private commit must materialize .pit/policy.toml"
    );

    // Managed exclude must list .pit/**
    let exclude = fs::read_to_string(work.join(".git/info/exclude")).unwrap();
    assert!(
        exclude.contains(".pit/**"),
        "managed exclude missing .pit/**: {exclude}"
    );

    // Plain git add must not stage policy into the public index
    let add_out = StdCommand::new("git")
        .current_dir(&work)
        .args(["add", "."])
        .output()
        .unwrap();
    assert!(add_out.status.success(), "git add . failed");
    let staged = git_in(&work, &["diff", "--cached", "--name-only"]);
    assert!(
        !staged.contains(".pit"),
        "git add . staged pit metadata into public index: {staged}"
    );
    let public_tracked = git_in(&work, &["ls-files"]);
    assert!(
        !public_tracked.lines().any(|l| l.starts_with(".pit")),
        "public index tracks .pit path: {public_tracked}"
    );

    // --- Scenario A: pure public-history leak (NOT dual-tracked) ---
    // Remove .pit from the private index so DualTracked cannot short-circuit
    // push; outbound walk must be what rejects the path.
    let _ = StdCommand::new("git")
        .current_dir(&work)
        .args([
            format!("--git-dir={}", work.join(".git/pit/private.git").display()),
            format!("--work-tree={}", work.display()),
            "rm".into(),
            "--cached".into(),
            "-f".into(),
            ".pit/policy.toml".into(),
        ])
        .output()
        .unwrap();
    // Commit the private untrack so dual_tracked is empty
    let _ = StdCommand::new("git")
        .current_dir(&work)
        .env("GIT_AUTHOR_NAME", "Pit Test")
        .env("GIT_AUTHOR_EMAIL", "pit@test.local")
        .env("GIT_COMMITTER_NAME", "Pit Test")
        .env("GIT_COMMITTER_EMAIL", "pit@test.local")
        .args([
            format!("--git-dir={}", work.join(".git/pit/private.git").display()),
            format!("--work-tree={}", work.display()),
            "commit".into(),
            "--no-verify".into(),
            "-m".into(),
            "untrack policy for dual-clear".into(),
        ])
        .output()
        .unwrap();

    // Force-stage .pit into public history only
    let force = StdCommand::new("git")
        .current_dir(&work)
        .args(["add", "-f", ".pit/policy.toml"])
        .output()
        .unwrap();
    assert!(force.status.success());
    let commit = StdCommand::new("git")
        .current_dir(&work)
        .env("GIT_AUTHOR_NAME", "Pit Test")
        .env("GIT_AUTHOR_EMAIL", "pit@test.local")
        .env("GIT_COMMITTER_NAME", "Pit Test")
        .env("GIT_COMMITTER_EMAIL", "pit@test.local")
        .args(["commit", "--no-verify", "-m", "forced policy leak"])
        .output()
        .unwrap();
    assert!(
        commit.status.success(),
        "forced commit failed: {}",
        String::from_utf8_lossy(&commit.stderr)
    );

    // Prove not dual-tracked: public has .pit, private does not
    let pub_files = git_in(&work, &["ls-files"]);
    assert!(
        pub_files.lines().any(|l| l.starts_with(".pit")),
        "public must track .pit for this leak scenario"
    );
    let priv_files = {
        let out = StdCommand::new("git")
            .args([
                format!("--git-dir={}", work.join(".git/pit/private.git").display()),
                format!("--work-tree={}", work.display()),
                "ls-files".into(),
            ])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    assert!(
        !priv_files.lines().any(|l| l.starts_with(".pit")),
        "private must NOT track .pit so DualTracked cannot mask the bug: {priv_files}"
    );

    let push = pit().current_dir(&work).args(["push"]).output().unwrap();
    assert!(
        !push.status.success(),
        "pit push must reject public .pit history; stdout={} stderr={}",
        String::from_utf8_lossy(&push.stdout),
        String::from_utf8_lossy(&push.stderr)
    );
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&push.stdout),
        String::from_utf8_lossy(&push.stderr)
    );
    assert!(
        err.contains("privacy validation")
            || err.contains("private metadata")
            || err.contains(".pit"),
        "expected outbound privacy validation error, got: {err}"
    );
    assert!(
        !err.contains("dual-tracked") && !err.contains("DualTracked"),
        "test must exercise outbound walk, not dual-tracked short-circuit: {err}"
    );

    let (path_hit, _) =
        public_has_path_or_canary(&public_bare, ".pit/policy.toml", "private/**");
    assert!(!path_hit, "public remote received .pit/policy.toml");
}

/// Stale journal tip must not skip validation of newer HEAD commits that
/// `git push` would still publish via refs/heads/<branch>.
#[test]
fn push_validates_head_not_stale_journal_tip() {
    let root = tempfile::tempdir().unwrap();
    let public_bare = root.path().join("pub.git");
    let private_bare = root.path().join("priv.git");
    let work = root.path().join("work");
    assert!(git()
        .args(["init", "--bare", "-b", "main", &public_bare.to_string_lossy()])
        .status()
        .unwrap()
        .success());
    assert!(git()
        .args(["init", "--bare", "-b", "main", &private_bare.to_string_lossy()])
        .status()
        .unwrap()
        .success());
    fs::create_dir_all(&work).unwrap();
    git_in(&work, &["init", "-b", "main"]);
    setup_identity(&work);
    git_in(
        &work,
        &["remote", "add", "origin", &public_bare.to_string_lossy()],
    );
    fs::write(work.join("README.md"), "x\n").unwrap();
    git_in(&work, &["add", "README.md"]);
    git_in(&work, &["commit", "-m", "init"]);
    git_in(&work, &["push", "-u", "origin", "main"]);

    pit()
        .current_dir(&work)
        .args([
            "setup",
            "--private",
            &private_bare.to_string_lossy(),
            "--yes",
        ])
        .assert()
        .success();

    // Clean public-only commit leaves LocalComplete journal with public_after = A
    fs::create_dir_all(work.join("src")).unwrap();
    fs::write(work.join("src/ok.rs"), "fn ok() {}\n").unwrap();
    pit()
        .current_dir(&work)
        .args(["add", "src/ok.rs"])
        .assert()
        .success();
    pit()
        .current_dir(&work)
        .args(["commit", "-m", "clean public A"])
        .assert()
        .success();
    let tip_a = git_in(&work, &["rev-parse", "HEAD"]);

    // After journal tip A, force a second public commit B that leaks .pit
    // without dual-tracking (never add .pit to private).
    fs::create_dir_all(work.join(".pit")).unwrap();
    fs::write(
        work.join(".pit/policy.toml"),
        "version = 1\n# leaked canary POLICY-LEAK-STALE-TIP\n",
    )
    .unwrap();
    git_in(&work, &["add", "-f", ".pit/policy.toml"]);
    let commit_b = StdCommand::new("git")
        .current_dir(&work)
        .env("GIT_AUTHOR_NAME", "Pit Test")
        .env("GIT_AUTHOR_EMAIL", "pit@test.local")
        .env("GIT_COMMITTER_NAME", "Pit Test")
        .env("GIT_COMMITTER_EMAIL", "pit@test.local")
        .args(["commit", "--no-verify", "-m", "leak after journal tip"])
        .output()
        .unwrap();
    assert!(
        commit_b.status.success(),
        "{}",
        String::from_utf8_lossy(&commit_b.stderr)
    );
    let tip_b = git_in(&work, &["rev-parse", "HEAD"]);
    assert_ne!(tip_a, tip_b, "HEAD must advance past journal tip A");

    // Journal still points at A while HEAD is B — push must still reject B.
    let push = pit().current_dir(&work).args(["push"]).output().unwrap();
    assert!(
        !push.status.success(),
        "stale journal must not allow leak push; stdout={} stderr={}",
        String::from_utf8_lossy(&push.stdout),
        String::from_utf8_lossy(&push.stderr)
    );
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&push.stdout),
        String::from_utf8_lossy(&push.stderr)
    );
    assert!(
        err.contains(".pit")
            || err.contains("privacy validation")
            || err.contains("private metadata"),
        "expected privacy validation of HEAD tip B, got: {err}"
    );

    let (path_hit, content_hit) = public_has_path_or_canary(
        &public_bare,
        ".pit/policy.toml",
        "POLICY-LEAK-STALE-TIP",
    );
    assert!(!path_hit, "public remote has .pit path after stale-tip push");
    assert!(
        !content_hit,
        "public remote has POLICY-LEAK-STALE-TIP after stale-tip push"
    );
}
