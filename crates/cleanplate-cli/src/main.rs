use anyhow::Result;
use clap::Parser;
use cleanplate_cli::video_io::{run, VideoOpts};
use cleanplate_core::video::Params;
use std::path::PathBuf;

/// Remove the visible generator mark (Google Veo text, Gemini diamond) from a video or a still, leaving a clean plate.
/// Visible marks only: SynthID and C2PA are not targeted (re-encoding cannot keep a C2PA signature valid).
#[derive(Parser)]
#[command(version, about)]
struct Args {
    /// Input video or still (.png .jpg .webp); as downloaded — native resolution gives the best result
    input: PathBuf,
    /// Output file. Video: .mp4 (H.264 / AV1), .mov (ProRes 422 HQ), .mkv (lossless FFV1). Still: .png (lossless,
    /// recommended), .jpg (q95), .webp (lossless)
    output: PathBuf,
    /// Video encoder. auto picks from the output extension and what your ffmpeg offers: h264x = libx264 CRF 12,
    /// av1 = SVT-AV1 CRF 14 10-bit, h264 = OpenH264, prores = ProRes 422 HQ, ffv1 = lossless
    #[arg(long, default_value = "auto", value_parser = ["auto", "h264x", "av1", "h264", "prores", "ffv1"])]
    codec: String,
    /// Temporal half-window (frames) on moving shots; static shots use 3×
    #[arg(long, default_value_t = 8)]
    k: usize,
    /// Detection floor away from a generator placement (the effective floor there is at least 0.75)
    #[arg(long, default_value_t = 0.5)]
    min_ncc: f32,
    /// TV prior strength; −1 = automatic (per shot for video, from the texture for lossy stills)
    #[arg(long, default_value_t = -1.0, allow_negative_numbers = true)]
    lam: f32,
    /// Disable the TV prior on the mark's edge ring
    #[arg(long)]
    no_edge_tv: bool,
    /// Treat the mark as opaque, support dilated by N px; −1 = automatic (Flow 1080p)
    #[arg(long, default_value_t = -1, allow_negative_numbers = true)]
    opaque: i32,
    /// For AI-upscaled downloads: a cleaned lower-resolution version of the same clip
    #[arg(long)]
    guide: Option<PathBuf>,
    /// Worker threads (default: all cores)
    #[arg(long)]
    threads: Option<usize>,
}

fn main() -> Result<()> {
    // usage errors exit 1: exit code 2 means "no mark found, nothing written"
    let a = Args::try_parse().unwrap_or_else(|e| {
        let _ = e.print();
        std::process::exit(if e.use_stderr() { 1 } else { 0 })
    });
    if let Some(t) = a.threads {
        rayon::ThreadPoolBuilder::new()
            .num_threads(t)
            .build_global()?;
    }
    if cleanplate_cli::still_io::is_still(&a.input) {
        let video_only = [
            ("--codec", a.codec != "auto"),
            ("--k", a.k != 8),
            ("--min-ncc", a.min_ncc != 0.5),
            ("--no-edge-tv", a.no_edge_tv),
            ("--opaque", a.opaque != -1),
            ("--guide", a.guide.is_some()),
        ];
        for (flag, _) in video_only.iter().filter(|(_, set)| *set) {
            eprintln!("note: {flag} applies to video only and is ignored for stills");
        }
        if !cleanplate_cli::still_io::is_still(&a.output) {
            anyhow::bail!(
                "a still must be written as .png, .jpg or .webp, not {}",
                a.output.display()
            );
        }
        std::process::exit(cleanplate_cli::still_io::run(&a.input, &a.output, a.lam)?);
    }
    let params = Params {
        k: a.k,
        lam: a.lam,
        edge_tv: !a.no_edge_tv,
        ..Params::default()
    };
    let o = VideoOpts {
        codec: a.codec,
        params,
        min_ncc: a.min_ncc,
        opaque: a.opaque,
        guide: a.guide,
    };
    let code = run(&a.input, &a.output, &o)?;
    std::process::exit(code);
}
