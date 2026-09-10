mod audio_render;

use staveloom_core::parser::load_and_parse;
use staveloom_core::renderer::Renderer;
use std::env;
use std::fs;
use std::path::Path;

const LICENSE_NOTICE: &str = "\
staveloom is dual-licensed under MIT OR Apache-2.0.
See LICENSE-MIT / LICENSE-APACHE (repo root) for the full license text.

This binary also incorporates third-party open-source components. Most
notably: SoundFont synthesis via rustysynth (MIT), and, only when the
--audio flag is used, MP3 encoding via the system's libmp3lame library
(LGPL-2.0, dynamically linked at runtime — not bundled with this binary).
See docs/THIRDPARTY_LICENSE.md for the full dependency list and review.";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    if args.contains(&"--license".to_string()) {
        println!("{}", LICENSE_NOTICE);
        return Ok(());
    }
    if args.len() < 2 {
        println!("Usage: staveloom <input.musicxml/.mxl> [OPTIONS]");
        println!("Options:");
        println!("  --output <file.svg>   Output SVG file path");
        println!("  --metadata <file.json> Output metadata JSON file path");
        println!("  --audio <file.mp3>    Output MP3 file path");
        println!("  --midi <file.mid>     Output MIDI file path");
        println!("  --sf2 <file.sf2>      SoundFont file path for audio rendering");
        println!("  --json                Show parsed JSON model");
        println!("  --list-parts          List all part IDs in the score");
        println!("  --parts P1,P2...      Render only specified part IDs");
        println!("  --width <width>       Page width (default: 1200)");
        println!("  --horizontal          Render as a single long line");
        println!("  --elastic             Use elastic layout (duration-based spacing)");
        println!("  --mobile              Use mobile layout (tight packing for small screens)");
        println!("  --license             Show license and third-party notices");
        return Ok(());
    }

    let file_path = &args[1];
    let show_json = args.contains(&"--json".to_string());
    let list_parts = args.contains(&"--list-parts".to_string());
    let elastic_mode = args.contains(&"--elastic".to_string());
    let mobile_mode = args.contains(&"--mobile".to_string());

    let mut output_path = None;
    if let Some(pos) = args.iter().position(|x| x == "--output") {
        if pos + 1 < args.len() {
            output_path = Some(args[pos + 1].clone());
        }
    }

    let mut metadata_path = None;
    if let Some(pos) = args.iter().position(|x| x == "--metadata") {
        if pos + 1 < args.len() {
            metadata_path = Some(args[pos + 1].clone());
        }
    }

    let mut audio_path = None;
    if let Some(pos) = args.iter().position(|x| x == "--audio") {
        if pos + 1 < args.len() {
            audio_path = Some(args[pos + 1].clone());
        }
    }

    let mut midi_path = None;
    if let Some(pos) = args.iter().position(|x| x == "--midi") {
        if pos + 1 < args.len() {
            midi_path = Some(args[pos + 1].clone());
        }
    }

    let mut sf2_path = None;
    if let Some(pos) = args.iter().position(|x| x == "--sf2") {
        if pos + 1 < args.len() {
            sf2_path = Some(args[pos + 1].clone());
        }
    }

    let mut filter_parts = None;
    if let Some(pos) = args.iter().position(|x| x == "--parts") {
        if pos + 1 < args.len() {
            let parts_str = &args[pos + 1];
            filter_parts = Some(
                parts_str
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .collect::<Vec<String>>(),
            );
        }
    }

    let renderer = Renderer::default();
    let mut page_width = renderer.page_width;
    if args.contains(&"--horizontal".to_string()) {
        page_width = None;
    } else if let Some(pos) = args.iter().position(|x| x == "--width") {
        if pos + 1 < args.len() {
            if let Ok(w) = args[pos + 1].parse::<f32>() {
                page_width = Some(w);
            }
        }
    }

    match load_and_parse(Path::new(file_path)) {
        Ok(score) => {
            if show_json {
                println!("{}", serde_json::to_string_pretty(&score)?);
            } else if list_parts {
                println!("Parts in {}:", file_path);
                for item in &score.part_list {
                    if let staveloom_core::models::PartListItem::Part { id, name, .. } = item {
                        println!(
                            "  - ID: {:<5} | Name: {}",
                            id,
                            name.as_deref().unwrap_or("Unknown")
                        );
                    }
                }
            } else {
                println!("Successfully parsed MusicXML: {}", file_path);

                let mut score = score;
                if let Some(target_ids) = &filter_parts {
                    score.filter_parts(target_ids);
                }

                let mut renderer = Renderer::default();
                renderer.page_width = page_width;
                if elastic_mode {
                    renderer.spacing_strategy = staveloom_core::renderer::SpacingStrategy::Elastic;
                } else {
                    renderer.spacing_strategy = staveloom_core::renderer::SpacingStrategy::Compact;
                }
                if mobile_mode {
                    renderer.apply_mobile_preset();
                }
                let (svg_content, metadata) = renderer.render_with_metadata(&score);

                let out_file = output_path.unwrap_or_else(|| {
                    let path = Path::new(file_path);
                    let stem = path.file_stem().unwrap().to_str().unwrap();
                    format!("{}.svg", stem)
                });

                fs::write(&out_file, svg_content)?;
                println!("SVG rendered to: {}", out_file);

                if let Some(meta_out) = metadata_path {
                    let meta_json = serde_json::to_string_pretty(&metadata)?;
                    fs::write(&meta_out, meta_json)?;
                    println!("Metadata exported to: {}", meta_out);
                }

                if audio_path.is_some() || midi_path.is_some() {
                    let timeline = staveloom_core::timeline::TimelineSolver::solve(&score);

                    if let Some(midi_out) = midi_path {
                        println!("Generating MIDI file...");
                        let smf = staveloom_core::midi_engine::MidiEngine::generate_smf(&score, &timeline);
                        smf.save(&midi_out)?;
                        println!("MIDI rendered to: {}", midi_out);
                    }

                    if let Some(audio_out) = audio_path {
                        if let Some(sf2) = sf2_path {
                            println!("Rendering audio with SoundFont: {}...", sf2);
                            let smf =
                                staveloom_core::midi_engine::MidiEngine::generate_smf(&score, &timeline);
                            let audio_renderer = audio_render::AudioRenderer::new(&sf2)?;
                            audio_renderer.render_to_mp3(&smf, &audio_out)?;
                            println!("Audio rendered to: {}", audio_out);
                        } else {
                            eprintln!(
                                "Warning: --audio specified but no --sf2 provided. Skipping audio rendering."
                            );
                        }
                    }
                }
            }
        }
        Err(e) => {
            eprintln!("Error parsing MusicXML: {}", e);
        }
    }

    Ok(())
}
