//! Decoding with a ceiling. Compressed bytes and image headers come from files and
//! servers GlobeCraft does not control; a few kilobytes can ask for gigabytes (a gzip
//! "bomb", a PNG header claiming 60 000 × 60 000 pixels). Every decoder of such bytes
//! goes through here and names the most it accepts.
use std::io::Read;

/// Gunzips `bytes`, refusing output beyond `max_bytes`.
pub(crate) fn gunzip(bytes: &[u8], max_bytes: u64) -> Result<Vec<u8>, crate::Error> {
    let mut out = Vec::new();
    // One byte more than allowed tells "exactly the limit" from "more than the limit".
    flate2::read::GzDecoder::new(bytes)
        .take(max_bytes.saturating_add(1))
        .read_to_end(&mut out)?;
    if out.len() as u64 > max_bytes {
        return Err(crate::Error::msg(format!(
            "compressed data unpacks to more than {} MB",
            max_bytes / 1_000_000
        )));
    }
    Ok(out)
}

/// The largest image a decoder accepts.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ImageLimit {
    /// Width and height may each be this many pixels at most.
    pub(crate) max_side_px: u32,
    /// The most memory the decoder may allocate for the image.
    pub(crate) max_decode_bytes: u64,
}

impl ImageLimit {
    /// A map tile or a layer's picture: map tiles are 256 to 1024 pixels wide, a
    /// layer's legend can be a tall strip. 4096 × 4096 RGBA is 64 MiB; twice that
    /// leaves room for 16-bit channels.
    pub(crate) const TILE: Self = Self {
        max_side_px: 4096,
        max_decode_bytes: 128 * 1024 * 1024,
    };
    /// A picture of a whole world: a planet's surface or sky the user opens, the
    /// downloaded cloud map. 16 384 × 8192 RGBA (512 MiB) is the largest texture
    /// common graphics cards take.
    pub(crate) const WORLD: Self = Self {
        max_side_px: 16_384,
        max_decode_bytes: 512 * 1024 * 1024,
    };
}

/// Decodes a PNG, JPEG or WebP image no larger than `limit`.
pub(crate) fn image(bytes: &[u8], limit: ImageLimit) -> Result<image::DynamicImage, crate::Error> {
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(limit.max_side_px);
    limits.max_image_height = Some(limit.max_side_px);
    limits.max_alloc = Some(limit.max_decode_bytes);
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format()?;
    reader.limits(limits);
    Ok(reader.decode()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn gzip(bytes: &[u8]) -> Vec<u8> {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(bytes).unwrap();
        encoder.finish().unwrap()
    }

    #[test]
    fn gunzip_stops_at_its_limit() {
        let packed = gzip(&[7; 1000]);
        assert_eq!(gunzip(&packed, 1000).unwrap(), vec![7; 1000]);
        assert!(gunzip(&packed, 999).is_err());
        // Ten megabytes of zeros pack into about ten kilobytes.
        let bomb = gzip(&vec![0; 10_000_000]);
        assert!(bomb.len() < 20_000);
        assert!(gunzip(&bomb, 1_000_000).is_err());
        assert!(gunzip(b"not gzip", 1000).is_err());
    }

    /// A PNG whose header claims `width` × `height` and whose pixel data is `rows`.
    fn png(width: u32, height: u32, rows: &[u8]) -> Vec<u8> {
        fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
            out.extend((data.len() as u32).to_be_bytes());
            let mut body = kind.to_vec();
            body.extend(data);
            let mut crc = flate2::Crc::new();
            crc.update(&body);
            out.extend(&body);
            out.extend(crc.sum().to_be_bytes());
        }
        let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
        let mut header = Vec::new();
        header.extend(width.to_be_bytes());
        header.extend(height.to_be_bytes());
        header.extend([8, 6, 0, 0, 0]);
        chunk(&mut out, b"IHDR", &header);
        let mut packed =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        packed.write_all(rows).unwrap();
        chunk(&mut out, b"IDAT", &packed.finish().unwrap());
        chunk(&mut out, b"IEND", &[]);
        out
    }

    #[test]
    fn images_beyond_their_limit_are_refused_before_they_are_decoded() {
        let small = png(2, 1, &[0, 10, 20, 30, 255, 40, 50, 60, 255]);
        let decoded = image(&small, ImageLimit::TILE).unwrap().into_rgba8();
        assert_eq!(decoded.dimensions(), (2, 1));
        assert_eq!(decoded.get_pixel(1, 0).0, [40, 50, 60, 255]);
        // A hundred bytes claiming 3.6 billion pixels: refused from the header alone.
        let huge = png(60_000, 60_000, &[0; 16]);
        assert!(huge.len() < 200);
        assert!(image(&huge, ImageLimit::TILE).is_err());
        assert!(image(&huge, ImageLimit::WORLD).is_err());
        // One side over the limit is enough.
        assert!(image(&png(4097, 1, &[0; 16]), ImageLimit::TILE).is_err());
        assert!(image(b"plainly not an image", ImageLimit::TILE).is_err());
    }
}
