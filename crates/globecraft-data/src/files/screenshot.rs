//! Screenshots saved under a free name, never overwriting: whole pictures on the file
//! worker, and large ones streamed strip by strip from where they are rendered.
use super::FileOutcome;
use std::{
    fs::{File, OpenOptions},
    io::{BufWriter, ErrorKind, Write},
    path::{Path, PathBuf},
};

pub(super) fn save(
    path: PathBuf,
    width: u32,
    height: u32,
    pixels: &[u8],
    credits: Option<&str>,
) -> Result<FileOutcome, crate::Error> {
    let (file, candidate) = create_unique(&path)?;
    let mut writer = BufWriter::new(file);
    if let Some(credits) = credits {
        let mut encoder = png::Encoder::new(&mut writer, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.add_itxt_chunk("Source".into(), credits.into())?;
        let mut png = encoder.write_header()?;
        png.write_image_data(pixels)?;
        png.finish()?;
    } else {
        image::write_buffer_with_format(
            &mut writer,
            pixels,
            width,
            height,
            image::ColorType::Rgba8,
            image::ImageFormat::Png,
        )?;
    }
    writer.flush()?;
    Ok(FileOutcome::ScreenshotSaved { path: candidate })
}

/// Creates `path`, or `path` with `_1`, `_2`… before the extension when it exists:
/// a screenshot never overwrites another.
fn create_unique(path: &Path) -> Result<(File, PathBuf), crate::Error> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut candidate = path.to_path_buf();
    let mut suffix = 0_u64;
    loop {
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => return Ok((file, candidate)),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                suffix += 1;
                let mut name = path
                    .file_stem()
                    .ok_or("Screenshot path has no filename")?
                    .to_os_string();
                name.push(format!("_{suffix}"));
                if let Some(extension) = path.extension() {
                    name.push(".");
                    name.push(extension);
                }
                candidate = path.with_file_name(name);
            }
            Err(error) => return Err(error.into()),
        }
    }
}

/// A PNG written strip by strip, top to bottom: pictures larger than memory should
/// hold at once (a Pro Max screenshot is 8× the view). Dropped before `finish`, the
/// partial file is deleted.
pub struct PngStrips {
    stream: Option<png::StreamWriter<'static, BufWriter<File>>>,
    path: PathBuf,
}

impl PngStrips {
    /// Starts `width`×`height` RGBA at a free name next to `path`, with `credits` as
    /// the image's source text.
    pub fn create(
        path: &Path,
        width: u32,
        height: u32,
        credits: &str,
    ) -> Result<Self, crate::Error> {
        let (file, path) = create_unique(path)?;
        let started = (|| -> Result<_, crate::Error> {
            let mut encoder = png::Encoder::new(BufWriter::new(file), width, height);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder.add_itxt_chunk("Source".into(), credits.into())?;
            Ok(encoder.write_header()?.into_stream_writer()?)
        })();
        match started {
            Ok(stream) => Ok(Self {
                stream: Some(stream),
                path,
            }),
            Err(error) => {
                let _ = std::fs::remove_file(&path);
                Err(error)
            }
        }
    }
    /// The file being written.
    pub fn path(&self) -> &Path {
        &self.path
    }
    /// Appends whole rows of RGBA pixels.
    pub fn write_rows(&mut self, rows: &[u8]) -> Result<(), crate::Error> {
        let stream = self.stream.as_mut().ok_or("Screenshot already finished")?;
        stream.write_all(rows)?;
        Ok(())
    }
    /// Completes the file once every row was written.
    pub fn finish(mut self) -> Result<PathBuf, crate::Error> {
        let stream = self.stream.take().ok_or("Screenshot already finished")?;
        let finished = (|| -> Result<(), crate::Error> {
            stream.finish()?;
            Ok(())
        })();
        match finished {
            Ok(()) => Ok(std::mem::take(&mut self.path)),
            Err(error) => {
                let _ = std::fs::remove_file(&self.path);
                Err(error)
            }
        }
    }
}

impl Drop for PngStrips {
    fn drop(&mut self) {
        if self.stream.take().is_some() {
            // Unfinished (cancelled or failed): remove the partial picture.
            let _ = std::fs::remove_file(&self.path);
        }
    }
}
