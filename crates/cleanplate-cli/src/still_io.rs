//! Still images: read (PNG / JPEG / WebP, any channel layout), remove the mark, write in the output's format.
use crate::{finish, partial_path, Outcome};
use anyhow::{Context, Result};
use cleanplate_core::still::remove_still;
use std::path::Path;

pub const EXTS: [&str; 4] = ["png", "jpg", "jpeg", "webp"];

pub fn is_still(p: &Path) -> bool {
    p.extension()
        .map(|e| EXTS.contains(&e.to_string_lossy().to_lowercase().as_str()))
        .unwrap_or(false)
}

/// Clean one still into `out` (its extension picks the format). `log` receives the progress lines.
pub fn clean(inp: &Path, out: &Path, lam: f32, log: &mut dyn FnMut(String)) -> Result<Outcome> {
    // the format comes from the content, not the extension (a JPEG saved as .png is common)
    let reader = image::ImageReader::open(inp)
        .with_context(|| format!("opening {}", inp.display()))?
        .with_guessed_format()?;
    let fmt = reader.format();
    let img = reader
        .decode()
        .with_context(|| format!("reading {}", inp.display()))?;
    let has_alpha = img.color().has_alpha();
    if img.color().bytes_per_pixel() > img.color().channel_count() {
        log(format!(
            "note: {} has 16-bit channels, the result is saved with 8-bit channels",
            inp.display()
        ));
    }
    let rgba = img.to_rgba8();
    let (w, h) = (rgba.width() as usize, rgba.height() as usize);
    let rgb: Vec<u8> = rgba.pixels().flat_map(|p| [p[0], p[1], p[2]]).collect();
    let ext = |p: &Path| {
        p.extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default()
    };
    let lossy = match fmt {
        Some(image::ImageFormat::Png) => false,
        Some(image::ImageFormat::Jpeg) => true,
        // WebP may be lossless or lossy; treat it as lossy like the reference
        _ => matches!(ext(inp).as_str(), "jpg" | "jpeg" | "webp"),
    };
    let r = match remove_still(&rgb, w, h, lossy, lam) {
        Ok(r) => r,
        Err(e) => return Ok(Outcome::Refused(format!("{e} - image left untouched"))),
    };
    for l in &r.log {
        log(l.clone());
    }
    let mut outimg = rgba;
    for (i, p) in outimg.pixels_mut().enumerate() {
        p[0] = r.rgb[i * 3];
        p[1] = r.rgb[i * 3 + 1];
        p[2] = r.rgb[i * 3 + 2];
    }
    let tmp = partial_path(out);
    let written = write(outimg, has_alpha, &ext(out), &tmp);
    if let Err(e) = written.and_then(|_| finish(&tmp, out).map_err(Into::into)) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.context(format!("writing {}", out.display())));
    }
    log(format!("done -> {}", out.display()));
    Ok(Outcome::Cleaned)
}

fn write(outimg: image::RgbaImage, has_alpha: bool, ext: &str, out: &Path) -> Result<()> {
    match ext {
        "jpg" | "jpeg" => {
            // jpeg-encoder at q95 4:2:0 (the reference's OpenCV settings); measured 0.17 dB closer to libjpeg than the
            // image crate's encoder on textured content. PNG output is lossless and recommended.
            let rgb8 = image::DynamicImage::ImageRgba8(outimg).to_rgb8();
            let mut e = jpeg_encoder::Encoder::new_file(out, 95)?;
            e.set_sampling_factor(jpeg_encoder::SamplingFactor::R_4_2_0);
            e.encode(
                rgb8.as_raw(),
                rgb8.width() as u16,
                rgb8.height() as u16,
                jpeg_encoder::ColorType::Rgb,
            )?;
        }
        _ if has_alpha => outimg.save(out)?,
        _ => image::DynamicImage::ImageRgba8(outimg)
            .to_rgb8()
            .save(out)?,
    }
    Ok(())
}
