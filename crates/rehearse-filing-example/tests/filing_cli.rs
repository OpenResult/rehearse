use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

struct OfficeTest {
    temporary: PathBuf,
    root: PathBuf,
}

impl OfficeTest {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let temporary = loop {
            let candidate = std::env::temp_dir().join(format!(
                "rehearse-filing-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&candidate) {
                Ok(()) => break candidate,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("cannot create test directory: {error}"),
            }
        };
        Self {
            root: temporary.join("office"),
            temporary,
        }
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_rehearse-filing-example"))
            .arg(&self.root)
            .args(args)
            .output()
            .unwrap()
    }

    fn seed(&self) {
        assert_success(&self.run(&["--seed-demo"]));
    }
}

impl Drop for OfficeTest {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.temporary);
    }
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn snapshot(root: &Path) -> BTreeMap<PathBuf, Option<Vec<u8>>> {
    fn visit(root: &Path, directory: &Path, result: &mut BTreeMap<PathBuf, Option<Vec<u8>>>) {
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            let relative = path.strip_prefix(root).unwrap().to_owned();
            if path.is_dir() {
                result.insert(relative, None);
                visit(root, &path, result);
            } else {
                result.insert(relative, Some(fs::read(path).unwrap()));
            }
        }
    }
    let mut result = BTreeMap::new();
    visit(root, root, &mut result);
    result
}

#[test]
fn describe_never_needs_an_existing_office() {
    let office = OfficeTest::new();
    let output = office.run(&["--describe"]);
    assert_success(&output);
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("move_documents"));
    assert!(text.contains("verify_filed_documents"));
    assert!(!office.root.exists());

    assert!(!office.run(&["--dry-run"]).status.success());
    assert!(!office.root.exists());
}

#[test]
fn default_dry_run_preserves_every_file_and_directory() {
    let office = OfficeTest::new();
    office.seed();
    let before = snapshot(&office.root);
    for args in [&[][..], &["--dry-run"][..]] {
        let output = office.run(args);
        assert_success(&output);
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.contains("7 documents assessed; 2 require manual review."));
        assert!(text.contains("inbox/invoice-1042.pdf -> accounts/invoices/invoice-1042.pdf"));
        assert!(text.contains("Inbox check: 7 documents remaining."));
        assert!(text.contains("[ok] check_remaining_inbox executed"));
        assert!(text.contains("[block] verify_filed_documents blocked"));
        assert!(text.contains("4 executed, 2 skipped, 0 denied, 1 blocked, 0 failed"));
        assert_eq!(snapshot(&office.root), before);
    }
}

#[test]
fn execute_files_documents_with_contents_intact_and_leaves_subdirectories_alone() {
    let office = OfficeTest::new();
    office.seed();
    fs::create_dir(office.root.join("inbox/nested")).unwrap();
    fs::write(office.root.join("inbox/nested/keep.txt"), b"leave here").unwrap();
    let before = snapshot(&office.root);
    let output = office.run(&["--execute"]);
    assert_success(&output);
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("7 documents filed and verified; 2 require manual review."));
    assert!(text.contains("Inbox check: 0 documents remaining."));
    for (name, folder) in [
        ("invoice-1042.pdf", "accounts/invoices"),
        ("invoice-1043.pdf", "accounts/invoices"),
        ("expense-march.csv", "accounts/expenses"),
        ("minutes-monday.txt", "administration/minutes"),
        ("report-quarterly.pdf", "management/reports"),
        ("document-final-FINAL.docx", "pending-review"),
        ("meeting-about-reducing-meetings.txt", "pending-review"),
    ] {
        let source = Path::new("inbox").join(name);
        assert!(!office.root.join(&source).exists());
        assert_eq!(
            Some(fs::read(office.root.join(folder).join(name)).unwrap()),
            before[&source]
        );
    }
    assert_eq!(
        fs::read(office.root.join("inbox/nested/keep.txt")).unwrap(),
        b"leave here"
    );
    let filed = snapshot(&office.root);
    let repeated = office.run(&["--execute"]);
    assert_success(&repeated);
    assert!(String::from_utf8(repeated.stdout)
        .unwrap()
        .contains("0 documents filed and verified"));
    assert_eq!(snapshot(&office.root), filed);
}

#[test]
fn destination_conflicts_fail_before_any_files_move() {
    let office = OfficeTest::new();
    office.seed();
    fs::create_dir_all(office.root.join("management/reports")).unwrap();
    fs::write(
        office.root.join("management/reports/report-quarterly.pdf"),
        b"existing report",
    )
    .unwrap();
    let before = snapshot(&office.root);
    let output = office.run(&["--dry-run"]);
    assert!(!output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("destination already exists"));
    assert!(text.contains("Inbox check: 7 documents remaining."));
    assert_eq!(snapshot(&office.root), before);
    let output = office.run(&["--execute"]);
    assert!(!output.status.success());
    assert!(String::from_utf8(output.stderr)
        .unwrap()
        .contains("destination already exists"));
    assert_eq!(snapshot(&office.root), before);
}

#[test]
fn reseeding_and_conflicting_modes_do_not_change_an_existing_office() {
    let office = OfficeTest::new();
    office.seed();
    fs::write(
        office.root.join("inbox/invoice-1042.pdf"),
        b"edited document",
    )
    .unwrap();
    let before = snapshot(&office.root);
    for args in [
        &["--seed-demo"][..],
        &["--describe", "--execute"][..],
        &["--dry-run", "--execute"][..],
        &["--seed-demo", "--execute"][..],
    ] {
        assert!(!office.run(args).status.success());
        assert_eq!(snapshot(&office.root), before);
    }
}

#[test]
fn empty_inbox_needs_no_filing_folders() {
    let office = OfficeTest::new();
    fs::create_dir_all(office.root.join("inbox")).unwrap();
    let before = snapshot(&office.root);
    assert_success(&office.run(&["--execute"]));
    assert_eq!(snapshot(&office.root), before);
}

#[cfg(unix)]
#[test]
fn symlinks_are_rejected_without_touching_their_targets() {
    use std::os::unix::fs::symlink;
    for relative in [
        "inbox/linked.txt",
        "accounts",
        "management/reports/report-quarterly.pdf",
    ] {
        let office = OfficeTest::new();
        office.seed();
        let external = office.temporary.join("external");
        fs::create_dir(&external).unwrap();
        fs::write(external.join("keep.txt"), b"external content").unwrap();
        let link = office.root.join(relative);
        fs::create_dir_all(link.parent().unwrap()).unwrap();
        // The destination file case is deliberately a dangling link.
        let target = if relative.ends_with(".pdf") {
            external.join("absent")
        } else {
            external.clone()
        };
        symlink(target, &link).unwrap();
        assert!(!office.run(&["--dry-run"]).status.success());
        assert!(!office.run(&["--execute"]).status.success());
        assert!(fs::symlink_metadata(&link).unwrap().is_symlink());
        assert_eq!(
            fs::read_dir(office.root.join("inbox")).unwrap().count(),
            if relative.starts_with("inbox/") { 8 } else { 7 }
        );
        assert_eq!(
            fs::read(external.join("keep.txt")).unwrap(),
            b"external content"
        );
        assert_eq!(fs::read_dir(&external).unwrap().count(), 1);
    }
}
