//! Provider and transport failures, with the diagnostic fields kept apart from the message.

/// Where a request failed before a response arrived. Classified where it happens, never by the
/// wording of a diagnostic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransportFailure {
    /// The connection could not be made or broke; worth retrying.
    Connection,
    /// The request took too long.
    Timeout,
    /// The reply exceeded the size limit or stopped mid-way.
    Interrupted,
}

/// A failed request and its diagnostic context. Diagnostics are English and never translated.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    /// HTTP status, including 200 for a stream that reported a failure.
    pub status: Option<u16>,
    /// The provider's error code (`context_length_exceeded`, `overloaded_error`…).
    pub code: Option<String>,
    /// The request parameter the provider objected to.
    pub param: Option<String>,
    /// The provider's request id, for support requests.
    pub request_id: Option<String>,
    /// A bounded diagnostic that never contains credentials.
    pub detail: String,
    pub transport: Option<TransportFailure>,
}

impl Error {
    pub fn transport(kind: TransportFailure, detail: impl Into<String>) -> Self {
        Self {
            transport: Some(kind),
            ..Self::from(detail.into())
        }
    }
}

impl From<String> for Error {
    fn from(detail: String) -> Self {
        Self {
            status: None,
            code: None,
            param: None,
            request_id: None,
            detail,
            transport: None,
        }
    }
}

impl From<&str> for Error {
    fn from(detail: &str) -> Self {
        detail.to_string().into()
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(status) = self.status.filter(|s| *s != 200) {
            write!(f, "HTTP {status}: ")?;
        }
        f.write_str(&self.detail)?;
        for v in [&self.code, &self.param, &self.request_id]
            .into_iter()
            .flatten()
        {
            write!(f, " [{v}]")?;
        }
        Ok(())
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_keeps_status_code_and_request_id() {
        let mut e = Error::from("model not found");
        e.status = Some(404);
        e.code = Some("not_found_error".into());
        e.request_id = Some("req_1".into());
        assert_eq!(
            e.to_string(),
            "HTTP 404: model not found [not_found_error] [req_1]"
        );
        let t = Error::transport(TransportFailure::Connection, "refused");
        assert_eq!(t.transport, Some(TransportFailure::Connection));
        assert_eq!(t.to_string(), "refused");
    }
}
