//! End-to-end checks of the cleanplate binary on tiny generated clips (skipped when ffmpeg is absent).
use cleanplate_cli::ffmpeg::{encoder_args, Probe};
use std::path::{Path, PathBuf};
use std::process::Command;

fn have_ffmpeg() -> bool {
    Command::new("ffmpeg").arg("-version").output().is_ok()
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("cleanplate-test-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d.join(name)
}

#[test]
fn encode_args_without_audio_or_tags() {
    let p = Probe { w: 640, h: 360, fps: "24/1".into(), has_audio: false, color: [None, None, None, None], bitrate: 1_000_000 };
    let a = encoder_args("av1", Path::new("in.mp4"), Path::new("out.mp4"), &p).join(" ");
    assert!(!a.contains("-color_range"), "{a}");
    assert!(a.contains("-map 1:a?"), "{a}");
    let p2 = Probe { color: [Some("unknown".into()), None, None, None], ..p };
    assert!(!encoder_args("ffv1", Path::new("in.mp4"), Path::new("o.mkv"), &p2).join(" ").contains("unknown"));
}

/// A 48-frame 640x360 testsrc2 clip, lossless; when `mark` the 720p Veo glyph is composited at its placement.
fn make_clip(path: &Path, mark: bool) {
    let raw = Command::new("ffmpeg")
        .args(["-v", "error", "-f", "lavfi", "-i", "testsrc2=size=640x360:rate=24:duration=2", "-f", "rawvideo", "-pix_fmt", "yuv420p", "-"])
        .output()
        .unwrap()
        .stdout;
    let mut raw = raw;
    if mark {
        let a = &cleanplate_core::marks::by_name("veo-small-720p.png").alpha;
        let (fw, fh, fs) = (640, 360, 640 * 360 * 3 / 2);
        let (x0, y0) = (fw - 16 - a.w, fh - 16 - a.h);
        for f in raw.chunks_mut(fs) {
            for y in 0..a.h {
                for x in 0..a.w {
                    let i = (y0 + y) * fw + x0 + x;
                    let al = a.at(x, y);
                    f[i] = ((1.0 - al) * f[i] as f32 + al * 235.0).round() as u8;
                }
            }
        }
    }
    let mut c = Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", "yuv420p", "-s", "640x360", "-r", "24", "-i", "-", "-c:v", "ffv1"])
        .arg(path)
        .stdin(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write;
    c.stdin.take().unwrap().write_all(&raw).unwrap();
    assert!(c.wait().unwrap().success());
}

fn frames(path: &Path) -> usize {
    let o = Command::new("ffprobe")
        .args(["-v", "error", "-count_frames", "-select_streams", "v:0", "-show_entries", "stream=nb_read_frames", "-of", "csv=p=0"])
        .arg(path)
        .output()
        .unwrap();
    String::from_utf8_lossy(&o.stdout).trim().parse().unwrap()
}

#[test]
fn video_roundtrip_small() {
    if !have_ffmpeg() {
        return;
    }
    let (inp, out) = (tmp("marked.mkv"), tmp("clean.mkv"));
    make_clip(&inp, true);
    let st = Command::new(env!("CARGO_BIN_EXE_cleanplate")).arg(&inp).arg(&out).args(["--codec", "ffv1"]).status().unwrap();
    assert!(st.success());
    assert_eq!(frames(&out), 48);
}

#[test]
fn video_refuses_clean() {
    if !have_ffmpeg() {
        return;
    }
    let (inp, out) = (tmp("clean_in.mkv"), tmp("clean_out.mkv"));
    make_clip(&inp, false);
    let st = Command::new(env!("CARGO_BIN_EXE_cleanplate")).arg(&inp).arg(&out).args(["--codec", "ffv1"]).status().unwrap();
    assert_eq!(st.code(), Some(2));
    assert!(!out.exists());
}
