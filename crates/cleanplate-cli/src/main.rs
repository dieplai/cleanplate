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
    /// Output file (.mov defaults to ProRes 422 HQ)
    output: PathBuf,
    /// auto | prores | av1 | h264 | ffv1
    #[arg(long, default_value = "auto")]
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
    let a = Args::parse();
    if let Some(t) = a.threads {
        rayon::ThreadPoolBuilder::new()
            .num_threads(t)
            .build_global()?;
    }
    if cleanplate_cli::still_io::is_still(&a.input) {
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
