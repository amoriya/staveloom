use staveloom_core::auto_beam::auto_beam_score;
use staveloom_core::midi_engine::MidiEngine;
use staveloom_core::midi_parser::MidiParser;
use staveloom_core::models::PartListItem;
use staveloom_core::parser::parse_in_memory;
use staveloom_core::renderer::Renderer;
use staveloom_core::timeline::TimelineSolver;
use serde::Serialize;
use wasm_bindgen::prelude::*;

#[derive(Serialize)]
pub struct PartInfo {
    pub id: String,
    pub name: String,
}

#[derive(Serialize)]
pub struct InstrumentInfo {
    pub program: i32,
    pub name: String,
}

#[derive(Serialize)]
pub struct SystemSvg {
    pub index: usize,
    pub svg_content: String,
    pub y_offset: f32,
    pub height: f32,
    pub width: f32,
    pub measure_start: usize,
    pub measure_end: usize,
}

#[derive(Serialize)]
pub struct RenderResult {
    pub systems: Vec<SystemSvg>,
    pub metadata: staveloom_core::models::PlayMetadata,
    pub midi: Vec<u8>,
}

fn parse_dimensions_from_svg(svg: &str) -> (f32, f32) {
    let mut width = 1200.0;
    let mut height = 800.0;
    if let Some(pos) = svg.find("viewBox=\"") {
        let rest = &svg[pos + 9..];
        if let Some(end) = rest.find('\"') {
            let coords: Vec<&str> = rest[..end]
                .split(|c| c == ' ' || c == ',')
                .filter(|s| !s.is_empty())
                .collect();
            if coords.len() == 4 {
                if let Ok(w) = coords[2].parse::<f32>() {
                    width = w;
                }
                if let Ok(h) = coords[3].parse::<f32>() {
                    height = h;
                }
            }
        }
    }
    (width, height)
}

/// Extract <style>...</style> block from SVG string (for embedding in per-system SVGs)
fn extract_style(svg: &str) -> String {
    if let (Some(s), Some(e)) = (svg.find("<style>"), svg.find("</style>")) {
        svg[s..e + 8].to_string()
    } else {
        String::new()
    }
}

/// Extract the inner content of <g id="sN">...</g> from the full SVG string.
/// Returns the content between the opening and closing tags (not including them).
fn extract_system_group(svg: &str, idx: usize) -> String {
    let open = format!("<g id=\"s{}\">", idx);
    let Some(start) = svg.find(&open) else {
        return String::new();
    };
    let content_start = start + open.len();

    // Walk bytes counting <g> depth to find the matching </g>
    let bytes = svg.as_bytes();
    let mut depth: i32 = 1;
    let mut i = content_start;
    while i < bytes.len() && depth > 0 {
        if i + 1 < bytes.len() && bytes[i] == b'<' {
            if bytes[i + 1] == b'g' {
                let after = if i + 2 < bytes.len() { bytes[i + 2] } else { 0 };
                if after == b' ' || after == b'>' || after == b'\n' || after == b'\r' {
                    depth += 1;
                    i += 2;
                    continue;
                }
            } else if i + 3 < bytes.len() && &bytes[i..i + 4] == b"</g>" {
                depth -= 1;
                if depth == 0 {
                    break;
                }
                i += 4;
                continue;
            }
        }
        i += 1;
    }

    svg[content_start..i].to_string()
}

#[wasm_bindgen]
pub fn list_parts(file_bytes: &[u8]) -> Result<JsValue, JsError> {
    let score = parse_in_memory(file_bytes)?;
    let mut parts = Vec::new();
    for item in &score.part_list {
        if let PartListItem::Part { id, name, .. } = item {
            parts.push(PartInfo {
                id: id.clone(),
                name: name.as_deref().unwrap_or("Unknown").to_string(),
            });
        }
    }
    Ok(serde_wasm_bindgen::to_value(&parts)?)
}

#[wasm_bindgen]
pub fn list_instruments(file_bytes: &[u8]) -> Result<JsValue, JsError> {
    let score = parse_in_memory(file_bytes)?;
    let mut instruments = Vec::new();
    for item in &score.part_list {
        if let PartListItem::Part {
            id: _,
            name,
            instrument_names,
            midi_instruments,
            instrument_sound,
            ..
        } = item
        {
            let sound = instrument_sound.as_deref();
            let midi = midi_instruments.first();
            let name_str = name.as_deref();
            let program = MidiEngine::get_program_number(sound, midi, name_str, instrument_names);
            instruments.push(InstrumentInfo {
                program,
                name: name.as_deref().unwrap_or("Unknown").to_string(),
            });
        }
    }
    Ok(serde_wasm_bindgen::to_value(&instruments)?)
}

#[wasm_bindgen]
pub fn parse_and_render(
    file_bytes: &[u8],
    elastic: bool,
    mobile: bool,
    horizontal: bool,
    page_width: f32,
    filter_parts_csv: &str,
) -> Result<JsValue, JsError> {
    let mut score = parse_in_memory(file_bytes)?;
    auto_beam_score(&mut score);

    if !filter_parts_csv.trim().is_empty() {
        let targets: Vec<String> = filter_parts_csv
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if !targets.is_empty() {
            score.filter_parts(&targets);
        }
    }

    let mut renderer = Renderer::default();
    if horizontal {
        renderer.page_width = None;
    } else {
        renderer.page_width = Some(page_width);
    }

    if elastic {
        renderer.spacing_strategy = staveloom_core::renderer::SpacingStrategy::Elastic;
    } else {
        renderer.spacing_strategy = staveloom_core::renderer::SpacingStrategy::Compact;
    }
    if mobile {
        renderer.apply_mobile_preset();
    }

    let (svg_content, metadata) = renderer.render_with_metadata(&score);
    let measure_count = score.parts.first().map(|p| p.measures.len()).unwrap_or(0);

    let n_systems = metadata.system_boundaries.len();

    let systems: Vec<SystemSvg> = if n_systems <= 1 {
        // Single system (horizontal mode or very short score): return full SVG as-is
        let (width, height) = parse_dimensions_from_svg(&svg_content);
        vec![SystemSvg {
            index: 0,
            svg_content,
            y_offset: 0.0,
            height,
            width,
            measure_start: 0,
            measure_end: measure_count.saturating_sub(1),
        }]
    } else {
        // Multi-system: extract per-system SVGs using the <g id="sN"> groups
        let full_width = {
            let mut w = 1200.0f32;
            if let Some(vb_pos) = svg_content.find("viewBox=\"") {
                let rest = &svg_content[vb_pos + 9..];
                if let Some(end) = rest.find('"') {
                    let parts: Vec<&str> = rest[..end].split_whitespace().collect();
                    if parts.len() == 4 {
                        w = parts[2].parse().unwrap_or(1200.0);
                    }
                }
            }
            w
        };
        let style_tag = extract_style(&svg_content);

        metadata
            .system_boundaries
            .iter()
            .enumerate()
            .map(|(i, boundary)| {
                let inner = extract_system_group(&svg_content, i);
                let h = boundary.height.max(1.0);
                let sys_svg = format!(
                    r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 {} {} {}" width="{}" height="{}">{}<g id="s{}">{}</g></svg>"#,
                    boundary.y_start,
                    full_width,
                    h,
                    full_width,
                    h,
                    style_tag,
                    i,
                    inner
                );
                SystemSvg {
                    index: i,
                    svg_content: sys_svg,
                    y_offset: boundary.y_start,
                    height: h,
                    width: full_width,
                    measure_start: boundary.measure_start,
                    measure_end: boundary.measure_end,
                }
            })
            .collect()
    };

    let timeline = TimelineSolver::solve(&score);
    let smf = MidiEngine::generate_smf(&score, &timeline);
    let mut midi_bytes = Vec::new();
    smf.write(&mut midi_bytes).map_err(|e| JsError::new(e))?;

    let result = RenderResult {
        systems,
        metadata,
        midi: midi_bytes,
    };

    Ok(serde_wasm_bindgen::to_value(&result)?)
}

/// Parse a MIDI file and render it as a score.
///
/// List parts (tracks/channels) in a MIDI file.
#[wasm_bindgen]
pub fn list_midi_parts(file_bytes: &[u8]) -> Result<JsValue, JsError> {
    let score = MidiParser::parse(file_bytes)
        .map_err(|e| JsError::new(&format!("MIDI parse error: {e}")))?;
    let mut parts = Vec::new();
    for item in &score.part_list {
        if let PartListItem::Part { id, name, .. } = item {
            parts.push(PartInfo {
                id: id.clone(),
                name: name.as_deref().unwrap_or("Unknown").to_string(),
            });
        }
    }
    Ok(serde_wasm_bindgen::to_value(&parts)?)
}

/// Returns a [`RenderResult`] identical to [`parse_and_render`], except:
/// - The score is built from MIDI event data (quantization, pitch conversion, score assembly)
/// - The `midi` field is regenerated from the quantized Score (same as MusicXML path) so that
///   SpessaSynth playback timing is perfectly in sync with the beat metadata.
#[wasm_bindgen]
pub fn parse_midi_and_render(
    file_bytes: &[u8],
    elastic: bool,
    mobile: bool,
    horizontal: bool,
    page_width: f32,
    filter_parts_csv: &str,
) -> Result<JsValue, JsError> {
    let mut score = MidiParser::parse(file_bytes)
        .map_err(|e| JsError::new(&format!("MIDI parse error: {e}")))?;

    if !filter_parts_csv.trim().is_empty() {
        let targets: Vec<String> = filter_parts_csv
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if !targets.is_empty() {
            score.filter_parts(&targets);
        }
    }

    let mut renderer = Renderer::default();
    if horizontal {
        renderer.page_width = None;
    } else {
        renderer.page_width = Some(page_width);
    }
    if elastic {
        renderer.spacing_strategy = staveloom_core::renderer::SpacingStrategy::Elastic;
    } else {
        renderer.spacing_strategy = staveloom_core::renderer::SpacingStrategy::Compact;
    }
    if mobile {
        renderer.apply_mobile_preset();
    }

    let (svg_content, metadata) = renderer.render_with_metadata(&score);
    let measure_count = score.parts.first().map(|p| p.measures.len()).unwrap_or(0);
    let n_systems = metadata.system_boundaries.len();

    let systems: Vec<SystemSvg> = if n_systems <= 1 {
        let (width, height) = parse_dimensions_from_svg(&svg_content);
        vec![SystemSvg {
            index: 0,
            svg_content,
            y_offset: 0.0,
            height,
            width,
            measure_start: 0,
            measure_end: measure_count.saturating_sub(1),
        }]
    } else {
        let full_width = {
            let mut w = 1200.0f32;
            if let Some(vb_pos) = svg_content.find("viewBox=\"") {
                let rest = &svg_content[vb_pos + 9..];
                if let Some(end) = rest.find('"') {
                    let parts: Vec<&str> = rest[..end].split_whitespace().collect();
                    if parts.len() == 4 {
                        w = parts[2].parse().unwrap_or(1200.0);
                    }
                }
            }
            w
        };
        let style_tag = extract_style(&svg_content);

        metadata
            .system_boundaries
            .iter()
            .enumerate()
            .map(|(i, boundary)| {
                let inner = extract_system_group(&svg_content, i);
                let h = boundary.height.max(1.0);
                let sys_svg = format!(
                    r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 {} {} {}" width="{}" height="{}">{}<g id="s{}">{}</g></svg>"#,
                    boundary.y_start,
                    full_width,
                    h,
                    full_width,
                    h,
                    style_tag,
                    i,
                    inner
                );
                SystemSvg {
                    index: i,
                    svg_content: sys_svg,
                    y_offset: boundary.y_start,
                    height: h,
                    width: full_width,
                    measure_start: boundary.measure_start,
                    measure_end: boundary.measure_end,
                }
            })
            .collect()
    };

    // Generate MIDI from the quantized Score so that SpessaSynth's playback
    // timing exactly matches the beat metadata (time_seconds) computed from the
    // same Score. Using the original bytes would cause sync drift wherever note
    // positions differ from the quantized score.
    let timeline = TimelineSolver::solve(&score);
    let smf = MidiEngine::generate_smf(&score, &timeline);
    let mut midi_bytes = Vec::new();
    smf.write(&mut midi_bytes).map_err(|e| JsError::new(e))?;

    let result = RenderResult {
        systems,
        metadata,
        midi: midi_bytes,
    };

    Ok(serde_wasm_bindgen::to_value(&result)?)
}
