use anyhow::{bail, Result};
use clap::Parser;
use cleanplate_cli::batch::{self, Done, Job, Status};
use cleanplate_cli::video_io::VideoOpts;
use cleanplate_cli::{still_io, video_io, Outcome};
use cleanplate_core::video::Params;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Remove the visible generator mark (Google Veo text, Gemini diamond) from videos and stills, leaving a clean plate.
/// Visible marks only: SynthID and C2PA are not targeted (re-encoding cannot keep a C2PA signature valid).
///
/// One file:    cleanplate IN OUT
/// Many files:  cleanplate IN... -o DIR      (files and folders, folders are searched recursively)
#[derive(Parser)]
#[command(version, about, verbatim_doc_comment)]
struct Args {
    /// One input and its output file, or (with -o) any number of videos, stills and folders
    #[arg(required = true)]
    inputs: Vec<PathBuf>,
    /// Write every result into this folder as <name>_clean.<ext>, keeping sub-folders. Existing results are
    /// skipped (so an interrupted run resumes), see --overwrite
    #[arg(short, long)]
    out_dir: Option<PathBuf>,
    /// With -o: the container for videos
    #[arg(long, default_value = "mp4", value_parser = ["mp4", "mov", "mkv"])]
    format: String,
    /// With -o: the format for stills (PNG is lossless and recommended)
    #[arg(long, default_value = "png", value_parser = ["png", "jpg", "webp"])]
    image_format: String,
    /// With -o: files cleaned at the same time (0 = automatic from cores and memory)
    #[arg(short, long, default_value_t = 0)]
    jobs: usize,
    /// Replace outputs that already exist
    #[arg(long)]
    overwrite: bool,
    /// With -o: print every file's log, not only its result
    #[arg(short, long)]
    verbose: bool,
    /// Video encoder. auto picks from the output extension and what your ffmpeg offers: h264x = libx264 CRF 12,
    /// av1 = SVT-AV1 CRF 14 10-bit, h264 = OpenH264, prores = ProRes 422 HQ, ffv1 = lossless
    #[arg(long, default_value = "auto", value_parser = ["auto", "h264x", "av1", "h264", "prores", "ffv1"])]
    codec: String,
    /// Temporal half-window (frames) on moving shots, static shots use 3×
    #[arg(long, default_value_t = 8)]
    k: usize,
    /// Detection floor away from a generator placement (the effective floor there is at least 0.75)
    #[arg(long, default_value_t = 0.5)]
    min_ncc: f32,
    /// TV prior strength, −1 = automatic (per shot for video, from the texture for lossy stills)
    #[arg(long, default_value_t = -1.0, allow_negative_numbers = true)]
    lam: f32,
    /// Disable the TV prior on the mark's edge ring
    #[arg(long)]
    no_edge_tv: bool,
    /// Treat the mark as opaque, support dilated by N px, −1 = automatic (Flow 1080p)
    #[arg(long, default_value_t = -1, allow_negative_numbers = true)]
    opaque: i32,
    /// For AI-upscaled downloads (one file only): a cleaned lower-resolution version of the same clip
    #[arg(long)]
    guide: Option<PathBuf>,
    /// Worker threads (default: all cores)
    #[arg(long)]
    threads: Option<usize>,
}

fn main() {
    match real_main() {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("error: {e:#}");
            eprintln!(
                "If this looks like a bug, please report it (no need to attach the file at first): \
                 https://github.com/dieplai/cleanplate/issues/new?template=bad_result.md"
            );
            std::process::exit(1);
        }
    }
}

/// Exit code: 0 done, 2 nothing written because no mark was found (one file), 1 an error.
fn real_main() -> Result<i32> {
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
    let vopts = VideoOpts {
        codec: a.codec.clone(),
        params: Params {
            k: a.k,
            lam: a.lam,
            edge_tv: !a.no_edge_tv,
            ..Params::default()
        },
        min_ncc: a.min_ncc,
        opaque: a.opaque,
        guide: a.guide.clone(),
    };
    match &a.out_dir {
        Some(dir) => run_batch(&a, dir, &vopts),
        None if a.inputs.len() == 2 && !a.inputs[1].is_dir() => run_one(&a, &vopts),
        None => bail!("several inputs need an output folder: cleanplate IN... -o DIR"),
    }
}

fn run_one(a: &Args, vopts: &VideoOpts) -> Result<i32> {
    let (inp, out) = (&a.inputs[0], &a.inputs[1]);
    if out.exists() && !a.overwrite {
        bail!("{} already exists, add --overwrite to replace it", out.display());
    }
    let mut print = |l: String| println!("{l}");
    let outcome = if still_io::is_still(inp) {
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
        if !still_io::is_still(out) {
            bail!("a still must be written as .png, .jpg or .webp, not {}", out.display());
        }
        still_io::clean(inp, out, a.lam, &mut print)?
    } else {
        video_io::clean(inp, out, vopts, &mut print)?
    };
    Ok(match outcome {
        Outcome::Cleaned => 0,
        Outcome::Refused(why) => {
            println!("{why}");
            2
        }
    })
}

fn run_batch(a: &Args, dir: &Path, vopts: &VideoOpts) -> Result<i32> {
    if a.guide.is_some() {
        bail!("--guide works with one file only");
    }
    let jobs = batch::plan(&a.inputs, dir, &a.format, &a.image_format)?;
    let cores = std::thread::available_parallelism().map_or(4, |n| n.get());
    let workers = if a.jobs > 0 { a.jobs } else { batch::auto_workers(&jobs, cores, ram_gb()) };
    let videos = jobs.iter().filter(|j| j.video).count();
    println!(
        "Cleaning {} files ({videos} videos, {} stills) into {}, {workers} at a time",
        jobs.len(),
        jobs.len() - videos,
        dir.display()
    );
    let t0 = Instant::now();
    let n = jobs.len();
    let width = n.to_string().len();
    let clean = |j: &Job, log: &mut dyn FnMut(String)| {
        if j.video {
            video_io::clean(&j.input, &j.output, vopts, log)
        } else {
            still_io::clean(&j.input, &j.output, a.lam, log)
        }
    };
    let print = |k: usize, d: &Done| {
        let (word, extra) = match &d.status {
            Status::Cleaned => ("cleaned ", String::new()),
            Status::Refused(w) => ("no mark ", format!("  {w}")),
            Status::Skipped => ("skipped ", "  already there".into()),
            Status::Failed(e) => ("FAILED  ", format!("  {e}")),
        };
        let mut s = format!(
            "[{k:>width$}/{n}] {word}{} -> {} ({:.1} s){extra}",
            d.job.input.display(),
            d.job.output.display(),
            d.secs
        );
        if a.verbose {
            for l in &d.log {
                s.push_str(&format!("\n        {l}"));
            }
        }
        println!("{s}");
    };
    let done = batch::run(&jobs, workers, a.overwrite, &clean, &print);
    let secs = t0.elapsed().as_secs_f32();
    let count = |f: fn(&Status) -> bool| done.iter().filter(|d| f(&d.status)).count();
    let failed = count(|s| matches!(s, Status::Failed(_)));
    println!(
        "\nDone in {}: {} cleaned, {} without a mark, {} skipped, {failed} failed",
        human(secs),
        count(|s| matches!(s, Status::Cleaned)),
        count(|s| matches!(s, Status::Refused(_))),
        count(|s| matches!(s, Status::Skipped)),
    );
    let report = dir.join("cleanplate-report.json");
    std::fs::write(&report, batch::report_json(&done, secs))?;
    println!("Report: {}", report.display());
    Ok(if failed > 0 { 1 } else { 0 })
}

fn human(secs: f32) -> String {
    let s = secs.round() as u64;
    if s >= 60 {
        format!("{}m {:02}s", s / 60, s % 60)
    } else {
        format!("{secs:.1} s")
    }
}

/// Available memory in GB where the OS tells us cheaply (Linux), to bound the files in flight.
fn ram_gb() -> Option<f64> {
    let m = std::fs::read_to_string("/proc/meminfo").ok()?;
    let kb: f64 = m
        .lines()
        .find(|l| l.starts_with("MemAvailable:"))?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()?;
    Some(kb / 1048576.0)
}
