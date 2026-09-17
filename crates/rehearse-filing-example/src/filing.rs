use rehearse::{operation, pipeline, Plan};
use std::collections::BTreeSet;
use std::error::Error;
use std::ffi::OsString;
use std::fmt;
use std::fs::{self, Metadata, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

pub struct Office {
    pub root: PathBuf,
}

impl Office {
    fn inbox(&self) -> PathBuf {
        self.root.join("inbox")
    }
}

#[derive(Debug)]
pub enum FilingError {
    Io {
        action: &'static str,
        path: PathBuf,
        source: io::Error,
    },
    UnexpectedEntry {
        path: PathBuf,
        expected: &'static str,
    },
    DestinationExists(PathBuf),
    VerificationFailed(PathBuf),
}

impl fmt::Display for FilingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io {
                action,
                path,
                source,
            } => write!(f, "cannot {action} {}: {source}", path.display()),
            Self::UnexpectedEntry { path, expected } => write!(
                f,
                "expected {expected} at {} (symlinks are not supported)",
                path.display()
            ),
            Self::DestinationExists(path) => {
                write!(f, "destination already exists: {}", path.display())
            }
            Self::VerificationFailed(path) => write!(
                f,
                "file changed or filing could not be verified: {}",
                path.display()
            ),
        }
    }
}

impl Error for FilingError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

fn io_error(action: &'static str, path: &Path, source: io::Error) -> FilingError {
    FilingError::Io {
        action,
        path: path.to_owned(),
        source,
    }
}

#[derive(Clone)]
struct Document {
    name: OsString,
    bytes: u64,
}

#[derive(Clone)]
struct Assignment {
    document: Document,
    folder: &'static str,
}

#[derive(Clone)]
struct FilingBatch(Vec<Assignment>);

#[derive(Clone)]
struct PreparedFolders(FilingBatch);

#[derive(Clone)]
struct FilingReceipt(FilingBatch);

#[derive(Clone)]
pub struct FilingSummary {
    pub filed: usize,
    pub pending_review: usize,
}

// Bodies are delayed: constructing or describing this plan never opens the inbox.
#[pipeline]
pub fn filing_plan() -> Plan<Office, FilingSummary, FilingError> {
    let documents = step!(scan_inbox())?;
    let batch = step!(determine_filing_locations(documents))?;
    let checked = step!(check_destinations(batch))?;
    let folders = step!(create_destination_folders(checked))?;
    let receipt = step!(move_documents(folders))?;
    // This read has no dependency on the write, so it still runs in dry-run.
    step!(check_remaining_inbox())?;
    // This read needs a real receipt, so it is blocked in dry-run.
    let summary = step!(verify_filed_documents(receipt))?;
    Ok(summary)
}

#[operation(impact = read)]
async fn scan_inbox(#[context] office: &Office) -> Result<Vec<Document>, FilingError> {
    read_inbox(office)
}

#[operation(impact = pure)]
async fn determine_filing_locations(documents: Vec<Document>) -> Result<FilingBatch, FilingError> {
    let assignments = documents
        .into_iter()
        .map(|document| {
            let name = document.name.to_string_lossy();
            let folder = if name.starts_with("invoice-") {
                "accounts/invoices"
            } else if name.starts_with("expense-") {
                "accounts/expenses"
            } else if name.starts_with("minutes-") {
                "administration/minutes"
            } else if name.starts_with("report-") {
                "management/reports"
            } else {
                "pending-review"
            };
            Assignment { document, folder }
        })
        .collect();
    Ok(FilingBatch(assignments))
}

#[operation(impact = read)]
async fn check_destinations(
    #[context] office: &Office,
    batch: FilingBatch,
) -> Result<FilingBatch, FilingError> {
    for assignment in &batch.0 {
        println!(
            "  inbox/{} -> {}/{}",
            assignment.document.name.to_string_lossy(),
            assignment.folder,
            assignment.document.name.to_string_lossy()
        );
    }
    println!(
        "{} documents assessed; {} require manual review.",
        batch.0.len(),
        pending_review(&batch)
    );
    validate_destinations(office, &batch)?;
    Ok(batch)
}

#[operation(impact = write)]
async fn create_destination_folders(
    #[context] office: &Office,
    batch: FilingBatch,
) -> Result<PreparedFolders, FilingError> {
    validate_destinations(office, &batch)?;
    let folders: BTreeSet<_> = batch.0.iter().map(|assignment| assignment.folder).collect();
    for folder in folders {
        let path = office.root.join(folder);
        fs::create_dir_all(&path).map_err(|error| io_error("create directory", &path, error))?;
    }
    Ok(PreparedFolders(batch))
}

#[operation(impact = write)]
async fn move_documents(
    #[context] office: &Office,
    prepared: PreparedFolders,
) -> Result<FilingReceipt, FilingError> {
    let batch = prepared.0;
    validate_destinations(office, &batch)?;
    for assignment in &batch.0 {
        let source = office.inbox().join(&assignment.document.name);
        let destination = office
            .root
            .join(assignment.folder)
            .join(&assignment.document.name);
        check_document(&source, assignment.document.bytes)?;
        // Unlike rename on Unix, hard_link refuses to replace any existing entry.
        // This example requires one filesystem and an inbox that is not changing.
        fs::hard_link(&source, &destination)
            .map_err(|error| io_error("link document to", &destination, error))?;
        // Only remove the inbox entry once the destination exists. If removal
        // fails, both entries remain and execution stops; there is no rollback.
        fs::remove_file(&source).map_err(|error| {
            io_error(
                "remove inbox entry after linking (file remains in both locations)",
                &source,
                error,
            )
        })?;
    }
    Ok(FilingReceipt(batch))
}

#[operation(impact = read)]
async fn check_remaining_inbox(#[context] office: &Office) -> Result<usize, FilingError> {
    let remaining = read_inbox(office)?.len();
    println!("Inbox check: {remaining} documents remaining.");
    Ok(remaining)
}

#[operation(impact = read)]
async fn verify_filed_documents(
    #[context] office: &Office,
    receipt: FilingReceipt,
) -> Result<FilingSummary, FilingError> {
    let batch = receipt.0;
    for assignment in &batch.0 {
        let source = office.inbox().join(&assignment.document.name);
        let destination = office
            .root
            .join(assignment.folder)
            .join(&assignment.document.name);
        check_document(&destination, assignment.document.bytes)?;
        if metadata_if_present(&source)?.is_some() {
            return Err(FilingError::VerificationFailed(source));
        }
    }
    Ok(FilingSummary {
        filed: batch.0.len(),
        pending_review: pending_review(&batch),
    })
}

fn pending_review(batch: &FilingBatch) -> usize {
    batch
        .0
        .iter()
        .filter(|assignment| assignment.folder == "pending-review")
        .count()
}

fn read_inbox(office: &Office) -> Result<Vec<Document>, FilingError> {
    require_directory(&office.root)?;
    let inbox = office.inbox();
    require_directory(&inbox)?;
    let entries =
        fs::read_dir(&inbox).map_err(|error| io_error("read directory", &inbox, error))?;
    let mut documents = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| io_error("read directory entry", &inbox, error))?;
        let path = entry.path();
        let metadata =
            fs::symlink_metadata(&path).map_err(|error| io_error("inspect", &path, error))?;
        if metadata.is_dir() {
            continue; // This example files only documents directly inside inbox/.
        }
        if !metadata.is_file() {
            return Err(FilingError::UnexpectedEntry {
                path,
                expected: "a regular file",
            });
        }
        documents.push(Document {
            name: entry.file_name(),
            bytes: metadata.len(),
        });
    }
    documents.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(documents)
}

fn validate_destinations(office: &Office, batch: &FilingBatch) -> Result<(), FilingError> {
    require_directory(&office.root)?;
    require_directory(&office.inbox())?;
    for assignment in &batch.0 {
        let mut directory = office.root.clone();
        for component in Path::new(assignment.folder).components() {
            directory.push(component);
            if let Some(metadata) = metadata_if_present(&directory)? {
                if !metadata.is_dir() {
                    return Err(FilingError::UnexpectedEntry {
                        path: directory,
                        expected: "a directory",
                    });
                }
            }
        }
        let destination = directory.join(&assignment.document.name);
        if metadata_if_present(&destination)?.is_some() {
            return Err(FilingError::DestinationExists(destination));
        }
    }
    Ok(())
}

fn require_directory(path: &Path) -> Result<(), FilingError> {
    let metadata =
        fs::symlink_metadata(path).map_err(|error| io_error("inspect directory", path, error))?;
    if !metadata.is_dir() {
        return Err(FilingError::UnexpectedEntry {
            path: path.to_owned(),
            expected: "a directory",
        });
    }
    Ok(())
}

fn metadata_if_present(path: &Path) -> Result<Option<Metadata>, FilingError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(io_error("inspect", path, error)),
    }
}

fn check_document(path: &Path, expected_bytes: u64) -> Result<(), FilingError> {
    let metadata =
        fs::symlink_metadata(path).map_err(|error| io_error("inspect document", path, error))?;
    if !metadata.is_file() || metadata.len() != expected_bytes {
        return Err(FilingError::VerificationFailed(path.to_owned()));
    }
    Ok(())
}

pub fn seed_demo(root: &Path) -> Result<(), FilingError> {
    // Requiring a new root prevents reseeding from overwriting an existing office.
    fs::create_dir(root).map_err(|error| io_error("create new demo directory", root, error))?;
    let inbox = root.join("inbox");
    fs::create_dir(&inbox).map_err(|error| io_error("create directory", &inbox, error))?;
    for name in [
        "invoice-1042.pdf",
        "invoice-1043.pdf",
        "expense-march.csv",
        "minutes-monday.txt",
        "report-quarterly.pdf",
        "document-final-FINAL.docx",
        "meeting-about-reducing-meetings.txt",
    ] {
        let path = inbox.join(name);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|error| io_error("create sample", &path, error))?;
        writeln!(file, "Office filing sample: {name}\nPlain-text placeholder; not a real PDF or Office document.")
            .map_err(|error| io_error("write sample", &path, error))?;
    }
    Ok(())
}
