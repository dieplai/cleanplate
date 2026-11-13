//! Still images: read (PNG / JPEG / WebP, any channel layout), remove the mark, write in the output's format.
use anyhow::{Context, Result};
use cleanplate_core::still::remove_still;
use std::path::Path;

pub const EXTS: [&str; 4] = ["png", "jpg", "jpeg", "webp"];

pub fn is_still(p: &Path) -> bool {
    p.extension()
        .map(|e| EXTS.contains(&e.to_string_lossy().to_lowercase().as_str()))
        .unwrap_or(false)
}

/// Exit code: 0 done, 2 refused (nothing written).
pub fn run(inp: &Path, out: &Path, lam: f32) -> Result<i32> {
    let img = image::open(inp).with_context(|| format!("reading {}", inp.display()))?;
    let has_alpha = img.color().has_alpha();
    let rgba = img.to_rgba8();
    let (w, h) = (rgba.width() as usize, rgba.height() as usize);
    let rgb: Vec<u8> = rgba.pixels().flat_map(|p| [p[0], p[1], p[2]]).collect();
    let ext = |p: &Path| {
        p.extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default()
    };
    let lossy = matches!(ext(inp).as_str(), "jpg" | "jpeg" | "webp");
    let r = match remove_still(&rgb, w, h, lossy, lam) {
        Ok(r) => r,
        Err(e) => {
            println!(
                "no mark found with confidence (ncc {:.2} < {}) — image left untouched",
                e.best_ncc, e.floor
            );
            return Ok(2);
        }
    };
    for l in &r.log {
        println!("{l}");
    }
    let mut outimg = rgba;
    for (i, p) in outimg.pixels_mut().enumerate() {
        p[0] = r.rgb[i * 3];
        p[1] = r.rgb[i * 3 + 1];
        p[2] = r.rgb[i * 3 + 2];
    }
    match ext(out).as_str() {
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
    println!("done -> {}", out.display());
    Ok(0)
}
