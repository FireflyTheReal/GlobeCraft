//! Downloads running now, with how many each source and each host group has, kept up
//! to date as downloads start and end instead of counted again for every job.
use super::{KeyBuild, Provider, TileKey, unkey};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Notify;

#[derive(Default)]
pub(super) struct Downloads {
    /// Each running download, with the signal that stops it.
    running: HashMap<TileKey, Arc<Notify>, KeyBuild>,
    /// Running downloads by `Provider::id`.
    per_source: HashMap<u64, usize>,
    /// Running downloads by `Provider::host_group`.
    per_host: HashMap<u64, usize>,
}

impl Downloads {
    pub(super) fn insert(&mut self, key: TileKey, cancel: Arc<Notify>) {
        if self.running.insert(key, cancel).is_none() {
            self.count(key, true);
        }
    }
    pub(super) fn remove(&mut self, key: TileKey) -> Option<Arc<Notify>> {
        let cancel = self.running.remove(&key)?;
        self.count(key, false);
        Some(cancel)
    }
    pub(super) fn contains(&self, key: TileKey) -> bool {
        self.running.contains_key(&key)
    }
    pub(super) fn keys(&self) -> impl Iterator<Item = TileKey> + '_ {
        self.running.keys().copied()
    }
    /// Keeps the downloads `keep` accepts; the others are dropped from the counts.
    pub(super) fn retain(&mut self, mut keep: impl FnMut(TileKey, &Arc<Notify>) -> bool) {
        let mut dropped = Vec::new();
        self.running.retain(|&key, cancel| {
            let kept = keep(key, cancel);
            if !kept {
                dropped.push(key);
            }
            kept
        });
        for key in dropped {
            self.count(key, false);
        }
    }
    pub(super) fn of_source(&self, provider: Provider) -> usize {
        self.per_source.get(&provider.id()).copied().unwrap_or(0)
    }
    pub(super) fn of_host(&self, provider: Provider) -> usize {
        self.per_host
            .get(&provider.host_group())
            .copied()
            .unwrap_or(0)
    }
    fn count(&mut self, key: TileKey, started: bool) {
        let provider = unkey(key).0;
        for (map, id) in [
            (&mut self.per_source, provider.id()),
            (&mut self.per_host, provider.host_group()),
        ] {
            if started {
                *map.entry(id).or_default() += 1;
            } else if let Some(count) = map.get_mut(&id) {
                *count = count.saturating_sub(1);
                if *count == 0 {
                    map.remove(&id);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tiles::make_key;

    #[test]
    fn counts_follow_inserts_removals_and_retain() {
        let mut downloads = Downloads::default();
        let satellite = |x| make_key(Provider::Satellite, 5, x, 0, 0);
        let dem = make_key(Provider::TerrainDem, 5, 0, 0, 0);
        for key in [satellite(0), satellite(1), satellite(2), dem] {
            downloads.insert(key, Arc::new(Notify::new()));
        }
        // Inserting a running key again does not count it twice.
        downloads.insert(satellite(0), Arc::new(Notify::new()));
        assert_eq!(downloads.of_source(Provider::Satellite), 3);
        assert_eq!(downloads.of_source(Provider::TerrainDem), 1);
        assert!(downloads.remove(satellite(1)).is_some());
        assert!(downloads.remove(satellite(1)).is_none());
        assert_eq!(downloads.of_source(Provider::Satellite), 2);
        downloads.retain(|key, _| key != dem);
        assert_eq!(downloads.of_source(Provider::TerrainDem), 0);
        assert!(!downloads.contains(dem));
        downloads.retain(|_, _| false);
        assert!(downloads.per_source.is_empty() && downloads.per_host.is_empty());
        assert_eq!(downloads.keys().count(), 0);
    }
}
