//! End-to-end live test against fal.ai — text-to-video by default,
//! image-to-video when an image URL is supplied.
//!
//! ⚠ **This costs real money.** Each invocation submits one job which fal
//! bills once accepted. Defaults are the cheapest possible run (480p × 4 s
//! standard tier ≈ $1.21 USD). The example **prompts for explicit `yes` on
//! stdin** before submitting so a mistyped command cannot drain credit.
//!
//! Reads the API key via [`playa_jobs::secret::lookup`] — checks env vars
//! first (`PLAYA_FAL_KEY`, `FAL_KEY`, `FAL_API_KEY`) then `.env` /  `../.env`.
//!
//! # Usage
//!
//! Text-to-video (no image):
//! ```bash
//! cargo run -p playa-job-seedance --example run_one -- \
//!     "a cyberpunk story of a red hood and wolf in a cybernetic future"
//! ```
//!
//! Image-to-video (image URL is positional 2):
//! ```bash
//! cargo run -p playa-job-seedance --example run_one -- \
//!     "<prompt>" "https://example.com/start.png"
//! ```
//!
//! Optional positional args after that: resolution (default `480p`),
//! duration seconds (default `4`).

use std::env;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::time::Duration;

use playa_job_seedance::params::SeedanceDuration;
use playa_job_seedance::{
    SeedanceEndpoint, SeedanceImageToVideoParams, SeedanceProvider, SeedanceTextToVideoParams,
};
use playa_jobs::{JobEvent, JobQueue, JobQueueConfig, JobState};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!(
            "Usage:\n\
             \tcargo run -p playa-job-seedance --example run_one -- \\\n\
             \t\t<PROMPT> [IMAGE_URL] [RESOLUTION] [DURATION]\n\n\
             ⚠ This calls the fal.ai API and bills your account.\n\
             Cheapest call: 480p × 4 s standard tier ≈ $1.21 USD.\n\
             API key sourced from env (PLAYA_FAL_KEY / FAL_KEY / FAL_API_KEY)\n\
             or `.env` in the current or parent directory.\n\n\
             Without IMAGE_URL → text-to-video. With IMAGE_URL → image-to-video.\n\
             Defaults: RESOLUTION=480p, DURATION=4s."
        );
        std::process::exit(2);
    }
    let prompt = args[1].clone();

    // The optional positional 2 may be either an image URL OR the resolution.
    // Heuristic: if it looks like an HTTP/HTTPS URL, treat it as the image_url
    // and shift remaining args. Otherwise it is the resolution and there is
    // no image (text-to-video).
    let mut idx = 2;
    let image_url = match args.get(idx) {
        Some(a) if a.starts_with("http://") || a.starts_with("https://") => {
            idx += 1;
            Some(a.clone())
        }
        _ => None,
    };
    let resolution = args.get(idx).cloned().unwrap_or_else(|| "480p".into());
    let duration_secs: u8 = args
        .get(idx + 1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(4);

    if !(4..=15).contains(&duration_secs) {
        eprintln!("Duration must be between 4 and 15 seconds (fal limit). Got {duration_secs}.");
        std::process::exit(2);
    }

    let endpoint = if image_url.is_some() {
        SeedanceEndpoint::ImageToVideo
    } else {
        SeedanceEndpoint::TextToVideo
    };

    let api_key = playa_jobs::secret::lookup(
        &["PLAYA_FAL_KEY", "FAL_KEY", "FAL_API_KEY"],
        &[PathBuf::from(".env"), PathBuf::from("../.env")],
    )
    .ok_or("FAL key not found. Set FAL_KEY or FAL_API_KEY in env or .env file.")?;
    let last4: String = api_key.chars().rev().take(4).collect::<String>().chars().rev().collect();
    println!(
        "Loaded fal.ai key (length={}, preview=fal_***{last4})",
        api_key.len()
    );

    // Cost estimate (standard tier, $0.3024–$0.3034 / second). Audio is free
    // per fal docs; we do not factor it in.
    let per_sec = match endpoint {
        SeedanceEndpoint::ImageToVideo => 0.3024,
        SeedanceEndpoint::TextToVideo => 0.3034,
    };
    let cost = per_sec * duration_secs as f64;
    println!("\nPlanned submission:");
    println!("  endpoint:   {:?}", endpoint);
    println!("  prompt:     {prompt}");
    if let Some(url) = &image_url {
        println!("  image_url:  {url}");
    }
    println!("  resolution: {resolution}");
    println!("  duration:   {duration_secs} s");
    println!("  estimated:  ${cost:.2} USD (standard tier)");
    print!("\nContinue? Type `yes` to submit: ");
    io::stdout().flush()?;

    let mut answer = String::new();
    io::stdin().lock().read_line(&mut answer)?;
    if answer.trim() != "yes" {
        eprintln!("Aborted (no submit, no charge).");
        std::process::exit(0);
    }

    let cfg = JobQueueConfig {
        thread_count: 1,
        files_dir: std::env::temp_dir().join("seedance-example"),
        // Disable persistence — this example runs one job, then exits.
        persist_path: None,
    };
    let queue = JobQueue::new(cfg)?;
    queue.register_provider(SeedanceProvider::new(endpoint, api_key));

    queue.subscribe(|ev| match &ev {
        JobEvent::Created(id) => println!("[event] created {id}"),
        JobEvent::StateChanged(id, st) => println!("[event] {id} state → {st:?}"),
        JobEvent::Progress(id, pr) => println!(
            "[event] {id} progress: {} {}",
            pr.stage,
            pr.message.as_deref().unwrap_or("")
        ),
        JobEvent::Completed(id, _) => println!("[event] {id} COMPLETED"),
        JobEvent::Failed(id, e) => eprintln!("[event] {id} FAILED: {e}"),
        JobEvent::Cancelled(id) => println!("[event] {id} CANCELLED"),
    });

    let body = match (endpoint, image_url) {
        (SeedanceEndpoint::ImageToVideo, Some(image_url)) => {
            let mut p = SeedanceImageToVideoParams::new(prompt, image_url);
            p.resolution = resolution;
            p.duration = SeedanceDuration::Seconds(duration_secs);
            p.into_json()
        }
        (SeedanceEndpoint::TextToVideo, _) => {
            let mut p = SeedanceTextToVideoParams::new(prompt);
            p.resolution = resolution;
            // text-to-video wants `duration` as a STRING per fal docs.
            p.duration = duration_secs.to_string();
            p.into_json()
        }
        (SeedanceEndpoint::ImageToVideo, None) => {
            unreachable!("image_to_video selected without image_url")
        }
    };

    let id = queue.submit(endpoint.kind(), body)?;
    println!("\nSubmitted: {id}\nWaiting for terminal state… (Ctrl-C to leave it running)\n");

    loop {
        std::thread::sleep(Duration::from_secs(1));
        let Some(job) = queue.get(id) else {
            eprintln!("Job vanished from queue?");
            break;
        };
        if job.state.is_terminal() {
            println!("\n=== final ===");
            println!("state:  {:?}", job.state);
            if let Some(r) = &job.result {
                println!("result: {}", serde_json::to_string_pretty(r)?);
            }
            if let Some(e) = &job.error {
                println!("error:  {e}");
            }
            if let Some(path) = job
                .result
                .as_ref()
                .and_then(|r| r.get("mp4_path"))
                .and_then(|v| v.as_str())
            {
                let exists = std::path::Path::new(path).is_file();
                let bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
                println!("mp4:    {path} (exists={exists}, bytes={bytes})");
            }
            if matches!(job.state, JobState::Complete) {
                println!(
                    "\nLeaving the file under {}",
                    std::env::temp_dir().join("seedance-example").display()
                );
            }
            break;
        }
    }

    queue.shutdown();
    Ok(())
}
