//! ffprobe / ffmpeg subprocess pipes: probe, decode to raw yuv420p frames, and the encode command.
use anyhow::{bail, Context, Result};
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
        .context("running ffprobe (is ffmpeg installed?)")?;
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
    Ok(Probe {
        w: v["width"].as_u64().context("width")? as usize,
        h: v["height"].as_u64().context("height")? as usize,
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
            .context("running ffmpeg")?;
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

/// Whether this ffmpeg build has an encoder (e.g. "libx264").
pub fn has_encoder(name: &str) -> bool {
    Command::new("ffmpeg")
        .args(["-hide_banner", "-encoders"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains(name))
        .unwrap_or(false)
}

/// Full ffmpeg argument list (without the program name) that reads raw yuv420p frames on stdin, takes the audio of
/// `inp` unchanged, encodes with `codec` (prores | av1 | h264x | h264 | ffv1) and copies the known colour tags.
/// An output path containing ".crop." is the research mode: lossless FFV1 of the bottom-right 384×256 only.
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
    if out.to_string_lossy().contains(".crop.") {
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
    a.extend(s(&["-map", "0:v", "-map", "1:a?", "-map_metadata", "1", "-c:a", "copy"]));
    a.extend(venc);
    a.extend(tags);
    a.push(out.to_string_lossy().into_owned());
    a
}
