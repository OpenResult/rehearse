use std::fmt;

/// Declared impact for an operation.
///
/// Impact is explicit metadata supplied by operation authors. It is not inferred
/// from Rust code and is interpreted by dry-run policies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Impact {
    /// Local computation with no externally meaningful effects.
    Pure,
    /// Authentication, token acquisition, or similar setup.
    Session,
    /// Observation of external or managed state.
    Read,
    /// Intentional mutation of managed state.
    Write,
    /// Intentional deletion of managed state.
    Delete,
    /// Unknown or intentionally opaque behavior.
    Opaque,
}

impl fmt::Display for Impact {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Pure => "pure",
            Self::Session => "session",
            Self::Read => "read",
            Self::Write => "write",
            Self::Delete => "delete",
            Self::Opaque => "opaque",
        };
        f.write_str(value)
    }
}
