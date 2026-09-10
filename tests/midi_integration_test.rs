use staveloom_core::midi_parser::MidiParser;
use staveloom_core::models::MeasureElement;
use staveloom_core::renderer::Renderer;

/// Helper: bytes for a minimal Format 1 MIDI with N quarter notes on C4 at 120 BPM, 4/4, TPQ=480.
fn make_test_midi_f1(note_count: usize) -> Vec<u8> {
    fn var_len(n: u32) -> Vec<u8> {
        if n < 0x80 {
            vec![n as u8]
        } else if n < 0x4000 {
            vec![((n >> 7) as u8) | 0x80, (n & 0x7F) as u8]
        } else {
            vec![
                ((n >> 14) as u8) | 0x80,
                ((n >> 7) as u8 & 0x7F) | 0x80,
                (n & 0x7F) as u8,
            ]
        }
    }

    fn track(events: Vec<u8>) -> Vec<u8> {
        let mut t = b"MTrk".to_vec();
        let len = events.len() as u32;
        t.extend_from_slice(&len.to_be_bytes());
        t.extend_from_slice(&events);
        t
    }

    // Tempo track (track 0)
    let mut tempo_events = Vec::new();
    // delta=0, tempo=500000
    tempo_events.extend_from_slice(&[0x00, 0xFF, 0x51, 0x03, 0x07, 0xA1, 0x20]);
    // delta=0, time sig 4/4
    tempo_events.extend_from_slice(&[0x00, 0xFF, 0x58, 0x04, 0x04, 0x02, 0x18, 0x08]);
    // delta=0, EOT
    tempo_events.extend_from_slice(&[0x00, 0xFF, 0x2F, 0x00]);
    let tempo_track = track(tempo_events);

    // Note track (track 1)
    let tpq: u32 = 480;
    let mut note_events = Vec::new();
    for _ in 0..note_count {
        // NoteOn delta=0, ch=0, pitch=60, vel=80
        note_events.extend_from_slice(&[0x00, 0x90, 0x3C, 0x50]);
        // NoteOff delta=480 (1 quarter)
        let mut d = var_len(tpq);
        note_events.append(&mut d);
        note_events.extend_from_slice(&[0x80, 0x3C, 0x00]);
    }
    // EOT
    note_events.extend_from_slice(&[0x00, 0xFF, 0x2F, 0x00]);
    let note_track = track(note_events);

    let mut midi = Vec::new();
    // MThd header: format=1, num_tracks=2, tpq=480
    midi.extend_from_slice(b"MThd");
    midi.extend_from_slice(&6u32.to_be_bytes());
    midi.extend_from_slice(&1u16.to_be_bytes());
    midi.extend_from_slice(&2u16.to_be_bytes());
    midi.extend_from_slice(&(tpq as u16).to_be_bytes());
    midi.extend_from_slice(&tempo_track);
    midi.extend_from_slice(&note_track);
    midi
}

#[test]
fn test_midi_parse_returns_score() {
    let bytes = make_test_midi_f1(4);
    let score = MidiParser::parse(&bytes).expect("should parse");
    assert_eq!(score.parts.len(), 1);
    assert!(!score.parts[0].measures.is_empty());
}

#[test]
fn test_midi_4_quarter_notes_pitches() {
    let bytes = make_test_midi_f1(4);
    let score = MidiParser::parse(&bytes).expect("should parse");

    // All real notes should be C4
    let real_notes: Vec<_> = score.parts[0]
        .measures
        .iter()
        .flat_map(|m| m.elements.iter())
        .filter_map(|e| {
            if let MeasureElement::Note(n) = e {
                Some(n)
            } else {
                None
            }
        })
        .filter(|n| !n.rest)
        .collect();

    assert_eq!(real_notes.len(), 4, "should have 4 real notes");
    for n in &real_notes {
        let p = n.pitch.as_ref().expect("pitched note");
        assert_eq!(p.step, "C");
        assert_eq!(p.octave, 4);
        assert_eq!(p.alter, None);
    }
}

#[test]
fn test_midi_note_types_are_quarter() {
    let bytes = make_test_midi_f1(4);
    let score = MidiParser::parse(&bytes).expect("should parse");

    let real_notes: Vec<_> = score.parts[0]
        .measures
        .iter()
        .flat_map(|m| m.elements.iter())
        .filter_map(|e| {
            if let MeasureElement::Note(n) = e {
                Some(n)
            } else {
                None
            }
        })
        .filter(|n| !n.rest)
        .collect();

    for n in &real_notes {
        assert_eq!(n.note_type.as_deref(), Some("quarter"));
        assert_eq!(n.dot_count, 0);
        assert_eq!(n.duration, 480);
    }
}

#[test]
fn test_midi_score_has_attributes() {
    let bytes = make_test_midi_f1(1);
    let score = MidiParser::parse(&bytes).expect("should parse");

    let m0 = &score.parts[0].measures[0];
    let attrs = m0
        .attributes
        .as_ref()
        .expect("first measure needs attributes");

    assert_eq!(attrs.divisions, Some(480));
    assert_eq!(attrs.key.as_ref().map(|k| k.fifths), Some(0));
    assert_eq!(attrs.time.as_ref().map(|t| t.beat_type), Some(4));
    assert!(!attrs.clefs.is_empty());
}

#[test]
fn test_midi_empty_score_no_panic() {
    // A MIDI with no notes still produces a valid Score
    let bytes = make_test_midi_f1(0);
    let score = MidiParser::parse(&bytes).expect("should not error");
    assert_eq!(score.parts.len(), 1);
}

#[test]
fn test_midi_score_renders_to_svg() {
    let bytes = make_test_midi_f1(8);
    let score = MidiParser::parse(&bytes).expect("should parse");
    let renderer = Renderer::default();
    let svg = renderer.render(&score);
    assert!(svg.contains("<svg"), "should produce valid SVG");
    assert!(svg.contains("viewBox"), "SVG should have viewBox");
}

#[test]
fn test_midi_from_sample_file() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/midi_samples/c_major_4notes.mid");
    if !path.exists() {
        return; // skip if file not present
    }
    let bytes = std::fs::read(&path).expect("read sample MIDI");
    let score = MidiParser::parse(&bytes).expect("should parse sample MIDI");
    assert!(!score.parts.is_empty());
    let real_notes: Vec<_> = score.parts[0]
        .measures
        .iter()
        .flat_map(|m| m.elements.iter())
        .filter_map(|e| {
            if let MeasureElement::Note(n) = e {
                Some(n)
            } else {
                None
            }
        })
        .filter(|n| !n.rest)
        .collect();
    assert!(real_notes.len() >= 4, "should have at least 4 notes");
}

#[test]
fn test_beam_assignment() {
    use staveloom_core::midi_parser::MidiParser;
    use staveloom_core::models::MeasureElement;

    // Build a minimal MIDI file with 8 eighth notes (4/4, TPQ=480, tempo=500000µs)
    // Header: format=0, tracks=1, TPQ=480
    // Track: tempo + 8 note-on/off pairs at 240-tick intervals (eighth = 240 ticks at TPQ=480)
    let mut midi: Vec<u8> = Vec::new();
    // MThd
    midi.extend_from_slice(b"MThd\x00\x00\x00\x06\x00\x00\x00\x01\x01\xe0"); // TPQ=480
    // MTrk placeholder
    let track_start = midi.len();
    midi.extend_from_slice(b"MTrk\x00\x00\x00\x00");
    let body_start = midi.len();
    // delta=0, tempo=500000 (0x07A120)
    midi.extend_from_slice(&[0x00, 0xFF, 0x51, 0x03, 0x07, 0xA1, 0x20]);
    // delta=0, time sig 4/4
    midi.extend_from_slice(&[0x00, 0xFF, 0x58, 0x04, 0x04, 0x02, 0x18, 0x08]);
    // 8 eighth notes: C4..C5, each 240 ticks (TPQ=480 → quarter=480, eighth=240)
    // VLQ for 240 = 0x81 0x70
    let pitches = [60u8, 62, 64, 65, 67, 69, 71, 72];
    for &p in &pitches {
        midi.push(0x00); // delta=0 before note-on
        midi.extend_from_slice(&[0x90, p, 0x64]); // note-on ch1
        midi.extend_from_slice(&[0x81, 0x70]); // delta=240 ticks
        midi.extend_from_slice(&[0x80, p, 0x00]); // note-off ch1
    }
    // End of track
    midi.extend_from_slice(&[0x00, 0xFF, 0x2F, 0x00]);
    // Fill in track length
    let body_end = midi.len();
    let track_len = (body_end - body_start) as u32;
    let len_bytes = track_len.to_be_bytes();
    midi[track_start + 4] = len_bytes[0];
    midi[track_start + 5] = len_bytes[1];
    midi[track_start + 6] = len_bytes[2];
    midi[track_start + 7] = len_bytes[3];

    let score = MidiParser::parse(&midi).expect("parse failed");

    let mut beamable_notes = 0usize;
    let mut beamed_notes = 0usize;

    for part in &score.parts {
        for measure in &part.measures {
            for el in &measure.elements {
                if let MeasureElement::Note(n) = el {
                    if n.rest || n.is_chord {
                        continue;
                    }
                    let is_beamable = matches!(
                        n.note_type.as_deref(),
                        Some("eighth" | "16th" | "32nd" | "64th")
                    );
                    if is_beamable {
                        beamable_notes += 1;
                    }
                    if !n.beams.is_empty() {
                        beamed_notes += 1;
                    }
                    eprintln!("  note type={:?} beams={:?}", n.note_type, n.beams);
                }
            }
        }
    }

    eprintln!("beamable={beamable_notes} beamed={beamed_notes}");
    assert!(beamable_notes > 0, "no beamable eighth notes found");
    assert_eq!(
        beamed_notes, beamable_notes,
        "all beamable notes should be beamed"
    );
}

#[test]
fn test_beam_renders_in_svg() {
    use staveloom_core::midi_parser::MidiParser;
    use staveloom_core::renderer::Renderer;

    // Same 8-eighth-note MIDI as test_beam_assignment
    let mut midi: Vec<u8> = Vec::new();
    midi.extend_from_slice(b"MThd\x00\x00\x00\x06\x00\x00\x00\x01\x01\xe0");
    let track_start = midi.len();
    midi.extend_from_slice(b"MTrk\x00\x00\x00\x00");
    let body_start = midi.len();
    midi.extend_from_slice(&[0x00, 0xFF, 0x51, 0x03, 0x07, 0xA1, 0x20]);
    midi.extend_from_slice(&[0x00, 0xFF, 0x58, 0x04, 0x04, 0x02, 0x18, 0x08]);
    let pitches = [60u8, 62, 64, 65, 67, 69, 71, 72];
    for &p in &pitches {
        midi.push(0x00);
        midi.extend_from_slice(&[0x90, p, 0x64]);
        midi.extend_from_slice(&[0x81, 0x70]);
        midi.extend_from_slice(&[0x80, p, 0x00]);
    }
    midi.extend_from_slice(&[0x00, 0xFF, 0x2F, 0x00]);
    let body_end = midi.len();
    let len_bytes = ((body_end - body_start) as u32).to_be_bytes();
    midi[track_start + 4] = len_bytes[0];
    midi[track_start + 5] = len_bytes[1];
    midi[track_start + 6] = len_bytes[2];
    midi[track_start + 7] = len_bytes[3];

    let score = MidiParser::parse(&midi).expect("parse");
    let renderer = Renderer::default();
    let (svg, _) = renderer.render_with_metadata(&score);

    // Beams are drawn as <path ... fill="black" ...> polygons.
    // Flags are drawn as text elements with Bravura unicode chars.
    // With 8 eighth notes in 4/4, we expect 4 beam paths (one per beat pair).
    let beam_path_count = svg.matches(r#"fill="black""#).count();
    eprintln!("SVG fill=black count: {beam_path_count}");
    eprintln!("SVG has 'flag'? {}", svg.contains("E240"));
    eprintln!("SVG length: {}", svg.len());

    // Should NOT have 8 flag symbols (E240/E241 = 8th note flag glyphs)
    assert!(
        !svg.contains('\u{E240}') && !svg.contains('\u{E241}'),
        "Flags should not appear when beams are assigned"
    );
}

#[test]
fn test_beam_svg_inspection() {
    use staveloom_core::midi_parser::MidiParser;
    use staveloom_core::renderer::Renderer;

    // Same 8-eighth-note MIDI as before
    let mut midi: Vec<u8> = Vec::new();
    midi.extend_from_slice(b"MThd\x00\x00\x00\x06\x00\x00\x00\x01\x01\xe0");
    let track_start = midi.len();
    midi.extend_from_slice(b"MTrk\x00\x00\x00\x00");
    let body_start = midi.len();
    midi.extend_from_slice(&[0x00, 0xFF, 0x51, 0x03, 0x07, 0xA1, 0x20]);
    midi.extend_from_slice(&[0x00, 0xFF, 0x58, 0x04, 0x04, 0x02, 0x18, 0x08]);
    let pitches = [60u8, 62, 64, 65, 67, 69, 71, 72];
    for &p in &pitches {
        midi.push(0x00);
        midi.extend_from_slice(&[0x90, p, 0x64]);
        midi.extend_from_slice(&[0x81, 0x70]);
        midi.extend_from_slice(&[0x80, p, 0x00]);
    }
    midi.extend_from_slice(&[0x00, 0xFF, 0x2F, 0x00]);
    let body_end = midi.len();
    let len_bytes = ((body_end - body_start) as u32).to_be_bytes();
    midi[track_start + 4..track_start + 8].copy_from_slice(&len_bytes);

    let score = MidiParser::parse(&midi).expect("parse");
    let renderer = Renderer::default();
    let (svg, _) = renderer.render_with_metadata(&score);

    // Count SVG element types
    let stem_count = svg.matches("stroke-width=\"1.2\"").count();
    let beam_count = svg.matches("fill=\"black\"/>").count();
    let flag_count = svg.matches('\u{E240}').count() + svg.matches('\u{E241}').count();

    eprintln!("Stems (sw=1.2): {stem_count}");
    eprintln!("Beam paths: {beam_count}");
    eprintln!("Flags: {flag_count}");

    // Print the SVG for inspection
    // eprintln!("SVG:\n{svg}");

    assert!(
        stem_count >= 8,
        "should have at least 8 stems for 8 eighth notes"
    );
    assert!(beam_count >= 4, "should have at least 4 beam paths");
    assert_eq!(flag_count, 0, "should have no flags");
}

#[test]
fn test_drum_stem_rendering() {
    use staveloom_core::renderer::Renderer;
    // Build a minimal MIDI with drum notes (channel 9, pitch 42=hi-hat, 16th notes)
    // TPQ=480, 4/4, one measure of 16 sixteenth-note hi-hats
    let mut midi: Vec<u8> = Vec::new();
    // MThd: format=1, tracks=2, tpq=480
    midi.extend_from_slice(b"MThd\x00\x00\x00\x06\x00\x01\x00\x02\x01\xe0");
    // Track 0: tempo
    let tempo_track = {
        let mut t = Vec::new();
        t.extend_from_slice(&[0x00, 0xFF, 0x51, 0x03, 0x07, 0xA1, 0x20]); // 120 BPM
        t.extend_from_slice(&[0x00, 0xFF, 0x58, 0x04, 0x04, 0x02, 0x18, 0x08]); // 4/4
        t.extend_from_slice(&[0x00, 0xFF, 0x2F, 0x00]);
        let mut track = b"MTrk".to_vec();
        track.extend_from_slice(&(t.len() as u32).to_be_bytes());
        track.extend_from_slice(&t);
        track
    };
    // Track 1: 16 hi-hat notes (pitch 42, channel 9) as 16th notes (120 ticks each at tpq=480)
    let drum_track = {
        let mut t = Vec::new();
        for i in 0..16 {
            let delta: u32 = if i == 0 { 0 } else { 120 }; // 120 ticks = 16th note
            // delta encoding
            if delta < 128 {
                t.push(delta as u8);
            } else {
                t.extend_from_slice(&[(delta >> 7) as u8 | 0x80, (delta & 0x7F) as u8]);
            }
            t.extend_from_slice(&[0x99, 42, 80]); // note on ch10 (0x9F would be ch16, 0x99=ch10)
            // note off after 60 ticks
            t.extend_from_slice(&[0x3C]); // 60 = 0x3C
            t.extend_from_slice(&[0x89, 42, 0]); // note off ch10
        }
        t.extend_from_slice(&[0x00, 0xFF, 0x2F, 0x00]);
        let mut track = b"MTrk".to_vec();
        track.extend_from_slice(&(t.len() as u32).to_be_bytes());
        track.extend_from_slice(&t);
        track
    };
    midi.extend_from_slice(&tempo_track);
    midi.extend_from_slice(&drum_track);

    let score = MidiParser::parse(&midi).expect("parse failed");
    println!("Parts: {}", score.parts.len());
    if let Some(part) = score.parts.first() {
        if let Some(m) = part.measures.first() {
            for (i, el) in m.elements.iter().enumerate().take(10) {
                if let MeasureElement::Note(n) = el {
                    println!(
                        "  [{i}] chord={} type={:?} beams={} stem={:?} unpitched={:?}",
                        n.is_chord,
                        n.note_type,
                        n.beams.len(),
                        n.stem,
                        n.unpitched
                            .as_ref()
                            .map(|u| format!("{}{}", u.display_step, u.display_octave))
                    );
                }
            }
        }
    }
    let renderer = Renderer::default();
    let (svg, _) = renderer.render_with_metadata(&score);
    std::fs::write("/tmp/drum_simple.svg", &svg).unwrap();

    let stem_count = svg.matches(r#"stroke-width="1.2""#).count();
    let beam_count = svg.matches(r#"fill="black""#).count();
    println!("Stems: {}, Beams: {}", stem_count, beam_count);
    assert!(stem_count > 0, "Expected drum stems to be drawn");
    assert!(beam_count > 0, "Expected drum beams to be drawn");
}

/// Verify that beamed notes with mixed stem directions have stems of sufficient length.
///
/// Treble clef midline B4 = MIDI 71.
/// D5 (74 > 71) → stem="down" (is_up=false); B3 (59 < 71) → stem="up" (is_up=true).
/// Alternating D5/B3 16th notes in the same beat group triggers the y_tip inversion bug:
/// beam_is_up=false (from D5), but B3 notes carry the wrong y_tip after corrected_pts,
/// causing the beam reference line to slope the wrong way and leaving some stems near-zero.
#[test]
fn test_mixed_stem_beam_group_stems_visible() {
    // Format 1, TPQ=480, 2 tracks
    fn var_len(n: u32) -> Vec<u8> {
        if n < 0x80 {
            vec![n as u8]
        } else {
            vec![((n >> 7) as u8) | 0x80, (n & 0x7F) as u8]
        }
    }
    fn note_events(pitches: &[(u8, u32)]) -> Vec<u8> {
        let mut t = Vec::new();
        for (pitch, dur) in pitches {
            t.push(0x00);
            t.extend_from_slice(&[0x90, *pitch, 0x60]); // note on
            t.extend_from_slice(&var_len(*dur));
            t.extend_from_slice(&[0x80, *pitch, 0x00]); // note off
        }
        t.extend_from_slice(&[0x00, 0xFF, 0x2F, 0x00]);
        t
    }

    let mut midi = b"MThd\x00\x00\x00\x06\x00\x01\x00\x02\x01\xe0".to_vec();

    // Tempo track
    let mut tt = Vec::new();
    tt.extend_from_slice(&[0x00, 0xFF, 0x51, 0x03, 0x07, 0xA1, 0x20]); // 120 BPM
    tt.extend_from_slice(&[0x00, 0xFF, 0x58, 0x04, 0x04, 0x02, 0x18, 0x08]); // 4/4
    tt.extend_from_slice(&[0x00, 0xFF, 0x2F, 0x00]);
    let mut tempo_track = b"MTrk".to_vec();
    tempo_track.extend_from_slice(&(tt.len() as u32).to_be_bytes());
    tempo_track.extend_from_slice(&tt);

    // Note track: alternating D5(74,above) and B3(59,below) as 16th notes (120 ticks at TPQ=480)
    let pitches = vec![
        (74u8, 120u32), // D5, stem=down (74>71)
        (59u8, 120u32), // B3, stem=up   (59<71)
        (74u8, 120u32), // D5, stem=down
        (59u8, 120u32), // B3, stem=up
        // beat 2
        (74u8, 120u32),
        (59u8, 120u32),
        (74u8, 120u32),
        (59u8, 120u32),
        // beat 3
        (74u8, 120u32),
        (59u8, 120u32),
        (74u8, 120u32),
        (59u8, 120u32),
        // beat 4
        (74u8, 120u32),
        (59u8, 120u32),
        (74u8, 120u32),
        (59u8, 120u32),
    ];
    let ne = note_events(&pitches);
    let mut note_track = b"MTrk".to_vec();
    note_track.extend_from_slice(&(ne.len() as u32).to_be_bytes());
    note_track.extend_from_slice(&ne);

    midi.extend_from_slice(&tempo_track);
    midi.extend_from_slice(&note_track);

    let score = MidiParser::parse(&midi).expect("parse failed");
    let renderer = Renderer::default();
    let (svg, _) = renderer.render_with_metadata(&score);
    std::fs::write("/tmp/mixed_stem_beam.svg", &svg).unwrap();

    // Stems are <line> elements with stroke-width="1.2".
    // Ledger lines use stroke-width="1", so we can distinguish them.
    // All 16 notes must have stems with length > 5px (essentially non-zero).
    let stem_re = regex_stem_lines(&svg);
    let short_stems: Vec<f32> = stem_re.iter().copied().filter(|&l| l < 5.0).collect();
    println!("All stem lengths (px): {:?}", stem_re);
    assert_eq!(
        stem_re.len(),
        16,
        "Expected 16 stems, got {}",
        stem_re.len()
    );
    assert!(
        short_stems.is_empty(),
        "Found near-zero stems (< 5px): {:?}. SVG: /tmp/mixed_stem_beam.svg",
        short_stems
    );
}

/// MIDI files often stack duplicate note-ons of the same pitch into one chord.
/// Those unisons must print as a single notehead — rendering them literally produces
/// extra stemless noteheads shoved next to the real one (Timpani, m.12 of the sample).
/// Extract stem line lengths (px) from SVG. Stems have stroke-width="1.2", ledger lines have "1".
fn regex_stem_lines(svg: &str) -> Vec<f32> {
    let mut results = Vec::new();
    let mut pos = 0;
    while let Some(start) = svg[pos..].find("<line ") {
        let abs_start = pos + start;
        let end = svg[abs_start..]
            .find('>')
            .map(|e| abs_start + e + 1)
            .unwrap_or(svg.len());
        let elem = &svg[abs_start..end];
        if elem.contains(r#"stroke-width="1.2""#) {
            let y1 = parse_attr(elem, "y1");
            let y2 = parse_attr(elem, "y2");
            if let (Some(y1), Some(y2)) = (y1, y2) {
                results.push((y2 - y1).abs());
            }
        }
        pos = end;
    }
    results
}

fn parse_attr(elem: &str, name: &str) -> Option<f32> {
    let needle = format!("{}=\"", name);
    let start = elem.find(&needle)? + needle.len();
    let end = elem[start..].find('"')? + start;
    elem[start..end].parse().ok()
}
