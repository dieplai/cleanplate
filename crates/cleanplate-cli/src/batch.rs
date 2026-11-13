//! Many files in one run: plan the outputs, clean them with a bounded number of files in flight, report.
//!
//! Production rules: an existing output is skipped unless `overwrite` (so an interrupted run resumes where it
//! stopped), one failing file never stops the others, a panic in one file is reported as its error, and every
//! output is written under a hidden partial name first (see [`crate::partial_path`]).
use crate::{still_io, Outcome};
use anyhow::{bail, Result};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Instant;

/// Video containers the batch picks up (decoded by ffmpeg).
pub const VIDEO_EXTS: [&str; 5] = ["mp4", "mov", "m4v", "mkv", "webm"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub input: PathBuf,
    pub output: PathBuf,
    pub video: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    Cleaned,
    Refused(String),
    /// The output was already there (and `overwrite` was off).
    Skipped,
    Failed(String),
}

#[derive(Debug, Clone)]
pub struct Done {
    pub job: Job,
    pub status: Status,
    pub secs: f32,
    pub log: Vec<String>,
}

fn ext_of(p: &Path) -> String {
    p.extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

fn is_video(p: &Path) -> bool {
    VIDEO_EXTS.contains(&ext_of(p).as_str())
}

/// Every supported file under `dir`, sorted, skipping hidden files (our own partial outputs among them).
fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)?.collect::<std::io::Result<_>>()?;
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let p = e.path();
        if e.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        if e.file_type()?.is_dir() {
            walk(&p, out)?;
        } else if is_video(&p) || still_io::is_still(&p) {
            out.push(p);
        }
    }
    Ok(())
}

/// The jobs for `inputs` (files and directories) written into `out_dir` as `<name>_clean.<ext>`, keeping each
/// directory's sub-folders. `video_ext` / `still_ext`: the output formats (e.g. "mp4", "png").
pub fn plan(inputs: &[PathBuf], out_dir: &Path, video_ext: &str, still_ext: &str) -> Result<Vec<Job>> {
    let mut jobs = vec![];
    let mut taken = HashSet::new();
    for inp in inputs {
        let files: Vec<(PathBuf, PathBuf)> = if inp.is_dir() {
            let mut v = vec![];
            walk(inp, &mut v)?;
            v.into_iter()
                .map(|f| {
                    let rel = f.strip_prefix(inp).unwrap_or(&f).parent().map(Path::to_path_buf);
                    (f, rel.unwrap_or_default())
                })
                .collect()
        } else if inp.is_file() {
            if !(is_video(inp) || still_io::is_still(inp)) {
                bail!("{} is not a supported video or image", inp.display());
            }
            vec![(inp.clone(), PathBuf::new())]
        } else {
            bail!("{} does not exist", inp.display());
        };
        for (input, sub) in files {
            let video = is_video(&input);
            let stem = input.file_stem().unwrap_or_default().to_string_lossy().into_owned();
            let ext = if video { video_ext } else { still_ext };
            let dir = out_dir.join(&sub);
            // two inputs with the same name from different places: number the second one
            let mut output = dir.join(format!("{stem}_clean.{ext}"));
            let mut k = 2;
            while !taken.insert(output.clone()) {
                output = dir.join(format!("{stem}_clean_{k}.{ext}"));
                k += 1;
            }
            jobs.push(Job { input, output, video });
        }
    }
    if jobs.is_empty() {
        bail!("no video or image found in the given inputs");
    }
    Ok(jobs)
}

/// Clean `jobs` with at most `workers` files in flight. `clean` does one file (its log lines go to the closure it
/// is given), `on_done` sees each result as it finishes (for progress output). Returns the results in job order.
pub fn run(
    jobs: &[Job],
    workers: usize,
    overwrite: bool,
    clean: &(dyn Fn(&Job, &mut dyn FnMut(String)) -> Result<Outcome> + Sync),
    on_done: &(dyn Fn(usize, &Done) + Sync),
) -> Vec<Done> {
    let next = AtomicUsize::new(0);
    let finished = AtomicUsize::new(0);
    let results: Mutex<Vec<Option<Done>>> = Mutex::new(vec![None; jobs.len()]);
    std::thread::scope(|sc| {
        for _ in 0..workers.clamp(1, jobs.len().max(1)) {
            sc.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::SeqCst);
                let Some(job) = jobs.get(i) else { break };
                let t0 = Instant::now();
                let mut log = vec![];
                let status = if job.output.exists() && !overwrite {
                    Status::Skipped
                } else {
                    let r = job
                        .output
                        .parent()
                        .map_or(Ok(()), std::fs::create_dir_all)
                        .map_err(anyhow::Error::from)
                        .and_then(|_| {
                            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                clean(job, &mut |l| log.push(l))
                            }))
                            .unwrap_or_else(|p| {
                                let msg = p
                                    .downcast_ref::<&str>()
                                    .map(|s| s.to_string())
                                    .or_else(|| p.downcast_ref::<String>().cloned())
                                    .unwrap_or_default();
                                bail!("internal error: {msg}")
                            })
                        });
                    match r {
                        Ok(Outcome::Cleaned) => Status::Cleaned,
                        Ok(Outcome::Refused(why)) => Status::Refused(why),
                        Err(e) => Status::Failed(format!("{e:#}")),
                    }
                };
                let done = Done {
                    job: job.clone(),
                    status,
                    secs: t0.elapsed().as_secs_f32(),
                    log,
                };
                on_done(finished.fetch_add(1, Ordering::SeqCst) + 1, &done);
                results.lock().unwrap()[i] = Some(done);
            });
        }
    });
    results.into_inner().unwrap().into_iter().flatten().collect()
}

/// Files in flight by default. One file already uses every core, so a second one mainly overlaps ffmpeg's decoding
/// and encoding with computation. Measured on 17 Veo/Gemini clips, 12 cores, MP4 out: 1 file at a time 245 s, 2 at a
/// time 198 s, 3 at a time 193 s, peak memory ~2.4 GB for the heaviest single clip. So: 2 on 8+ cores, bounded by
/// available memory at ~2.5 GB per file in flight.
pub fn auto_workers(jobs: &[Job], cores: usize, ram_gb: Option<f64>) -> usize {
    let by_cores = if cores >= 8 { 2 } else { 1 };
    let by_ram = ram_gb.map_or(by_cores, |g| ((g / 2.5) as usize).max(1));
    let stills_only = jobs.iter().all(|j| !j.video);
    let w = if stills_only { 2 } else { by_cores };
    w.min(by_ram).max(1)
}

/// Machine-readable record of a run, written next to the outputs.
pub fn report_json(done: &[Done], secs: f32) -> String {
    let esc = |s: &str| serde_json::to_string(s).unwrap();
    let items: Vec<String> = done
        .iter()
        .map(|d| {
            let (status, detail) = match &d.status {
                Status::Cleaned => ("cleaned", String::new()),
                Status::Refused(w) => ("no_mark", w.clone()),
                Status::Skipped => ("skipped", String::new()),
                Status::Failed(e) => ("failed", e.clone()),
            };
            format!(
                "{{\"input\":{},\"output\":{},\"status\":\"{status}\",\"detail\":{},\"seconds\":{:.1},\"log\":[{}]}}",
                esc(&d.job.input.to_string_lossy()),
                esc(&d.job.output.to_string_lossy()),
                esc(&detail),
                d.secs,
                d.log.iter().map(|l| esc(l)).collect::<Vec<_>>().join(",")
            )
        })
        .collect();
    format!(
        "{{\"version\":\"{}\",\"seconds\":{secs:.1},\"files\":[\n{}\n]}}\n",
        env!("CARGO_PKG_VERSION"),
        items.join(",\n")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(p: &Path) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, b"x").unwrap();
    }

    #[test]
    fn plan_names_outputs_and_keeps_sub_folders() {
        let d = std::env::temp_dir().join(format!("cleanplate-batch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        for f in ["in/a.mp4", "in/sub/b.PNG", "in/sub/.hidden.png", "in/notes.txt", "x/a.mp4"] {
            touch(&d.join(f));
        }
        let jobs = plan(&[d.join("in"), d.join("x/a.mp4")], &d.join("out"), "mp4", "png").unwrap();
        let outs: Vec<_> = jobs.iter().map(|j| j.output.strip_prefix(&d).unwrap().to_path_buf()).collect();
        assert_eq!(
            outs,
            [PathBuf::from("out/a_clean.mp4"), "out/sub/b_clean.png".into(), "out/a_clean_2.mp4".into()]
        );
        assert_eq!(jobs.iter().map(|j| j.video).collect::<Vec<_>>(), [true, false, true]);
        assert!(plan(&[d.join("in/notes.txt")], &d.join("out"), "mp4", "png").is_err());
        assert!(plan(&[d.join("missing")], &d.join("out"), "mp4", "png").is_err());
    }

    #[test]
    fn run_survives_failures_and_panics_and_skips_existing() {
        let d = std::env::temp_dir().join(format!("cleanplate-run-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        touch(&d.join("already_clean.png"));
        let job = |name: &str| Job {
            input: name.into(),
            output: d.join(format!("{name}_clean.png")),
            video: false,
        };
        let mut jobs: Vec<Job> = ["ok", "refused", "fails", "panics"].map(job).to_vec();
        jobs.push(Job { input: "x".into(), output: d.join("already_clean.png"), video: false });
        let clean = |j: &Job, log: &mut dyn FnMut(String)| -> Result<Outcome> {
            log(format!("working on {}", j.input.display()));
            match j.input.to_str().unwrap() {
                "ok" => Ok(Outcome::Cleaned),
                "refused" => Ok(Outcome::Refused("no mark".into())),
                "fails" => bail!("decoder broke"),
                _ => panic!("boom"),
            }
        };
        let seen = AtomicUsize::new(0);
        let done = run(&jobs, 3, false, &clean, &|_, _| {
            seen.fetch_add(1, Ordering::SeqCst);
        });
        assert_eq!(seen.load(Ordering::SeqCst), 5);
        let st: Vec<_> = done.iter().map(|d| d.status.clone()).collect();
        assert_eq!(st[0], Status::Cleaned);
        assert_eq!(st[1], Status::Refused("no mark".into()));
        assert!(matches!(&st[2], Status::Failed(e) if e.contains("decoder broke")));
        assert!(matches!(&st[3], Status::Failed(e) if e.contains("boom")));
        assert_eq!(st[4], Status::Skipped);
        assert_eq!(done[0].log, ["working on ok"]);
        let json = report_json(&done, 1.0);
        assert!(json.contains("\"status\":\"no_mark\"") && json.contains("decoder broke"));
    }
}
