//! ffprobe / ffmpeg subprocess pipes: probe, decode to raw yuv420p frames, and the encode command.
use anyhow::{anyhow, bail, Context, Result};
use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, Stdio};

#[derive(Clone, Debug)]
pub struct Probe {
    pub w: usize,
    pub h: usize,
    pub fps: String,
    pub has_audio: bool,
    /// color_range, color_space, color_primaries, color_transfer
    pub color: [Option<String>; 4],
    pub bitrate: u64,
}

/// How to get ffmpeg on this OS (shown when it is missing).
pub fn install_hint() -> &'static str {
    if cfg!(windows) {
        "install it with `winget install --id Gyan.FFmpeg -e`, then open a new terminal"
    } else if cfg!(target_os = "macos") {
        "install it with `brew install ffmpeg`"
    } else {
        "install it with your package manager, e.g. `sudo apt install ffmpeg` or `sudo dnf install ffmpeg-free`"
    }
}

fn spawn_err(prog: &str, e: std::io::Error) -> anyhow::Error {
    if e.kind() == std::io::ErrorKind::NotFound {
        anyhow!(
            "`{prog}` was not found on PATH: video needs ffmpeg - {}",
            install_hint()
        )
    } else {
        anyhow!("running {prog}: {e}")
    }
}

pub fn probe(path: &Path) -> Result<Probe> {
    let o = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_streams",
            "-show_format",
            "-of",
            "json",
        ])
        .arg(path)
        .output()
        .map_err(|e| spawn_err("ffprobe", e))?;
    if !o.status.success() {
        bail!(
            "ffprobe failed on {}: {}",
            path.display(),
            String::from_utf8_lossy(&o.stderr)
        );
    }
    let j: serde_json::Value = serde_json::from_slice(&o.stdout)?;
    let streams = j["streams"].as_array().context("no streams")?;
    let v = streams
        .iter()
        .find(|s| s["codec_type"] == "video")
        .context("no video stream")?;
    let s = |k: &str| v[k].as_str().map(str::to_string);
    let num = |x: &serde_json::Value| x.as_str().and_then(|s| s.parse::<u64>().ok());
    let (mut w, mut h) = (
        v["width"].as_u64().context("width")? as usize,
        v["height"].as_u64().context("height")? as usize,
    );
    // ffmpeg decodes rotated (phone) videos upright, so the frames are h×w
    let rotation = v["side_data_list"]
        .as_array()
        .and_then(|l| l.iter().find_map(|d| d["rotation"].as_f64()))
        .or_else(|| v["tags"]["rotate"].as_str().and_then(|r| r.parse().ok()))
        .unwrap_or(0.0);
    if (rotation.abs() as i64) % 180 == 90 {
        (w, h) = (h, w);
    }
    Ok(Probe {
        w,
        h,
        fps: s("r_frame_rate").unwrap_or_else(|| "24/1".into()),
        has_audio: streams.iter().any(|s| s["codec_type"] == "audio"),
        color: [
            s("color_range"),
            s("color_space"),
            s("color_primaries"),
            s("color_transfer"),
        ],
        bitrate: num(&v["bit_rate"])
            .or_else(|| num(&j["format"]["bit_rate"]))
            .unwrap_or(8_000_000),
    })
}

/// Raw yuv420p frames (w·h·3/2 bytes each) from ffmpeg.
pub struct Decoder {
    child: Child,
    frame: usize,
}

impl Decoder {
    pub fn open(path: &Path, w: usize, h: usize) -> Result<Self> {
        let child = Command::new("ffmpeg")
            .args(["-v", "error", "-i"])
            .arg(path)
            .args(["-f", "rawvideo", "-pix_fmt", "yuv420p", "-"])
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|e| spawn_err("ffmpeg", e))?;
        Ok(Decoder {
            child,
            frame: w * h * 3 / 2,
        })
    }
}

impl Iterator for Decoder {
    type Item = Vec<u8>;
    fn next(&mut self) -> Option<Vec<u8>> {
        let mut buf = vec![0u8; self.frame];
        let out = self.child.stdout.as_mut()?;
        match out.read_exact(&mut buf) {
            Ok(()) => Some(buf),
            Err(_) => {
                let _ = self.child.wait();
                None
            }
        }
    }
}

/// Encoder names this ffmpeg build offers (`ffmpeg -encoders`, second column).
pub fn encoders() -> Vec<String> {
    Command::new("ffmpeg")
        .args(["-hide_banner", "-encoders"])
        .output()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter_map(|l| l.split_whitespace().nth(1).map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// The ffmpeg encoder behind each `--codec` value.
pub const CODECS: [(&str, &str); 5] = [
    ("h264x", "libx264"),
    ("av1", "libsvtav1"),
    ("h264", "libopenh264"),
    ("prores", "prores_ks"),
    ("ffv1", "ffv1"),
];

/// Resolve `--codec` against what this ffmpeg build offers. `auto`: ProRes for .mov, FFV1 for .mkv, otherwise the
/// first available of libx264 (CRF 12), SVT-AV1 (CRF 14, 10-bit), OpenH264 (3× source bitrate).
pub fn pick_codec(req: &str, out: &Path, has: impl Fn(&str) -> bool) -> Result<String> {
    let ext = out
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let enc = |c: &str| CODECS.iter().find(|(k, _)| *k == c).map(|(_, e)| *e);
    if req != "auto" {
        let e = enc(req).with_context(|| format!("unknown codec {req}"))?;
        if !has(e) {
            bail!("this ffmpeg build has no {e} encoder for --codec {req}, use --codec auto");
        }
        return Ok(req.into());
    }
    let order: &[&str] = match ext.as_str() {
        "mov" => &["prores", "h264x", "av1", "h264"],
        "mkv" => &["ffv1"],
        _ => &["h264x", "av1", "h264"],
    };
    order
        .iter()
        .find(|c| has(enc(c).unwrap()))
        .map(|c| c.to_string())
        .with_context(|| {
            format!(
                "this ffmpeg build has none of the encoders for .{ext} ({}). Install a full ffmpeg build or write .mkv",
                order.iter().map(|c| enc(c).unwrap()).collect::<Vec<_>>().join(", ")
            )
        })
}

/// Full ffmpeg argument list (without the program name) that reads raw yuv420p frames on stdin, takes the audio of
/// `inp` unchanged, encodes with `codec` (prores | av1 | h264x | h264 | ffv1) and copies the known colour tags.
/// An output file name containing ".crop." is the research mode: lossless FFV1 of the bottom-right 384×256 only.
pub fn encoder_args(codec: &str, inp: &Path, out: &Path, p: &Probe) -> Vec<String> {
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    let mut venc = match codec {
        "prores" => s(&[
            "-c:v",
            "prores_ks",
            "-profile:v",
            "3",
            "-pix_fmt",
            "yuv422p10le",
            "-vendor",
            "apl0",
        ]),
        "av1" => s(&[
            "-c:v",
            "libsvtav1",
            "-crf",
            "14",
            "-preset",
            "5",
            "-svtav1-params",
            "tune=0",
            "-pix_fmt",
            "yuv420p10le",
        ]),
        "h264x" => s(&[
            "-c:v", "libx264", "-crf", "12", "-preset", "slow", "-pix_fmt", "yuv420p",
        ]),
        "h264" => {
            let br = (3 * p.bitrate).max(12_000_000).to_string();
            s(&["-c:v", "libopenh264", "-b:v", &br, "-pix_fmt", "yuv420p"])
        }
        _ => s(&["-c:v", "ffv1", "-level", "3", "-pix_fmt", "yuv420p"]),
    };
    let research = out
        .file_name()
        .is_some_and(|f| f.to_string_lossy().contains(".crop."));
    if research && p.w >= 384 && p.h >= 256 {
        let crop = format!("crop=384:256:{}:{}", p.w - 384, p.h - 256);
        venc = s(&["-vf", &crop, "-c:v", "ffv1", "-pix_fmt", "yuv420p"]);
    }
    let mut tags = vec![];
    for (val, opt) in p.color.iter().zip([
        "-color_range",
        "-colorspace",
        "-color_primaries",
        "-color_trc",
    ]) {
        if let Some(v) = val {
            if v != "unknown" {
                tags.push(opt.to_string());
                tags.push(v.clone());
            }
        }
    }
    let mut a = s(&[
        "-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", "yuv420p", "-s",
    ]);
    a.push(format!("{}x{}", p.w, p.h));
    a.push("-r".into());
    a.push(p.fps.clone());
    if tags.first().map(|t| t == "-color_range").unwrap_or(false) {
        a.extend(tags[..2].iter().cloned()); // input range, as the reference does
    }
    a.extend(s(&["-i", "-", "-i"]));
    a.push(inp.to_string_lossy().into_owned());
    // audio and container metadata of the source are carried over unchanged
    a.extend(s(&[
        "-map",
        "0:v",
        "-map",
        "1:a?",
        "-map_metadata",
        "1",
        "-c:a",
        "copy",
    ]));
    a.extend(venc);
    a.extend(tags);
    a.push(out.to_string_lossy().into_owned());
    a
}

#[cfg(test)]
mod tests {
    use super::pick_codec;
    use std::path::Path;

    #[test]
    fn auto_codec_follows_extension_and_available_encoders() {
        let only = |names: &'static [&'static str]| move |e: &str| names.contains(&e);
        let all = only(&["libx264", "libsvtav1", "libopenh264", "prores_ks", "ffv1"]);
        assert_eq!(
            pick_codec("auto", Path::new("a.mp4"), all).unwrap(),
            "h264x"
        );
        assert_eq!(
            pick_codec("auto", Path::new("a.MOV"), all).unwrap(),
            "prores"
        );
        assert_eq!(pick_codec("auto", Path::new("a.mkv"), all).unwrap(), "ffv1");
        // Fedora's ffmpeg-free: no libx264
        assert_eq!(
            pick_codec("auto", Path::new("a.mp4"), only(&["libsvtav1", "ffv1"])).unwrap(),
            "av1"
        );
        assert_eq!(
            pick_codec("auto", Path::new("a.mp4"), only(&["libopenh264"])).unwrap(),
            "h264"
        );
        assert!(pick_codec("auto", Path::new("a.mp4"), only(&["ffv1"])).is_err());
        assert!(pick_codec("h264x", Path::new("a.mp4"), only(&["libsvtav1"])).is_err());
        assert_eq!(pick_codec("av1", Path::new("a.mp4"), all).unwrap(), "av1");
    }
}
