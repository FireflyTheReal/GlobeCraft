//! Built without the `updater` feature (for a distribution package that updates through
//! its own package manager): no release is looked up, and the service stays off.
use super::Snapshot;
use std::{path::Path, sync::Arc};
use tokio::sync::watch;

#[derive(Clone)]
pub(super) struct SelectedRelease {
    pub(super) version: String,
}
pub(super) enum Checked {
    Installable(SelectedRelease),
    NoBuild { version: String },
}
#[derive(Debug, thiserror::Error)]
#[error("this build of GlobeCraft has no updater; use your package manager or Releases")]
pub(super) struct Error;

pub(super) fn is_development_path(_path: &Path) -> bool {
    false
}
pub(super) fn manual_install_reason(_path: &Path) -> Option<String> {
    Some(Error.to_string())
}
pub(super) async fn check() -> Result<Option<Checked>, Error> {
    Err(Error)
}
pub(super) async fn install(
    _selected: &SelectedRelease,
    _sender: watch::Sender<Snapshot>,
    _repaint: Arc<dyn Fn() + Send + Sync>,
) -> Result<bool, Error> {
    Err(Error)
}
pub(super) fn install_path_protected(_error: &Error) -> bool {
    false
}
