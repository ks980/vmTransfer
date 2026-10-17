//! Offline/live native driver; exit 0=success, 2=incomplete, 1=fatal error.
use std::{
    fs,
    path::{Path, PathBuf},
};
use vmtransfer::{
    capture::{png::PngSequence, FrameSource},
    codec::decode_bitmap,
    session::{ReceiveSession, ReceiveState},
    Error,
};

fn main() {
    match run() {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
fn run() -> Result<i32, Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|s| s == "--help" || s == "-h") {
        println!("{}", usage());
        return Ok(0);
    }
    if args
        .first()
        .is_some_and(|s| s == "snapshot" || s == "receive")
    {
        return run_live(&args);
    }
    if args.len() != 5
        || args[0] != "decode"
        || args[1] != "--manifest"
        || args[3] != "--output-dir"
    {
        return Err(usage().into());
    }
    let manifest = Path::new(&args[2]);
    if fs::metadata(manifest)?.len() > 16 * 1024 * 1024 {
        return Err("manifest exceeds 16 MiB".into());
    }
    let paths: Vec<String> = serde_json::from_slice(&fs::read(manifest)?)?;
    if paths.len() > 1_000_000 {
        return Err("too many image paths".into());
    }
    let base = manifest.parent().unwrap_or(Path::new("."));
    let paths = paths.into_iter().map(|p| base.join(p)).collect();
    let destination = PathBuf::from(&args[4]);
    fs::create_dir_all(&destination)?;
    let mut source = PngSequence::new(paths);
    receive(&mut source, &destination, "offline processing only", true)
}

fn usage() -> &'static str {
    "usage:\n  vmtransfer decode --manifest images.json --output-dir DIRECTORY\n  vmtransfer snapshot --output capture.png [--source window|monitor|any]\n  vmtransfer receive --output-dir DIRECTORY [--source window|monitor|any]\nLive commands require --features live-capture and user consent in the GNOME chooser."
}

fn receive(
    source: &mut impl FrameSource,
    destination: &Path,
    scope: &str,
    verbose: bool,
) -> Result<i32, Box<dyn std::error::Error>> {
    let mut session = ReceiveSession::new(destination);
    let mut last_progress = std::time::Instant::now();
    let mut reported_format = false;
    while session.is_consuming() {
        let Some(captured) = source.next_frame()? else {
            break;
        };
        session.metrics.counters.images += 1;
        match decode_bitmap(captured.bitmap.view()?) {
            Ok(frame) => match session.ingest(&frame) {
                Ok(event) => {
                    if !reported_format {
                        if let Some((profile, mode)) = session.visual_format() {
                            let label = if mode == 24 {
                                "RGB888".to_owned()
                            } else {
                                format!("{mode} colors")
                            };
                            eprintln!(
                                "detected: {}×{} matrix, {label}",
                                profile.matrix_width, profile.matrix_height
                            );
                            reported_format = true;
                        }
                    }
                    if verbose
                        || !matches!(
                            event,
                            vmtransfer::session::SessionEvent::Duplicate
                                | vmtransfer::session::SessionEvent::BlockAccepted(_)
                        )
                    {
                        eprintln!("{event:?}");
                    }
                }
                Err(Error::Invalid(message)) => eprintln!("discarded: {message}"),
                Err(e) => return Err(e.into()),
            },
            Err(Error::Crc) => session.metrics.counters.crc_failures += 1,
            Err(Error::Locate) => session.metrics.counters.locator_failures += 1,
            Err(Error::Palette) => session.metrics.counters.palette_failures += 1,
            Err(Error::Invalid(_)) => session.metrics.counters.invalid_frames += 1,
            Err(e) => return Err(e.into()),
        }
        if !verbose && last_progress.elapsed() >= std::time::Duration::from_secs(1) {
            let snapshot = session.metrics.snapshot();
            let c = snapshot.counters;
            eprintln!("{}/{} blocks | unique {:.1}/s | duplicates {} | CRC {} | locate {} | color {} | {:.0} bytes/s | {:.1}s",
                c.unique_blocks,c.total_blocks,snapshot.unique_blocks_per_second,c.duplicates,c.crc_failures,
                c.locator_failures,c.palette_failures,snapshot.goodput_bytes_per_second,snapshot.elapsed_seconds);
            last_progress = std::time::Instant::now();
        }
    }
    source.stop()?;
    let complete = matches!(session.state(), ReceiveState::Complete(_));
    let format = session.visual_format().map(|(profile, mode)| {
        serde_json::json!({"matrix_width":profile.matrix_width,
            "matrix_height":profile.matrix_height,"color_mode":mode})
    });
    println!(
        "{}",
        serde_json::json!({"complete":complete,"state":format!("{:?}",session.state()),
        "rate_scope":scope,"visual_format":format,"metrics":session.metrics.snapshot()})
    );
    Ok(if complete { 0 } else { 2 })
}

#[cfg(not(feature = "live-capture"))]
fn run_live(_args: &[String]) -> Result<i32, Box<dyn std::error::Error>> {
    Err("live capture is disabled; build with cargo build --manifest-path receiver/Cargo.toml --features live-capture".into())
}

#[cfg(feature = "live-capture")]
fn run_live(args: &[String]) -> Result<i32, Box<dyn std::error::Error>> {
    use vmtransfer::capture::{
        png::save_png,
        portal::{AuthorizedCapture, CaptureTarget, PortalCapture},
    };
    let mut target = CaptureTarget::WindowOrMonitor;
    let mut path = None;
    let expected = if args[0] == "snapshot" {
        "--output"
    } else {
        "--output-dir"
    };
    let mut index = 1;
    while index < args.len() {
        let value = args.get(index + 1).ok_or_else(usage)?;
        match args[index].as_str() {
            "--source" => {
                target = match value.as_str() {
                    "window" => CaptureTarget::Window,
                    "monitor" => CaptureTarget::Monitor,
                    "any" => CaptureTarget::WindowOrMonitor,
                    _ => return Err("--source must be window, monitor or any".into()),
                }
            }
            flag if flag == expected && path.is_none() => path = Some(PathBuf::from(value)),
            _ => return Err(usage().into()),
        }
        index += 2;
    }
    let path = path.ok_or_else(usage)?;
    if args[0] == "receive" {
        fs::create_dir_all(&path)?;
    } else {
        if path.symlink_metadata().is_ok() {
            return Err("snapshot output already exists; choose a new path".into());
        }
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent)?;
        }
    }
    eprintln!("Select the VNC window or monitor in the GNOME screen-sharing chooser. Waiting for your selection…");
    let mut source = futures_lite::future::block_on(PortalCapture::authorize(target))?;
    let result = if args[0] == "snapshot" {
        let frame = source
            .next_frame()?
            .ok_or("capture stopped before the first image")?;
        save_png(&path, &frame.bitmap)?;
        println!(
            "{}",
            serde_json::json!({"snapshot":path,"width":frame.bitmap.width,"height":frame.bitmap.height})
        );
        source.stop()?;
        Ok(0)
    } else {
        receive(
            &mut source,
            &path,
            "live capture; elapsed includes processing and final hash",
            false,
        )
    };
    eprintln!("capture stats: {}", serde_json::to_string(&source.stats())?);
    result
}
