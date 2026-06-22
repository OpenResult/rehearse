use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Impact {
    Pure,
    Session,
    Read,
    Write,
    Delete,
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
