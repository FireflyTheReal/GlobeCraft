//! Service-specific downloads and radar palette decoding.
use super::{CloudMap, RadarFrame};
use image::{GrayImage, RgbaImage, imageops::FilterType};
const CLOUD_SOURCE: &str = "https://clouds.matteason.co.uk/images/8192x4096/clouds-alpha.png";
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
    let epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        / (3 * 60 * 60);
    let url = format!("{CLOUD_SOURCE}?slot={epoch}");
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
        Ok(CloudMap { levels })
    })
    .await?
}

pub(super) async fn download_radar() -> Result<RadarFrame, crate::Error> {
    let client = crate::http::client(crate::http::Policy::Radar)?;
    let metadata = crate::http::body(client.get(RADAR_API), RADAR_METADATA_MAX_BYTES).await?;
    let json = crate::http::parse_json(metadata).await?;
    let frame = json
        .pointer("/radar/past")
        .and_then(|v| v.as_array())
        .and_then(|v| v.last())
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
