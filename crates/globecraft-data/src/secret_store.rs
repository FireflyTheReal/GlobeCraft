//! Keys in the system's credential store (the macOS Keychain, the Windows Credential
//! Manager) when the user turns it on in Settings › Keys. It is off by default: a build
//! signed differently (every local rebuild) makes macOS ask again before reading. When
//! off, or on other systems, keys stay in files only the user can read.
use std::sync::atomic::{AtomicBool, Ordering};

static ENABLED: AtomicBool = AtomicBool::new(false);

/// Whether this system has a credential store GlobeCraft can use.
pub fn available() -> bool {
    cfg!(any(target_os = "macos", windows))
}

pub(crate) fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

pub(crate) fn set_enabled(on: bool) {
    ENABLED.store(on && available(), Ordering::Relaxed);
}

#[cfg(any(target_os = "macos", windows))]
mod store {
    /// The name every GlobeCraft entry is filed under.
    const SERVICE: &str = "GlobeCraft";

    fn entry(account: &str) -> Result<keyring::Entry, crate::Error> {
        keyring::Entry::new(SERVICE, account).map_err(failure)
    }

    fn failure(error: keyring::Error) -> crate::Error {
        crate::Error::msg(format!("system keychain: {error}"))
    }

    pub(super) fn read(account: &str) -> Result<Option<String>, crate::Error> {
        match entry(account)?.get_password() {
            Ok(text) => Ok(Some(text)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(failure(error)),
        }
    }

    pub(super) fn write(account: &str, text: Option<&str>) -> Result<(), crate::Error> {
        let entry = entry(account)?;
        match text {
            Some(text) => entry.set_password(text).map_err(failure),
            None => match entry.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(error) => Err(failure(error)),
            },
        }
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
mod store {
    pub(super) fn read(_account: &str) -> Result<Option<String>, crate::Error> {
        Ok(None)
    }

    pub(super) fn write(_account: &str, _text: Option<&str>) -> Result<(), crate::Error> {
        Err(crate::Error::Unsupported(
            "this system has no credential store GlobeCraft uses".into(),
        ))
    }
}

/// The text stored under `account`; `None` when there is none.
pub(crate) fn read(account: &str) -> Result<Option<String>, crate::Error> {
    store::read(account)
}

/// Stores `text` under `account`, or removes the entry for `None`.
pub(crate) fn write(account: &str, text: Option<&str>) -> Result<(), crate::Error> {
    store::write(account, text)
}
