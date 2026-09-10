use staveloom_core::parser::load_and_parse;
use staveloom_core::renderer::Renderer;
use std::fs;
use std::path::PathBuf;
use walkdir::WalkDir;

// ── MusicXML snapshot tests ───────────────────────────────────────────────────

#[test]
fn test_all_samples_json_snapshot() {
    run_snapshots("json", |score| serde_json::to_string_pretty(score).unwrap());
}

#[test]
fn test_all_samples_svg_snapshot() {
    let renderer = Renderer::default();
    run_snapshots("svg", |score| renderer.render(score));
}

#[test]
fn test_all_samples_mobile_svg_snapshot() {
    let renderer = Renderer::mobile();
    run_snapshots("mobile.svg", |score| renderer.render(score));
}

#[test]
fn test_all_samples_metadata_snapshot() {
    let renderer = Renderer::default();
    run_snapshots("metadata", |score| {
        let (_, metadata) = renderer.render_with_metadata(score);
        serde_json::to_string_pretty(&metadata).unwrap()
    });
}

#[test]
fn test_all_samples_timeline_snapshot() {
    run_snapshots("timeline", |score| {
        let timeline = staveloom_core::timeline::TimelineSolver::solve(score);
        serde_json::to_string_pretty(&timeline).unwrap()
    });
}

#[test]
fn test_all_samples_debug_svg_snapshot() {
    let mut renderer = Renderer::default();
    renderer.debug_metadata = true;
    run_snapshots("debug.svg", |score| renderer.render(score));
}

#[test]
fn test_all_samples_midi_snapshot() {
    run_snapshots_binary("mid", |score| {
        let timeline = staveloom_core::timeline::TimelineSolver::solve(score);
        let smf = staveloom_core::midi_engine::MidiEngine::generate_smf(score, &timeline);
        let mut buffer = Vec::new();
        smf.write(&mut buffer).unwrap();
        buffer
    });
}

// ── MIDI-input snapshot tests ─────────────────────────────────────────────────

#[test]
fn test_all_midi_samples_svg_snapshot() {
    let renderer = Renderer::default();
    run_midi_snapshots("svg", |score| renderer.render(score));
}

#[test]
fn test_all_midi_samples_json_snapshot() {
    run_midi_snapshots("json", |score| serde_json::to_string_pretty(score).unwrap());
}

#[test]
fn test_all_midi_samples_metadata_snapshot() {
    let renderer = Renderer::default();
    run_midi_snapshots("metadata", |score| {
        let (_, metadata) = renderer.render_with_metadata(score);
        serde_json::to_string_pretty(&metadata).unwrap()
    });
}

#[test]
fn test_all_midi_samples_debug_svg_snapshot() {
    let mut renderer = Renderer::default();
    renderer.debug_metadata = true;
    run_midi_snapshots("debug.svg", |score| renderer.render(score));
}

#[test]
fn test_all_midi_samples_midi_snapshot() {
    run_midi_snapshots_binary("mid", |score| {
        let timeline = staveloom_core::timeline::TimelineSolver::solve(score);
        let smf = staveloom_core::midi_engine::MidiEngine::generate_smf(score, &timeline);
        let mut buffer = Vec::new();
        smf.write(&mut buffer).unwrap();
        buffer
    });
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn run_snapshots<F>(extension: &str, transform: F)
where
    F: Fn(&staveloom_core::models::Score) -> String,
{
    run_snapshots_internal(extension, extension, |score| transform(score).into_bytes());
}

fn run_snapshots_binary<F>(extension: &str, transform: F)
where
    F: Fn(&staveloom_core::models::Score) -> Vec<u8>,
{
    run_snapshots_internal("midi", extension, transform);
}

fn run_snapshots_internal<F>(folder_name: &str, extension: &str, transform: F)
where
    F: Fn(&staveloom_core::models::Score) -> Vec<u8>,
{
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let samples_dir = manifest_dir.join("../../tests/samples");
    let snapshots_base_dir = manifest_dir.join("../../tests/snapshots").join(folder_name);

    if !snapshots_base_dir.exists() {
        fs::create_dir_all(&snapshots_base_dir).unwrap();
    }

    let filter = std::env::var("TEST_FILTER").ok();

    for entry in WalkDir::new(&samples_dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        let ext = path.extension().and_then(|s| s.to_str());

        if path.is_file() && (ext == Some("xml") || ext == Some("mxl")) {
            let relative_path = path.strip_prefix(&samples_dir).unwrap();

            if let Some(ref f) = filter {
                if !relative_path.to_string_lossy().contains(f) {
                    continue;
                }
            }

            let mut snapshot_path = snapshots_base_dir.join(relative_path);
            snapshot_path.set_extension(extension);

            fs::create_dir_all(snapshot_path.parent().unwrap()).unwrap();

            let score = match load_and_parse(path) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("Failed to parse {}: {}", relative_path.display(), e);
                    continue;
                }
            };

            let current_output = transform(&score);

            println!(
                "Updating {} snapshot for {}",
                folder_name.to_uppercase(),
                relative_path.display()
            );
            fs::write(&snapshot_path, current_output).unwrap();
        }
    }
}

/// Walk `tests/samples/` for `.mid` files, parse each with MidiParser, and
/// write the transformed output to `tests/snapshots/{folder_name}/`.
/// The snapshot file keeps the same base name with the extension replaced by
/// `folder_name` (e.g. `midi/foo.mid` → `svg/midi/foo.svg`).
fn run_midi_snapshots<F>(folder_name: &str, transform: F)
where
    F: Fn(&staveloom_core::models::Score) -> String,
{
    use staveloom_core::midi_parser::MidiParser;

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let samples_dir = manifest_dir.join("../../tests/samples");
    let snapshots_base_dir = manifest_dir.join("../../tests/snapshots").join(folder_name);

    if !snapshots_base_dir.exists() {
        fs::create_dir_all(&snapshots_base_dir).unwrap();
    }

    let filter = std::env::var("TEST_FILTER").ok();

    for entry in WalkDir::new(&samples_dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        let ext = path.extension().and_then(|s| s.to_str());

        if !path.is_file() || ext != Some("mid") {
            continue;
        }

        let relative_path = path.strip_prefix(&samples_dir).unwrap();

        if let Some(ref f) = filter {
            if !relative_path.to_string_lossy().contains(f) {
                continue;
            }
        }

        let mut snapshot_path = snapshots_base_dir.join(relative_path);
        snapshot_path.set_extension(folder_name);

        fs::create_dir_all(snapshot_path.parent().unwrap()).unwrap();

        let bytes = match fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("Failed to read {}: {}", relative_path.display(), e);
                continue;
            }
        };

        let score = match MidiParser::parse(&bytes) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Failed to parse MIDI {}: {}", relative_path.display(), e);
                continue;
            }
        };

        let current_output = transform(&score);

        println!(
            "Updating {} snapshot for MIDI {}",
            folder_name.to_uppercase(),
            relative_path.display()
        );
        fs::write(&snapshot_path, current_output.as_bytes()).unwrap();
    }
}

/// Same as `run_midi_snapshots` but for binary output (e.g. re-exported MIDI).
/// Saves to `tests/snapshots/midi/{subdir}/{name}.mid`.
fn run_midi_snapshots_binary<F>(extension: &str, transform: F)
where
    F: Fn(&staveloom_core::models::Score) -> Vec<u8>,
{
    use staveloom_core::midi_parser::MidiParser;

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let samples_dir = manifest_dir.join("../../tests/samples");
    let snapshots_base_dir = manifest_dir.join("../../tests/snapshots/midi");

    if !snapshots_base_dir.exists() {
        fs::create_dir_all(&snapshots_base_dir).unwrap();
    }

    let filter = std::env::var("TEST_FILTER").ok();

    for entry in WalkDir::new(&samples_dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        let ext = path.extension().and_then(|s| s.to_str());

        if !path.is_file() || ext != Some("mid") {
            continue;
        }

        let relative_path = path.strip_prefix(&samples_dir).unwrap();

        if let Some(ref f) = filter {
            if !relative_path.to_string_lossy().contains(f) {
                continue;
            }
        }

        let mut snapshot_path = snapshots_base_dir.join(relative_path);
        snapshot_path.set_extension(extension);

        fs::create_dir_all(snapshot_path.parent().unwrap()).unwrap();

        let bytes = match fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("Failed to read {}: {}", relative_path.display(), e);
                continue;
            }
        };

        let score = match MidiParser::parse(&bytes) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Failed to parse MIDI {}: {}", relative_path.display(), e);
                continue;
            }
        };

        let current_output = transform(&score);

        println!("Updating MIDI snapshot for {}", relative_path.display());
        fs::write(&snapshot_path, current_output).unwrap();
    }
}
