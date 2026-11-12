//! Preserve typed causes until the application formats a user-facing status.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Image(#[from] image::ImageError),
    #[error(transparent)]
    Http(#[from] reqwest::Error),
    #[error(transparent)]
    Png(#[from] png::EncodingError),
    #[error(transparent)]
    Xml(#[from] quick_xml::Error),
    #[error(transparent)]
    Coordinates(#[from] globecraft_core::coordinates::CoordinateError),
    #[error(transparent)]
    Kml(#[from] ::kml::Error),
    #[error(transparent)]
    Zip(#[from] zip::result::ZipError),
    #[error(transparent)]
    PrefixIndex(#[from] fst::Error),
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    SqliteWorker(#[from] tokio_rusqlite::Error),
    #[error(transparent)]
    Task(#[from] tokio::task::JoinError),
    /// A response longer than the caller accepts; nothing of it is used.
    #[error("the response exceeds {limit} bytes")]
    TooLarge { limit: u64 },
    /// Work offline is on: no request was made.
    #[error("GlobeCraft is set to work offline")]
    Offline,
    /// A layer needs a key for this service and none was typed.
    #[error("this layer needs a {service} key: add it in Settings → Keys")]
    MissingKey { service: &'static str },
    /// A file or service in a version or encoding GlobeCraft does not read.
    #[error("{0}")]
    Unsupported(String),
    /// Anything else, described in words; nothing reacts to these by type.
    #[error("{0}")]
    Message(String),
}
impl Error {
    /// An [`Error::Message`]. There is deliberately no `From<String>`, so `?` cannot
    /// turn a plain string into an error by accident.
    pub fn msg(text: impl Into<String>) -> Self {
        Self::Message(text.into())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error as _;
    #[test]
    fn io_errors_keep_their_source_and_display() {
        let io = std::io::Error::new(std::io::ErrorKind::NotFound, "missing tile");
        let error = Error::from(io);
        assert_eq!(error.to_string(), "missing tile");
        assert!(matches!(error, Error::Io(_)));
        // Transparent errors delegate their display to the original cause.
        assert!(error.source().is_none());
    }
}
