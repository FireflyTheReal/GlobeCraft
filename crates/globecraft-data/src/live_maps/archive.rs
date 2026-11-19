//! The clouds of an earlier moment (roadmap C2), while the app's time is set back:
//! matteason's cloud maps as kept every three hours by
//! joshuabinswanger/matteason_live-cloud-maps_downloader on GitHub (CC0). This week's
//! maps are releases of their own, `maps-YYYYMMDD_HHMM`, the whole 8192 × 4096 map;
//! each finished week is one release, `archive-YYYY-Www`, holding a zip of 2048 × 1024
//! frames, of which only the one wanted is read (its length, the zip's directory at its
//! end, then the frame: three ranges of a 30 MB file).
use chrono::{DateTime, Datelike, NaiveDateTime};
use std::io::{Read, Seek, SeekFrom};

const RELEASES: &str =
    "https://github.com/joshuabinswanger/matteason_live-cloud-maps_downloader/releases/download";
/// The maps are kept every three hours, at 00:00, 03:00 … 21:00 UTC.
pub(super) const SLOT_SECS: i64 = 3 * 60 * 60;
/// The end of a week's zip, read before its frame: its directory (about 4 KB for 56
/// frames) is in it.
const ZIP_TAIL_BYTES: u64 = 64 * 1024;
/// A 2048 × 1024 frame is about 600 KB.
const FRAME_MAX_BYTES: u64 = 4 * 1024 * 1024;
/// Room for a frame's own header in the zip (30 bytes, its name and extra field).
const LOCAL_HEADER_MAX_BYTES: u64 = 1024;
/// A whole 8192 × 4096 map is about 5 MB as a JPEG.
const MAP_MAX_BYTES: u64 = 32 * 1024 * 1024;
/// How the archive names a moment (`20261112_0900`), and how it is shown.
const STAMP_FORMAT: &str = "%Y%m%d_%H%M";
const SHOWN_FORMAT: &str = "%Y-%m-%d %H:%M";

/// The three-hour slot of `unix` when it is before the slot of `now`: its clouds are
/// kept. `None`: the live map (the current slot, or a time ahead).
pub(super) fn earlier_slot(unix: Option<f64>, now: f64) -> Option<i64> {
    let slot = |time: f64| (time / SLOT_SECS as f64).floor() as i64;
    unix.map(slot).filter(|&wanted| wanted < slot(now))
}

/// A slot's moment as the archive names it (`20261112_0900`) and its ISO week
/// (`2026-W46`), by the moment's own date.
fn names(slot: i64) -> Option<(String, String)> {
    let moment = DateTime::from_timestamp(slot.checked_mul(SLOT_SECS)?, 0)?.naive_utc();
    let week = moment.iso_week();
    Some((
        moment.format(STAMP_FORMAT).to_string(),
        format!("{}-W{:02}", week.year(), week.week()),
    ))
}

/// The moment's time as shown (`2026-11-12 09:00`).
pub(super) fn shown(slot: i64) -> String {
    DateTime::from_timestamp(slot.saturating_mul(SLOT_SECS), 0)
        .map(|moment| moment.format(SHOWN_FORMAT).to_string())
        .unwrap_or_default()
}

/// The clouds of `slot` (white on black: their opacity), from this week's maps or a
/// week's archive, whichever is likelier to hold it first. Where none was kept (before
/// the archive began, or a missed week), a map without levels: no clouds show, and the
/// moment is not asked for again while it holds (errors are retried every minute).
pub(super) async fn download(slot: i64, now: f64) -> Result<super::CloudMap, crate::Error> {
    let none_kept = || super::CloudMap { levels: Vec::new() };
    if ((slot + 1) * SLOT_SECS) as f64 <= super::CLOUDS_KEPT_SINCE_UNIX {
        return Ok(none_kept());
    }
    let (stamp, week) = names(slot).ok_or_else(|| crate::Error::msg("time out of range"))?;
    let this_week =
        names((now / SLOT_SECS as f64).floor() as i64).is_some_and(|(_, current)| current == week);
    let client = crate::http::client(crate::http::Policy::Cloud)?;
    let still = format!("{RELEASES}/maps-{stamp}/clouds_{stamp}.jpg");
    let found = if this_week {
        match fetch(&client, &still, MAP_MAX_BYTES).await? {
            Some(bytes) => Some(bytes),
            None => archived(&client, &stamp, &week).await?,
        }
    } else {
        match archived(&client, &stamp, &week).await? {
            Some(bytes) => Some(bytes),
            None => fetch(&client, &still, MAP_MAX_BYTES).await?,
        }
    };
    let Some(bytes) = found else {
        return Ok(none_kept());
    };
    tokio::task::spawn_blocking(move || {
        let image = crate::bounded::image(&bytes, crate::bounded::ImageLimit::WORLD)?;
        Ok(super::download::cloud_map(image.into_luma8()))
    })
    .await?
}

/// A file's body, `None` where it is not there (404).
async fn fetch(
    client: &reqwest::Client,
    url: &str,
    limit: u64,
) -> Result<Option<Vec<u8>>, crate::Error> {
    let response = client.get(url).send().await?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    Ok(Some(
        crate::http::read_limited(response.error_for_status()?, limit).await?,
    ))
}

/// The frame of `week`'s zip nearest to `stamp`, `None` when the week has no archive.
async fn archived(
    client: &reqwest::Client,
    stamp: &str,
    week: &str,
) -> Result<Option<Vec<u8>>, crate::Error> {
    let url = format!("{RELEASES}/archive-{week}/clouds_2K_{week}.zip");
    // Its length from its first byte: the host takes no range counted from the end.
    let response = client
        .get(&url)
        .header(reqwest::header::RANGE, "bytes=0-0")
        .send()
        .await?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    let response = response.error_for_status()?;
    let total = (response.status() == reqwest::StatusCode::PARTIAL_CONTENT)
        .then(|| response.headers().get(reqwest::header::CONTENT_RANGE))
        .flatten()
        .and_then(|range| range.to_str().ok()?.rsplit_once('/')?.1.parse::<u64>().ok())
        .filter(|&total| total > 0)
        .ok_or_else(|| crate::Error::msg("the cloud archive answered without a range"))?;
    let range = |range: std::ops::Range<u64>| {
        crate::http::body(
            client.get(&url).header(
                reqwest::header::RANGE,
                format!("bytes={}-{}", range.start, range.end - 1),
            ),
            range.end - range.start,
        )
    };
    let tail = range(total.saturating_sub(ZIP_TAIL_BYTES)..total).await?;
    let frame = find_frame(&tail, total, stamp)?;
    let part = range(frame.range.clone()).await?;
    read_frame(&tail, total, &frame, &part).map(Some)
}

/// A frame in a week's zip: its name and where its header and data are.
#[derive(Debug, PartialEq)]
struct Frame {
    name: String,
    range: std::ops::Range<u64>,
}

/// The frame nearest in time to `stamp` (an earlier one on a tie: a missed run leaves
/// a gap), from the zip's directory in its last bytes `tail`.
fn find_frame(tail: &[u8], total: u64, stamp: &str) -> Result<Frame, crate::Error> {
    let minutes = |stamp: &str| {
        NaiveDateTime::parse_from_str(stamp, STAMP_FORMAT)
            .ok()
            .map(|moment| moment.and_utc().timestamp() / 60)
    };
    let wanted = minutes(stamp).ok_or_else(|| crate::Error::msg("bad cloud time"))?;
    let archive = zip_reader(Ranges::new(total, [(total - tail.len() as u64, tail)]))?;
    let directory = archive.central_directory_start();
    let mut best: Option<(i64, usize)> = None;
    for index in 0..archive.len() {
        let Some(Ok(name)) = archive.name_for_index(index) else {
            continue;
        };
        let Some(at) = name
            .strip_prefix("clouds_")
            .and_then(|rest| rest.strip_suffix(".jpg"))
            .and_then(minutes)
        else {
            continue;
        };
        let key = ((at - wanted).abs() * 2 + i64::from(at > wanted), index);
        if best.is_none_or(|best| key < best) {
            best = Some(key);
        }
    }
    let (_, index) = best.ok_or_else(|| crate::Error::msg("the cloud archive holds no frame"))?;
    let entry = archive.by_index_data(index)?;
    if entry.size() > FRAME_MAX_BYTES {
        return Err(crate::Error::TooLarge {
            limit: FRAME_MAX_BYTES,
        });
    }
    let start = entry.header_start();
    let end = (start + LOCAL_HEADER_MAX_BYTES + entry.compressed_size()).min(directory);
    if end <= start {
        return Err(crate::Error::msg(
            "the cloud archive's directory is damaged",
        ));
    }
    Ok(Frame {
        name: entry.name()?.into_owned(),
        range: start..end,
    })
}

/// A zip read where it stands: the archive's start is not searched for (that reads
/// the whole file).
fn zip_reader<const N: usize>(
    ranges: Ranges<'_, N>,
) -> Result<zip::ZipArchive<Ranges<'_, N>>, crate::Error> {
    let config = zip::read::Config {
        archive_offset: zip::read::ArchiveOffset::Known(0),
    };
    Ok(zip::ZipArchive::with_config(config, ranges)?)
}

/// The frame's bytes, from the zip's end and the frame's own range `part`.
fn read_frame(
    tail: &[u8],
    total: u64,
    frame: &Frame,
    part: &[u8],
) -> Result<Vec<u8>, crate::Error> {
    let ranges = Ranges::new(
        total,
        [(total - tail.len() as u64, tail), (frame.range.start, part)],
    );
    let mut archive = zip_reader(ranges)?;
    let mut bytes = Vec::new();
    archive
        .by_name(&frame.name)?
        .take(FRAME_MAX_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > FRAME_MAX_BYTES {
        return Err(crate::Error::TooLarge {
            limit: FRAME_MAX_BYTES,
        });
    }
    Ok(bytes)
}

/// A file of which only some ranges were fetched, read as a whole: elsewhere it reads
/// as zeros, which the zip's own checks refuse (its signatures, a frame's checksum),
/// so nothing is made up.
struct Ranges<'a, const N: usize> {
    len: u64,
    parts: [(u64, &'a [u8]); N],
    at: u64,
}

impl<'a, const N: usize> Ranges<'a, N> {
    fn new(len: u64, parts: [(u64, &'a [u8]); N]) -> Self {
        Self { len, parts, at: 0 }
    }
}

impl<const N: usize> Read for Ranges<'_, N> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let at = self.at;
        let left = self.len.saturating_sub(at);
        if left == 0 || buffer.is_empty() {
            return Ok(0);
        }
        let mut count = buffer.len().min(left as usize);
        match self
            .parts
            .iter()
            .find(|(start, bytes)| (*start..start + bytes.len() as u64).contains(&at))
        {
            Some((start, bytes)) => {
                let from = &bytes[(at - start) as usize..];
                count = count.min(from.len());
                buffer[..count].copy_from_slice(&from[..count]);
            }
            None => {
                // Up to the next part fetched, nothing.
                let next = self
                    .parts
                    .iter()
                    .map(|(start, _)| *start)
                    .filter(|&start| start > at)
                    .min()
                    .unwrap_or(self.len);
                count = count.min((next - at) as usize);
                buffer[..count].fill(0);
            }
        }
        self.at += count as u64;
        Ok(count)
    }
}

impl<const N: usize> Seek for Ranges<'_, N> {
    fn seek(&mut self, to: SeekFrom) -> std::io::Result<u64> {
        let at = match to {
            SeekFrom::Start(at) => Some(at),
            SeekFrom::End(back) => self.len.checked_add_signed(back),
            SeekFrom::Current(by) => self.at.checked_add_signed(by),
        };
        self.at = at.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "seek before the start")
        })?;
        Ok(self.at)
    }
}

/// Only for the tests: the hour of a slot.
#[cfg(test)]
fn hour(slot: i64) -> u32 {
    use chrono::Timelike;
    DateTime::from_timestamp(slot * SLOT_SECS, 0)
        .unwrap()
        .hour()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn a_time_set_back_wants_the_slot_it_falls_in() {
        let time = DateTime::parse_from_rfc3339("2026-11-12T10:30:00Z")
            .unwrap()
            .timestamp() as f64;
        let slot = earlier_slot(Some(time), time + 86_400.0).unwrap();
        assert_eq!(hour(slot), 9);
        assert_eq!(
            names(slot).unwrap(),
            ("20261112_0900".into(), "2026-W46".into())
        );
        assert_eq!(shown(slot), "2026-11-12 09:00");
        // The current slot, a time ahead and no time: the live map.
        assert_eq!(earlier_slot(Some(time), time + 60.0), None);
        assert_eq!(earlier_slot(Some(time + 86_400.0), time), None);
        assert_eq!(earlier_slot(None, time), None);
        // The ISO week runs across the new year: 1 January 2027 is in 2026's week 53.
        let new_year = DateTime::parse_from_rfc3339("2027-01-01T13:00:00Z")
            .unwrap()
            .timestamp() as f64;
        let slot = earlier_slot(Some(new_year), new_year + 86_400.0).unwrap();
        assert_eq!(names(slot).unwrap().1, "2026-W53");
    }

    /// A week's zip as the archive makes it: frames `stamps`, each its stamp's bytes.
    fn week_zip(stamps: &[&str]) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for stamp in stamps {
            writer
                .start_file(format!("clouds_{stamp}.jpg"), options)
                .unwrap();
            writer.write_all(stamp.repeat(500).as_bytes()).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    #[test]
    fn one_frame_is_read_from_the_end_and_its_own_range() {
        let stamps = ["20261109_0000", "20261109_0300", "20261109_0900"];
        let zip = week_zip(&stamps);
        let total = zip.len() as u64;
        let tail = &zip[zip.len().saturating_sub(400)..];
        let read = |stamp: &str| {
            let frame = find_frame(tail, total, stamp).unwrap();
            let part = &zip[frame.range.start as usize..frame.range.end as usize];
            (
                frame.name.clone(),
                read_frame(tail, total, &frame, part).unwrap(),
            )
        };
        let (name, bytes) = read("20261109_0300");
        assert_eq!(name, "clouds_20261109_0300.jpg");
        assert_eq!(bytes, "20261109_0300".repeat(500).as_bytes());
        // A missed run: the nearest frame, the earlier of two as near.
        assert_eq!(read("20261109_0600").0, "clouds_20261109_0300.jpg");
        assert_eq!(read("20261110_0000").0, "clouds_20261109_0900.jpg");
        // Outside the ranges fetched nothing is made up.
        let frame = find_frame(tail, total, "20261109_0000").unwrap();
        assert!(read_frame(tail, total, &frame, &[]).is_err());
    }
}

#[cfg(test)]
mod probe {
    use super::*;

    /// A frame of a finished week (2048 wide), and one of six hours ago (this week's
    /// 8192-wide map, or 2048 wide once its week is archived).
    #[tokio::test]
    #[ignore = "network: fetches two cloud maps from GitHub"]
    async fn kept_clouds_load() {
        let now = super::super::unix_now();
        let ago = |hours: f64| earlier_slot(Some(now - hours * 3600.0), now).unwrap();
        let archived = download(ago(14.0 * 24.0), now).await.unwrap();
        assert_eq!(archived.levels[0].width(), 2048);
        let recent = download(ago(6.0), now).await.unwrap();
        assert!([2048, 8192].contains(&recent.levels[0].width()));
        println!("CLOUDS OK");
    }
}
