//! Service-specific downloads and radar palette decoding.
use super::{CloudMap, RADAR_STEP_SECS, RadarFrame};
use image::{GrayImage, RgbaImage, imageops::FilterType};
const CLOUD_SOURCE: &str = "https://clouds.matteason.co.uk/images/8192x4096/clouds-alpha.png";
/// The same service's map of how much the sea shines: water bright, land and cloud
/// dark (it follows the clouds). Its 4096-wide edition: a texel is 10 km.
const WATER_SOURCE: &str = "https://clouds.matteason.co.uk/images/4096x2048/specular.jpg";
/// The 4096 × 2048 JPEG is about 1.1 MB.
const WATER_IMAGE_MAX_BYTES: u64 = 8 * 1024 * 1024;
const RADAR_API: &str = "https://api.rainviewer.com/public/weather-maps.json";
const TILE_ZOOM: usize = 2;
const TILE_SIZE: u32 = 512;
/// The source cloud image is an 8192×4096 PNG. Reject runaway or substituted downloads.
const CLOUD_IMAGE_MAX_BYTES: u64 = 32 * 1024 * 1024;
/// The radar release list is a small JSON document.
const RADAR_METADATA_MAX_BYTES: u64 = 1024 * 1024;
/// One 512 px radar tile, with ample room for its PNG encoding.
const RADAR_TILE_MAX_BYTES: u64 = 2 * 1024 * 1024;
/// The cloud map is kept at its full width and at these smaller ones (each half the
/// last), so a view far out samples a level near its own pixel size.
const CLOUD_LEVELS: usize = 3;
pub(super) async fn download_clouds() -> Result<CloudMap, crate::Error> {
    let client = crate::http::client(crate::http::Policy::Cloud)?;
    // Cache bust by 3-hour slots; URL remains the requested source image.
    let url = format!("{CLOUD_SOURCE}?slot={}", three_hour_slot());
    let bytes = crate::http::body(client.get(url), CLOUD_IMAGE_MAX_BYTES).await?;
    // Decode off the UI thread. The clouds are their opacity (the image's alpha; its
    // grey is near white throughout): one channel at full size, 32 MB, and the
    // smaller levels from it. Shrunk to 2048 × 1024 they blurred (user, 2026-11-18).
    tokio::task::spawn_blocking(move || {
        let image = crate::bounded::image(&bytes, crate::bounded::ImageLimit::WORLD)?;
        let (width, height) = (image.width(), image.height());
        let alpha: Vec<u8> = image
            .into_luma_alpha8()
            .pixels()
            .map(|pixel| pixel[1])
            .collect();
        let full = GrayImage::from_raw(width, height, alpha)
            .ok_or_else(|| crate::Error::msg("cloud image has an unexpected size"))?;
        Ok(cloud_map(full))
    })
    .await?
}

/// The cloud map of the clouds' opacity `full`, with its smaller levels.
pub(super) fn cloud_map(full: GrayImage) -> CloudMap {
    let mut levels = vec![full];
    while levels.len() < CLOUD_LEVELS {
        let last = levels.last().expect("one level at least");
        let (width, height) = ((last.width() / 2).max(1), (last.height() / 2).max(1));
        levels.push(image::imageops::resize(
            last,
            width,
            height,
            FilterType::Triangle,
        ));
    }
    CloudMap { levels }
}

/// The water's shine (see `WATER_SOURCE`), one channel.
pub(super) async fn download_water() -> Result<GrayImage, crate::Error> {
    let client = crate::http::client(crate::http::Policy::Cloud)?;
    let url = format!("{WATER_SOURCE}?slot={}", three_hour_slot());
    let bytes = crate::http::body(client.get(url), WATER_IMAGE_MAX_BYTES).await?;
    tokio::task::spawn_blocking(move || {
        Ok(crate::bounded::image(&bytes, crate::bounded::ImageLimit::WORLD)?.into_luma8())
    })
    .await?
}

/// The current 3-hour slot since 1970, to bust caches with the source's refresh.
fn three_hour_slot() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        / (3 * 60 * 60)
}

/// Which of the radar frames (their times, oldest first) to show for the ten-minute
/// step `wanted`: the last one measured by its end, the oldest when it is earlier
/// than all of them, the newest for `None`.
pub(super) fn radar_frame_index(times: &[i64], wanted: Option<i64>) -> Option<usize> {
    let last = times.len().checked_sub(1)?;
    let Some(wanted) = wanted else {
        return Some(last);
    };
    let by = wanted + RADAR_STEP_SECS as i64;
    Some(times.iter().rposition(|&time| time < by).unwrap_or(0))
}

/// The radar frame for the ten-minute step `wanted` (`None`: the newest).
pub(super) async fn download_radar(wanted: Option<i64>) -> Result<RadarFrame, crate::Error> {
    let client = crate::http::client(crate::http::Policy::Radar)?;
    let metadata = crate::http::body(client.get(RADAR_API), RADAR_METADATA_MAX_BYTES).await?;
    let json = crate::http::parse_json(metadata).await?;
    let frames = json
        .pointer("/radar/past")
        .and_then(|v| v.as_array())
        .ok_or_else(|| crate::Error::msg("No radar frames available"))?;
    let times: Vec<i64> = frames
        .iter()
        .map(|f| f.get("time").and_then(|v| v.as_i64()).unwrap_or(i64::MIN))
        .collect();
    let frame = radar_frame_index(&times, wanted)
        .and_then(|index| frames.get(index))
        .ok_or_else(|| crate::Error::msg("No radar frames available"))?;
    let path = frame
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| crate::Error::msg("Radar path missing"))?;
    if !path.starts_with("/v2/radar/")
        || !path[10..]
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '/')
    {
        return Err(crate::Error::msg("Unexpected radar path"));
    }
    let time = frame
        .get("time")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| crate::Error::msg("Radar time missing"))?;
    let n = 1usize << TILE_ZOOM;
    let mut image = RgbaImage::new((n as u32) * TILE_SIZE, (n as u32) * TILE_SIZE);
    let mut accepted = 0;
    let limiter = std::sync::Arc::new(tokio::sync::Semaphore::new(4));
    let mut tasks = tokio::task::JoinSet::new();
    for y in 0..n {
        for x in 0..n {
            // RainViewer 2026 free API: ONLY color scheme 2 and max zoom 7.
            let url = format!(
                "https://tilecache.rainviewer.com{path}/{TILE_SIZE}/{TILE_ZOOM}/{x}/{y}/2/1_0.png"
            );
            let client = client.clone();
            let limiter = limiter.clone();
            tasks.spawn(async move {
                let _permit = limiter.acquire_owned().await.ok()?;
                let bytes = crate::http::body(client.get(url), RADAR_TILE_MAX_BYTES)
                    .await
                    .ok()?;
                let image = crate::bounded::image(&bytes, crate::bounded::ImageLimit::TILE)
                    .ok()?
                    .into_rgba8();
                Some((x, y, image))
            });
        }
    }
    while let Some(result) = tasks.join_next().await {
        let Ok(Some((x, y, tile))) = result else {
            continue;
        };
        accepted += 1;
        for (px, py, c) in tile.enumerate_pixels() {
            if px >= TILE_SIZE || py >= TILE_SIZE {
                continue;
            }
            let mapped = recolor_radar(c.0);
            image.put_pixel(
                x as u32 * TILE_SIZE + px,
                y as u32 * TILE_SIZE + py,
                image::Rgba(mapped),
            );
        }
    }
    if accepted == 0 {
        return Err(crate::Error::msg(
            "RainViewer radar tiles unavailable (check network/coverage)",
        ));
    }
    Ok(RadarFrame { image, time })
}

// The API only delivers Universal Blue as of 2026. Map its RGB palette to an
// approximate reflectivity (dBZ) and display a radar-style green-yellow-red ramp.
// This is an intensity visualization, not temperature or satellite infrared.
fn recolor_radar(src: [u8; 4]) -> [u8; 4] {
    if src[3] == 0 {
        return [0, 0, 0, 0];
    }
    const PALETTE: &[(f32, [u8; 3])] = &[
        (0.0, [0, 90, 0]),
        (5.0, [0, 126, 0]),
        (10.0, [8, 127, 219]),
        (15.0, [28, 71, 232]),
        (20.0, [110, 13, 198]),
        (25.0, [200, 15, 134]),
        (30.0, [192, 100, 135]),
        (35.0, [210, 136, 59]),
        (40.0, [250, 196, 49]),
        (45.0, [254, 251, 2]),
        (50.0, [254, 154, 88]),
        (55.0, [254, 95, 5]),
        (60.0, [253, 52, 28]),
        (65.0, [190, 190, 190]),
    ];
    let mut min_error = u32::MAX;
    let mut dbz = 0.0;
    for (value, rgb) in PALETTE {
        let err = (src[0] as i32 - rgb[0] as i32).pow(2)
            + (src[1] as i32 - rgb[1] as i32).pow(2)
            + (src[2] as i32 - rgb[2] as i32).pow(2);
        if (err as u32) < min_error {
            min_error = err as u32;
            dbz = *value;
        }
    }
    let [r, g, b] = super::RADAR_BANDS
        .iter()
        .rev()
        .find(|(lowest, _)| dbz >= *lowest)
        .map_or(super::RADAR_BANDS[0].1, |(_, rgb)| *rgb);
    [r, g, b, src[3].min(200)]
}

#[cfg(test)]
mod radar_frame_tests {
    use super::radar_frame_index;

    /// Two hours of frames, on the ten minutes as RainViewer's are.
    fn times() -> Vec<i64> {
        (0..13).map(|k| 1_795_099_800 + k * 600).collect()
    }

    /// The step the app asks for at a time (`super::super::radar_step`).
    fn step(unix: i64) -> Option<i64> {
        Some(unix.div_euclid(600) * 600)
    }

    #[test]
    fn the_newest_frame_while_live() {
        assert_eq!(radar_frame_index(&times(), None), Some(12));
        assert_eq!(radar_frame_index(&[], None), None);
    }

    #[test]
    fn the_frame_measured_in_the_wanted_ten_minutes() {
        let times = times();
        // A time in a frame's ten minutes, at its start or later: that frame.
        assert_eq!(radar_frame_index(&times, step(times[4])), Some(4));
        assert_eq!(radar_frame_index(&times, step(times[4] + 599)), Some(4));
        assert_eq!(radar_frame_index(&times, step(times[4] + 600)), Some(5));
        // After the newest: the newest; before the oldest: the oldest.
        assert_eq!(radar_frame_index(&times, step(times[12] + 7_200)), Some(12));
        assert_eq!(radar_frame_index(&times, step(times[0] - 7_200)), Some(0));
    }
}
