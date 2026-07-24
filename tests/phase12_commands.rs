//! Phase 1 leftovers + Phase 2 collaboration flows against temp bare remotes.
//! Drives the shipped `pit` binary.

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
    let out = git().current_dir(dir).args(args).output().expect("git");
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
    c
}

fn setup_identity(dir: &Path) {
    git_in(dir, &["config", "user.name", "Pit Test"]);
    git_in(dir, &["config", "user.email", "pit@test.local"]);
}

fn bare(path: &Path) {
    assert!(git()
        .args(["init", "--bare", "-b", "main", &path.to_string_lossy()])
        .status()
        .unwrap()
        .success());
}

fn init_work(work: &Path, public_bare: &Path, private_bare: &Path) {
    fs::create_dir_all(work).unwrap();
    git_in(work, &["init", "-b", "main"]);
    setup_identity(work);
    git_in(
        work,
        &["remote", "add", "origin", &public_bare.to_string_lossy()],
    );
    fs::write(work.join("README.md"), "# t\n").unwrap();
    git_in(work, &["add", "README.md"]);
    git_in(work, &["commit", "-m", "init"]);
    git_in(work, &["push", "-u", "origin", "main"]);
    pit()
        .current_dir(work)
        .args([
            "setup",
            "--private",
            &private_bare.to_string_lossy(),
            "--yes",
        ])
        .assert()
        .success();
}

#[test]
fn restore_staged_splits_indexes() {
    let root = tempfile::tempdir().unwrap();
    let pub_b = root.path().join("p.git");
    let priv_b = root.path().join("v.git");
    let work = root.path().join("w");
    bare(&pub_b);
    bare(&priv_b);
    init_work(&work, &pub_b, &priv_b);

    fs::create_dir_all(work.join("src")).unwrap();
    fs::create_dir_all(work.join("private")).unwrap();
    fs::write(work.join("src/a.rs"), "fn a(){}\n").unwrap();
    fs::write(work.join("private/b.txt"), "sec\n").unwrap();
    pit()
        .current_dir(&work)
        .args(["add", "."])
        .assert()
        .success();

    let pub_staged = git_in(&work, &["diff", "--cached", "--name-only"]);
    assert!(pub_staged.contains("src/a.rs"));

    pit()
        .current_dir(&work)
        .args(["restore", "--staged", "src/a.rs"])
        .assert()
        .success();
    let pub_staged2 = git_in(&work, &["diff", "--cached", "--name-only"]);
    assert!(
        !pub_staged2.contains("src/a.rs"),
        "public still staged: {pub_staged2}"
    );

    // private still staged
    let priv_staged = StdCommand::new("git")
        .args([
            format!("--git-dir={}", work.join(".git/pit/private.git").display()),
            format!("--work-tree={}", work.display()),
            "diff".into(),
            "--cached".into(),
            "--name-only".into(),
        ])
        .output()
        .unwrap();
    let ps = String::from_utf8_lossy(&priv_staged.stdout);
    assert!(ps.contains("private/b.txt"), "private unstaged unexpectedly: {ps}");
}

#[test]
fn hooks_lifecycle_and_json_schema() {
    let root = tempfile::tempdir().unwrap();
    let pub_b = root.path().join("p.git");
    let priv_b = root.path().join("v.git");
    let work = root.path().join("w");
    bare(&pub_b);
    bare(&priv_b);
    init_work(&work, &pub_b, &priv_b);

    pit()
        .current_dir(&work)
        .args(["hooks", "status", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("schema_version"))
        .stdout(predicate::str::contains("pre-commit"));

    pit()
        .current_dir(&work)
        .args(["hooks", "uninstall"])
        .assert()
        .success();
    let pre = work.join(".git/hooks/pre-commit");
    assert!(!pre.exists() || {
        let t = fs::read_to_string(&pre).unwrap_or_default();
        !t.contains("Pit hook dispatcher")
    });

    pit()
        .current_dir(&work)
        .args(["hooks", "install"])
        .assert()
        .success();
    let t = fs::read_to_string(work.join(".git/hooks/pre-commit")).unwrap();
    assert!(t.contains("Pit hook dispatcher"));
    assert!(work.join(".git/hooks/post-checkout").exists());

    pit()
        .current_dir(&work)
        .args(["hooks", "repair"])
        .assert()
        .success();

    pit()
        .current_dir(&work)
        .args(["doctor", "--repair", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("schema_version"))
        .stdout(predicate::str::contains("repairs"));
}

#[test]
fn config_get_set_list_and_prompt_fail_closed() {
    let root = tempfile::tempdir().unwrap();
    let pub_b = root.path().join("p.git");
    let priv_b = root.path().join("v.git");
    let work = root.path().join("w");
    bare(&pub_b);
    bare(&priv_b);
    init_work(&work, &pub_b, &priv_b);

    pit()
        .current_dir(&work)
        .args(["config", "set", "policy.new_files", "prompt"])
        .assert()
        .success();
    pit()
        .current_dir(&work)
        .args(["config", "get", "policy.new_files"])
        .assert()
        .success()
        .stdout(predicate::str::contains("prompt"));
    pit()
        .current_dir(&work)
        .args(["config", "list", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("schema_version"));

    fs::write(work.join("mystery.bin"), "x").unwrap();
    // non-TTY: prompt mode still fail-closed
    pit()
        .current_dir(&work)
        .args(["add", "mystery.bin"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("unclassified"));
}

#[test]
fn protect_reveal_ignore_and_diff() {
    let root = tempfile::tempdir().unwrap();
    let pub_b = root.path().join("p.git");
    let priv_b = root.path().join("v.git");
    let work = root.path().join("w");
    bare(&pub_b);
    bare(&priv_b);
    init_work(&work, &pub_b, &priv_b);

    fs::create_dir_all(work.join("src")).unwrap();
    fs::write(work.join("src/pub.rs"), "fn p(){}\n").unwrap();
    pit()
        .current_dir(&work)
        .args(["add", "src/pub.rs"])
        .assert()
        .success();
    pit()
        .current_dir(&work)
        .args(["commit", "-m", "add public"])
        .assert()
        .success();
    pit().current_dir(&work).args(["push"]).assert().success();

    // protect with history → requires --yes and warns
    pit()
        .current_dir(&work)
        .args(["protect", "src/pub.rs"])
        .assert()
        .failure();
    pit()
        .current_dir(&work)
        .args(["protect", "src/pub.rs", "--yes"])
        .assert()
        .success()
        .stderr(predicate::str::contains("WARNING").or(predicate::str::contains("history")));

    pit()
        .current_dir(&work)
        .args(["diff", "--private", "--staged"])
        .assert()
        .success();

    pit()
        .current_dir(&work)
        .args(["commit", "-m", "protect pub.rs"])
        .assert()
        .success();

    // reveal requires --yes non-interactive
    pit()
        .current_dir(&work)
        .args(["reveal", "src/pub.rs"])
        .assert()
        .failure();
    pit()
        .current_dir(&work)
        .args(["reveal", "src/pub.rs", "--yes"])
        .assert()
        .success();

    fs::write(work.join("tmp_ignore_me.txt"), "x").unwrap();
    pit()
        .current_dir(&work)
        .args(["add", "--public", "tmp_ignore_me.txt"])
        .assert()
        .success();
    pit()
        .current_dir(&work)
        .args(["ignore", "tmp_ignore_me.txt"])
        .assert()
        .success();
    let tracked = git_in(&work, &["ls-files"]);
    assert!(!tracked.lines().any(|l| l == "tmp_ignore_me.txt"));
}

#[test]
fn switch_and_transaction_list() {
    let root = tempfile::tempdir().unwrap();
    let pub_b = root.path().join("p.git");
    let priv_b = root.path().join("v.git");
    let work = root.path().join("w");
    bare(&pub_b);
    bare(&priv_b);
    init_work(&work, &pub_b, &priv_b);

    pit()
        .current_dir(&work)
        .args(["switch", "-c", "feature/x"])
        .assert()
        .success();
    let b = git_in(&work, &["branch", "--show-current"]);
    assert_eq!(b, "feature/x");

    pit()
        .current_dir(&work)
        .args(["transaction", "list", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("schema_version"));
}

#[test]
fn clone_with_private_and_canary_roundtrip() {
    let root = tempfile::tempdir().unwrap();
    let pub_b = root.path().join("public.git");
    let priv_b = root.path().join("private.git");
    bare(&pub_b);
    bare(&priv_b);

    // seed public remote
    let seed = root.path().join("seed");
    fs::create_dir_all(&seed).unwrap();
    git_in(&seed, &["init", "-b", "main"]);
    setup_identity(&seed);
    git_in(&seed, &["remote", "add", "origin", &pub_b.to_string_lossy()]);
    fs::write(seed.join("README.md"), "seed\n").unwrap();
    git_in(&seed, &["add", "README.md"]);
    git_in(&seed, &["commit", "-m", "seed"]);
    git_in(&seed, &["push", "-u", "origin", "main"]);

    // also need empty private remote ready — setup in clone will init local private

    let dest = root.path().join("cloned");
    pit()
        .current_dir(root.path())
        .args([
            "clone",
            &pub_b.to_string_lossy(),
            "--private",
            &priv_b.to_string_lossy(),
            "--directory",
            &dest.to_string_lossy(),
            "--yes",
        ])
        .assert()
        .success();

    assert!(dest.join(".git/pit/config.toml").exists());

    fs::create_dir_all(dest.join("src")).unwrap();
    fs::create_dir_all(dest.join("private")).unwrap();
    fs::write(dest.join("src/i.ts"), "export const a=1\n").unwrap();
    fs::write(dest.join("private/notes.txt"), "PIT-CANARY-PHASE12\n").unwrap();
    pit().current_dir(&dest).args(["add", "."]).assert().success();
    pit()
        .current_dir(&dest)
        .args(["commit", "-m", "mixed"])
        .assert()
        .success();
    pit().current_dir(&dest).args(["push"]).assert().success();

    let objects = StdCommand::new("git")
        .arg(format!("--git-dir={}", pub_b.display()))
        .args(["rev-list", "--all", "--objects"])
        .output()
        .unwrap();
    let listing = String::from_utf8_lossy(&objects.stdout);
    assert!(
        !listing.contains("private/notes"),
        "public has private path"
    );
    assert!(listing.contains("src/i.ts"));
}

#[test]
fn pull_ff_smoke() {
    let root = tempfile::tempdir().unwrap();
    let pub_b = root.path().join("p.git");
    let priv_b = root.path().join("v.git");
    let work = root.path().join("w");
    bare(&pub_b);
    bare(&priv_b);
    init_work(&work, &pub_b, &priv_b);

    // second clone to push a public commit
    let other = root.path().join("other");
    assert!(git()
        .args(["clone", &pub_b.to_string_lossy(), &other.to_string_lossy()])
        .status()
        .unwrap()
        .success());
    setup_identity(&other);
    fs::write(other.join("extra.md"), "e\n").unwrap();
    git_in(&other, &["add", "extra.md"]);
    git_in(&other, &["commit", "-m", "extra"]);
    git_in(&other, &["push", "origin", "main"]);

    pit()
        .current_dir(&work)
        .args(["pull", "--yes"])
        .assert()
        .success();
    assert!(work.join("extra.md").exists());
}
