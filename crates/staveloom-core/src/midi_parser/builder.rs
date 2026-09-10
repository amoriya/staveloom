use super::event::{ParsedMidi, RawNote};
use super::pitch::{choose_clef, drum_notehead, drum_position, midi_to_pitch};
use super::quantizer::{
    decompose_duration, detect_grid, is_human_midi, quantize_note, ticks_to_note_value_tolerant,
};
use crate::models::{
    Attributes, Direction, DirectionType, GroupSymbol, Key, Measure, MeasureElement,
    MidiInstrument, Notation, Note, Notehead, Part, PartGroup, PartListItem, Score, Sound, Time,
    Unpitched,
};

/// General MIDI program names (0-indexed, program 0 = Acoustic Grand Piano).
const GM_NAMES: [&str; 128] = [
    "Acoustic Grand Piano",
    "Bright Acoustic Piano",
    "Electric Grand Piano",
    "Honky-Tonk Piano",
    "Electric Piano 1",
    "Electric Piano 2",
    "Harpsichord",
    "Clavi",
    "Celesta",
    "Glockenspiel",
    "Music Box",
    "Vibraphone",
    "Marimba",
    "Xylophone",
    "Tubular Bells",
    "Dulcimer",
    "Drawbar Organ",
    "Percussive Organ",
    "Rock Organ",
    "Church Organ",
    "Reed Organ",
    "Accordion",
    "Harmonica",
    "Tango Accordion",
    "Acoustic Guitar (Nylon)",
    "Acoustic Guitar (Steel)",
    "Electric Guitar (Jazz)",
    "Electric Guitar (Clean)",
    "Electric Guitar (Muted)",
    "Overdriven Guitar",
    "Distortion Guitar",
    "Guitar Harmonics",
    "Acoustic Bass",
    "Electric Bass (Finger)",
    "Electric Bass (Pick)",
    "Fretless Bass",
    "Slap Bass 1",
    "Slap Bass 2",
    "Synth Bass 1",
    "Synth Bass 2",
    "Violin",
    "Viola",
    "Cello",
    "Contrabass",
    "Tremolo Strings",
    "Pizzicato Strings",
    "Orchestral Harp",
    "Timpani",
    "String Ensemble 1",
    "String Ensemble 2",
    "Synth Strings 1",
    "Synth Strings 2",
    "Choir Aahs",
    "Voice Oohs",
    "Synth Voice",
    "Orchestra Hit",
    "Trumpet",
    "Trombone",
    "Tuba",
    "Muted Trumpet",
    "French Horn",
    "Brass Section",
    "Synth Brass 1",
    "Synth Brass 2",
    "Soprano Sax",
    "Alto Sax",
    "Tenor Sax",
    "Baritone Sax",
    "Oboe",
    "English Horn",
    "Bassoon",
    "Clarinet",
    "Piccolo",
    "Flute",
    "Recorder",
    "Pan Flute",
    "Blown Bottle",
    "Shakuhachi",
    "Whistle",
    "Ocarina",
    "Lead 1 (Square)",
    "Lead 2 (Sawtooth)",
    "Lead 3 (Calliope)",
    "Lead 4 (Chiff)",
    "Lead 5 (Charang)",
    "Lead 6 (Voice)",
    "Lead 7 (Fifths)",
    "Lead 8 (Bass+Lead)",
    "Pad 1 (New Age)",
    "Pad 2 (Warm)",
    "Pad 3 (Polysynth)",
    "Pad 4 (Choir)",
    "Pad 5 (Bowed)",
    "Pad 6 (Metallic)",
    "Pad 7 (Halo)",
    "Pad 8 (Sweep)",
    "FX 1 (Rain)",
    "FX 2 (Soundtrack)",
    "FX 3 (Crystal)",
    "FX 4 (Atmosphere)",
    "FX 5 (Brightness)",
    "FX 6 (Goblins)",
    "FX 7 (Echoes)",
    "FX 8 (Sci-Fi)",
    "Sitar",
    "Banjo",
    "Shamisen",
    "Koto",
    "Kalimba",
    "Bag Pipe",
    "Fiddle",
    "Shanai",
    "Tinkle Bell",
    "Agogo",
    "Steel Drums",
    "Woodblock",
    "Taiko Drum",
    "Melodic Tom",
    "Synth Drum",
    "Reverse Cymbal",
    "Guitar Fret Noise",
    "Breath Noise",
    "Seashore",
    "Bird Tweet",
    "Telephone Ring",
    "Helicopter",
    "Applause",
    "Gunshot",
];

fn gm_name(program: u8) -> &'static str {
    GM_NAMES.get(program as usize).copied().unwrap_or("Unknown")
}

fn velocity_to_dynamics(vel: u8) -> &'static str {
    match vel {
        0..=15 => "pppp",
        16..=31 => "ppp",
        32..=47 => "pp",
        48..=63 => "p",
        64..=79 => "mp",
        80..=95 => "mf",
        96..=111 => "f",
        112..=126 => "ff",
        _ => "fff",
    }
}

/// Infer the most likely key signature (fifths) from the pitch-class distribution
/// of the notes using a simple major/minor key-profile match.
///
/// Returns 0 (C major) when the evidence is weak or inconclusive.
/// Only call this when the MIDI didn't embed an explicit key-sig event (i.e., the
/// parsed key is 0 but the score might actually be in a different key).
fn infer_key_fifths(notes: &[RawNote]) -> i8 {
    if notes.len() < 8 {
        return 0;
    }

    // Count notes per pitch class (0=C, 1=C#/Db, …, 11=B).
    let mut pc = [0u32; 12];
    for n in notes {
        pc[(n.pitch % 12) as usize] += 1;
    }

    // Relative major-scale step pattern (semitones from root).
    const MAJOR: [u8; 7] = [0, 2, 4, 5, 7, 9, 11];

    // Key roots in circle-of-fifths order (C, G, D, A, E, B, F#/Gb, Db, Ab, Eb, Bb, F)
    // and their corresponding fifths values.
    const ROOTS: [u8; 12] = [0, 7, 2, 9, 4, 11, 6, 1, 8, 3, 10, 5];
    const FIFTHS: [i8; 12] = [0, 1, 2, 3, 4, 5, 6, -5, -4, -3, -2, -1];

    let total: u32 = pc.iter().sum();
    let mut best_score = 0u32;
    let mut best_fifths = 0i8;

    for (i, &root) in ROOTS.iter().enumerate() {
        let score: u32 = MAJOR
            .iter()
            .map(|&step| pc[((root + step) % 12) as usize])
            .sum();
        if score > best_score {
            best_score = score;
            best_fifths = FIFTHS[i];
        }
    }

    // Only commit to a non-C key when the best key clearly explains more notes
    // than C major; otherwise stay with 0 to avoid false positives.
    let c_major_score: u32 = MAJOR.iter().map(|&s| pc[s as usize]).sum();
    if best_score > c_major_score && best_score * 10 >= total * 6 {
        // Confidence: best key accounts for ≥ 60% of note instances
        best_fifths
    } else {
        0
    }
}

/// Median of a non-empty slice (does not allocate if already sorted).
fn median_velocity(velocities: &[u8]) -> u8 {
    if velocities.is_empty() {
        return 64;
    }
    let mut v = velocities.to_vec();
    v.sort_unstable();
    v[v.len() / 2]
}

/// An in-progress note event during measure building.
#[derive(Clone)]
struct NoteEvent {
    note: RawNote,
    tie_stop: bool,
    tie_start: bool,
}

/// Heuristically separate overlapping notes into up to six independent voices.
///
/// Voice 1 receives notes that fit sequentially; voice 2 receives notes that
/// overlap voice 1. When both are busy, a small snap (≤ tpq/8) groups the note
/// with the nearest active chord. Beyond that, overflow goes to voice 3, then
/// voice 4/5/6 — each tracked with an end-time so fill_and_convert never emits
/// a force-chord at the wrong onset.
fn separate_voices(
    events: Vec<NoteEvent>,
    measure_start: u64,
    tpq: u32,
) -> (
    Vec<NoteEvent>,
    Vec<NoteEvent>,
    Vec<NoteEvent>,
    Vec<NoteEvent>,
    Vec<NoteEvent>,
    Vec<NoteEvent>,
) {
    if events.is_empty() {
        return (vec![], vec![], vec![], vec![], vec![], vec![]);
    }

    // Sort: start_tick asc, then longer-duration notes first at the same tick
    let mut sorted = events;
    sorted.sort_by(|a, b| {
        a.note
            .start_tick
            .cmp(&b.note.start_tick)
            .then(b.note.end_tick.cmp(&a.note.end_tick))
    });

    let mut voice1: Vec<NoteEvent> = Vec::new();
    let mut voice2: Vec<NoteEvent> = Vec::new();
    let mut voice3: Vec<NoteEvent> = Vec::new();
    let mut voice4: Vec<NoteEvent> = Vec::new();
    let mut voice5: Vec<NoteEvent> = Vec::new();
    let mut voice6: Vec<NoteEvent> = Vec::new();
    let mut v1_end = measure_start;
    let mut v2_end = measure_start;
    let mut v3_end = measure_start;
    let mut v4_end = measure_start;
    let mut v5_end = measure_start;
    let mut v4_chord_tick: Option<u64> = None;
    let mut v5_chord_tick: Option<u64> = None;

    // Track the "current chord" for v1/v2: start tick and max end tick.
    // This ensures that when several notes start at the same tick and voice 1 is
    // busy (e.g. occupied by a longer held note), later notes at that same tick
    // correctly join voice 2 as chord companions rather than spilling back into
    // voice 1 via the "both-busy" fallback.
    let mut v1_chord_tick: Option<u64> = None;
    let mut v1_chord_max_end: u64 = measure_start;
    let mut v1_chord_max_pitch: u8 = 0;
    let mut v2_chord_tick: Option<u64> = None;
    let mut v2_chord_max_end: u64 = measure_start;
    let mut v2_chord_max_pitch: u8 = 0;

    // Helper: is `end` within a 3:1 duration ratio of the chord's reference duration?
    let is_compatible =
        |chord_max_end: u64, chord_tick: u64, note_end: u64, note_tick: u64| -> bool {
            let chord_dur = chord_max_end.saturating_sub(chord_tick);
            let this_dur = note_end.saturating_sub(note_tick);
            if chord_dur == 0 || this_dur == 0 {
                return false;
            }
            let (lo, hi) = if this_dur < chord_dur {
                (this_dur, chord_dur)
            } else {
                (chord_dur, this_dur)
            };
            hi <= lo * 3
        };

    // Track whether a note from the current both-busy tick was routed to v3/v4.
    // If so, block snapping of any later note from the same source tick:
    // snapping only the highest pitch would add an extra note to the target chord
    // beat while leaving others at the original beat, causing comparison misalignment.
    let mut bb_tick_had_unsnapped: Option<u64> = None;

    for ev in sorted {
        let tick = ev.note.start_tick;
        let end = ev.note.end_tick;

        let is_v1_chord =
            v1_chord_tick == Some(tick) && is_compatible(v1_chord_max_end, tick, end, tick);

        let is_v2_chord =
            v2_chord_tick == Some(tick) && is_compatible(v2_chord_max_end, tick, end, tick);

        if is_v1_chord {
            // Chord companion in voice 1
            v1_chord_max_end = v1_chord_max_end.max(end);
            v1_chord_max_pitch = v1_chord_max_pitch.max(ev.note.pitch);
            v1_end = v1_end.max(end);
            voice1.push(ev);
        } else if tick >= v1_end {
            // Fits sequentially in voice 1
            v1_chord_tick = Some(tick);
            v1_chord_max_end = end;
            v1_chord_max_pitch = ev.note.pitch;
            v1_end = v1_end.max(end);
            voice1.push(ev);
        } else if is_v2_chord {
            // Chord companion in voice 2
            v2_chord_max_end = v2_chord_max_end.max(end);
            v2_chord_max_pitch = v2_chord_max_pitch.max(ev.note.pitch);
            v2_end = v2_end.max(end);
            voice2.push(ev);
        } else if tick >= v2_end {
            // Voice 1 is busy → voice 2 (new note)
            v2_chord_tick = Some(tick);
            v2_chord_max_end = end;
            v2_chord_max_pitch = ev.note.pitch;
            v2_end = v2_end.max(end);
            voice2.push(ev);
        } else {
            // Both voices busy (rare). Snap this note to the nearest active
            // chord start within ≈ one 32nd note (tpq/8) so it joins that
            // chord as a true companion (same start_tick → grouped by
            // group_chords → played at the correct onset without cursor
            // overflow). Beyond that threshold route to voice 3 or 4.
            //
            // Voice 4 is used when voice 3 would also overflow: if a
            // prior v3 note's duration extends past this note's start_tick,
            // fill_and_convert would force-chord it at the wrong onset.
            let beat_tol = (tpq / 8) as u64; // ≈ one 32nd note
            let v1_diff = v1_chord_tick.map_or(u64::MAX, |ct| tick.saturating_sub(ct));
            let v2_diff = v2_chord_tick.map_or(u64::MAX, |ct| tick.saturating_sub(ct));
            // Only snap when:
            //  1. Duration is within 3:1 of the chord (same ratio as is_v1/v2_chord).
            //  2. The incoming pitch is HIGHER than every pitch already in the chord.
            //  3. No earlier note from this same source tick was already routed to v3/v4.
            //     Snapping only the top-pitch note while leaving others at the original
            //     beat creates an extra note at the target beat → comparison misalignment.
            let v2_compat = is_compatible(v2_chord_max_end, v2_chord_tick.unwrap_or(0), end, tick);
            let v1_compat = is_compatible(v1_chord_max_end, v1_chord_tick.unwrap_or(0), end, tick);
            let v2_pitch_ok = ev.note.pitch > v2_chord_max_pitch;
            let v1_pitch_ok = ev.note.pitch > v1_chord_max_pitch;
            let allow_snap = bb_tick_had_unsnapped != Some(tick);
            if allow_snap && v2_diff <= v1_diff && v2_diff <= beat_tol && v2_compat && v2_pitch_ok {
                let mut snapped = ev;
                snapped.note.start_tick = v2_chord_tick.unwrap();
                v2_chord_max_pitch = v2_chord_max_pitch.max(snapped.note.pitch);
                v2_end = v2_end.max(end);
                voice2.push(snapped);
            } else if allow_snap
                && v1_diff < v2_diff
                && v1_diff <= beat_tol
                && v1_compat
                && v1_pitch_ok
            {
                let mut snapped = ev;
                snapped.note.start_tick = v1_chord_tick.unwrap();
                v1_chord_max_end = v1_chord_max_end.max(end);
                v1_chord_max_pitch = v1_chord_max_pitch.max(snapped.note.pitch);
                v1_end = v1_end.max(end);
                voice1.push(snapped);
            } else if tick >= v3_end {
                // v3 is free — no overflow risk
                bb_tick_had_unsnapped = Some(tick);
                v3_end = v3_end.max(end);
                voice3.push(ev);
            } else if tick >= v4_end || v4_chord_tick == Some(tick) {
                // v4 is free or chord companion — no force-chord risk
                bb_tick_had_unsnapped = Some(tick);
                v4_chord_tick = Some(tick);
                v4_end = v4_end.max(end);
                voice4.push(ev);
            } else if tick >= v5_end || v5_chord_tick == Some(tick) {
                // v5 is free or chord companion — no force-chord risk
                bb_tick_had_unsnapped = Some(tick);
                v5_chord_tick = Some(tick);
                v5_end = v5_end.max(end);
                voice5.push(ev);
            } else {
                // v3–v5 all busy — route to v6 to avoid force-chord at wrong onset
                bb_tick_had_unsnapped = Some(tick);
                voice6.push(ev);
            }
        }
    }

    (voice1, voice2, voice3, voice4, voice5, voice6)
}

/// GM programs 0-7 are the Piano family (Acoustic Grand, Bright, Electric Grand,
/// Honky-tonk, Electric Piano 1/2, Harpsichord, Clavi).
fn is_piano_family(program: u8) -> bool {
    program < 8
}

/// Returns `true` when a set of notes spans both treble and bass registers
/// (has notes at/above C4 AND below B3) with a total pitch range ≥ 24 semitones.
fn needs_grand_staff_split(notes: &[RawNote]) -> bool {
    if notes.len() < 4 {
        return false;
    }
    let has_treble = notes.iter().any(|n| n.pitch >= 60); // at/above C4
    let has_bass = notes.iter().any(|n| n.pitch < 55); // below B3
    if !has_treble || !has_bass {
        return false;
    }
    let max_p = notes.iter().map(|n| n.pitch).max().unwrap_or(0);
    let min_p = notes.iter().map(|n| n.pitch).min().unwrap_or(127);
    (max_p as i32 - min_p as i32) >= 24
}

/// Find the best pitch at which to split a wide-range part into treble/bass.
///
/// Searches only the "boundary zone" [48, 65] (C3–F4) where the treble/bass
/// split naturally falls for piano music. Restricting to this range prevents
/// gaps *within* treble chords (e.g. Eb4→Bb4 = 7 semitones) from pushing the
/// split point above the actual treble/bass boundary.
/// Defaults to C4 (60) when no notes fall inside the zone.
fn grand_staff_split_pitch(notes: &[RawNote]) -> u8 {
    let mut pitches: Vec<u8> = notes.iter().map(|n| n.pitch).collect();
    pitches.sort_unstable();
    pitches.dedup();

    // Include only pitches in the treble/bass boundary zone.
    let middle: Vec<u8> = pitches
        .into_iter()
        .filter(|&p| p >= 48 && p <= 65)
        .collect();
    if middle.len() < 2 {
        return 60;
    }

    let mut best_gap = 0u8;
    let mut best_split = 60u8;
    for w in middle.windows(2) {
        let gap = w[1] - w[0];
        if gap > best_gap {
            best_gap = gap;
            best_split = w[1]; // first pitch of the upper cluster
        }
    }
    best_split.clamp(48, 72)
}

/// Build a `Score` from parsed MIDI data.
///
/// Applies quantization (grid detection + snap), converts pitches,
/// splits notes at barlines, fills rests, and assembles the final score.
/// Wide-range parts (piano-style) are automatically split into a treble and
/// bass staff wrapped in a `PartGroup` brace.
pub fn build(parsed: &ParsedMidi) -> Score {
    let tpq = parsed.tpq;
    let boundaries = parsed.measure_boundaries();

    let part_count = parsed.parts.len();
    let mut parts: Vec<Part> = Vec::new();
    let mut part_list: Vec<PartListItem> = Vec::new();
    let mut next_part_id = 1usize;
    let mut group_number = 0i32;

    // File-level human detection: if any non-drum part in this file looks like
    // a live recording (sloppy GCD + expressive velocities), treat the whole
    // file as a human performance and quantize every part.  Per-part detection
    // alone misses backup instruments (bass, rhythm guitar) that share the
    // same recording session but happen to have fewer distinct velocity values.
    let file_is_human = (0..part_count).any(|idx| {
        if parsed.parts[idx].is_drum {
            return false;
        }
        let part_notes: Vec<RawNote> = parsed
            .notes
            .iter()
            .filter(|n| n.part_idx == idx)
            .cloned()
            .collect();
        is_human_midi(&part_notes, tpq)
    });

    for part_idx in 0..part_count {
        let part_info = &parsed.parts[part_idx];
        let is_drum = part_info.is_drum;

        // Collect and optionally quantize notes for this part
        let raw_notes: Vec<RawNote> = parsed
            .notes
            .iter()
            .filter(|n| n.part_idx == part_idx)
            .cloned()
            .collect();

        let is_human = file_is_human;
        let notes: Vec<RawNote> = if !raw_notes.is_empty() && is_human {
            let raw_grid = detect_grid(&raw_notes, tpq);
            // Enforce a minimum grid of one 16th note for human MIDI.
            // This prevents sub-16th artifacts (48th/36th notes) while
            // preserving triplet-8th rhythms (160 ticks > 120 at tpq=480).
            let grid = raw_grid.max((tpq / 4).max(1));
            raw_notes.iter().map(|n| quantize_note(n, grid)).collect()
        } else {
            raw_notes
        };

        let instrument_name = if is_drum {
            "Drumset".to_string()
        } else {
            part_info
                .name
                .clone()
                .unwrap_or_else(|| gm_name(part_info.program).to_string())
        };

        // When the MIDI carries no explicit key signature (defaults to 0 = C major),
        // try to infer the key from the pitch distribution so accidentals are folded
        // into the key signature rather than printed on every note.
        let inferred_key: Option<i8> = if !is_drum
            && parsed.key_sig_changes.len() == 1
            && parsed.key_sig_changes[0].fifths == 0
        {
            let k = infer_key_fifths(&notes);
            if k != 0 { Some(k) } else { None }
        } else {
            None
        };

        if !is_drum && is_piano_family(part_info.program) && needs_grand_staff_split(&notes) {
            let split = grand_staff_split_pitch(&notes);

            let treble_notes: Vec<RawNote> =
                notes.iter().filter(|n| n.pitch >= split).cloned().collect();
            let bass_notes: Vec<RawNote> =
                notes.iter().filter(|n| n.pitch < split).cloned().collect();

            // Safety: if the split left one side empty, fall through to single-staff rendering.
            if treble_notes.is_empty() || bass_notes.is_empty() {
                let clef = choose_clef(&notes, false, part_info.program);
                let measures = build_measures(
                    &notes,
                    &boundaries,
                    parsed,
                    tpq,
                    false,
                    &clef,
                    inferred_key,
                    is_human,
                );
                let part_id = format!("P{}", next_part_id);
                next_part_id += 1;
                part_list.push(PartListItem::Part {
                    id: part_id.clone(),
                    name: Some(instrument_name),
                    abbreviation: None,
                    instrument_names: Vec::new(),
                    part_links: Vec::new(),
                    name_display: None,
                    abbreviation_display: None,
                    instrument_sound: None,
                    midi_instruments: vec![MidiInstrument {
                        id: format!("{}-I1", part_id),
                        channel: Some(part_idx as i32 + 1),
                        program: Some(part_info.program as i32 + 1),
                        volume: Some(78.7),
                        pan: Some(0.0),
                        elevation: None,
                        midi_unpitched: None,
                    }],
                });
                parts.push(Part {
                    id: part_id,
                    measures,
                });
                continue;
            }

            group_number += 1;

            // Brace group start
            part_list.push(PartListItem::Group(PartGroup {
                number: group_number,
                group_type: "start".to_string(),
                name: Some(instrument_name.clone()),
                abbreviation: None,
                symbol: Some(GroupSymbol::Brace),
                barline: Some("yes".to_string()),
            }));

            // Treble (right hand) part
            let treble_clef = choose_clef(&treble_notes, false, part_info.program);
            let treble_id = format!("P{}", next_part_id);
            next_part_id += 1;
            let treble_measures = build_measures(
                &treble_notes,
                &boundaries,
                parsed,
                tpq,
                false,
                &treble_clef,
                inferred_key,
                is_human,
            );
            part_list.push(PartListItem::Part {
                id: treble_id.clone(),
                name: Some(instrument_name.clone()),
                abbreviation: None,
                instrument_names: Vec::new(),
                part_links: Vec::new(),
                name_display: None,
                abbreviation_display: None,
                instrument_sound: None,
                midi_instruments: vec![MidiInstrument {
                    id: format!("{}-I1", treble_id),
                    channel: Some(part_idx as i32 + 1),
                    program: Some(part_info.program as i32 + 1),
                    volume: Some(78.7),
                    pan: Some(0.0),
                    elevation: None,
                    midi_unpitched: None,
                }],
            });
            parts.push(Part {
                id: treble_id,
                measures: treble_measures,
            });

            // Bass (left hand) part
            let bass_clef = choose_clef(&bass_notes, false, part_info.program);
            let bass_id = format!("P{}", next_part_id);
            next_part_id += 1;
            let bass_measures = build_measures(
                &bass_notes,
                &boundaries,
                parsed,
                tpq,
                false,
                &bass_clef,
                inferred_key,
                is_human,
            );
            part_list.push(PartListItem::Part {
                id: bass_id.clone(),
                name: Some(instrument_name),
                abbreviation: None,
                instrument_names: Vec::new(),
                part_links: Vec::new(),
                name_display: None,
                abbreviation_display: None,
                instrument_sound: None,
                midi_instruments: vec![MidiInstrument {
                    id: format!("{}-I1", bass_id),
                    channel: Some(part_idx as i32 + 1),
                    program: Some(part_info.program as i32 + 1),
                    volume: Some(78.7),
                    pan: Some(0.0),
                    elevation: None,
                    midi_unpitched: None,
                }],
            });
            parts.push(Part {
                id: bass_id,
                measures: bass_measures,
            });

            // Brace group stop
            part_list.push(PartListItem::Group(PartGroup {
                number: group_number,
                group_type: "stop".to_string(),
                name: None,
                abbreviation: None,
                symbol: None,
                barline: None,
            }));
        } else {
            // Single-staff part (existing behaviour)
            let clef = choose_clef(&notes, is_drum, part_info.program);
            let measures = build_measures(
                &notes,
                &boundaries,
                parsed,
                tpq,
                is_drum,
                &clef,
                inferred_key,
                is_human,
            );
            let part_id = format!("P{}", next_part_id);
            next_part_id += 1;

            let midi_instrument = MidiInstrument {
                id: format!("{}-I1", part_id),
                channel: Some(if is_drum { 10 } else { part_idx as i32 + 1 }),
                program: Some(part_info.program as i32 + 1),
                volume: Some(78.7),
                pan: Some(0.0),
                elevation: None,
                midi_unpitched: if is_drum { Some(1) } else { None },
            };

            part_list.push(PartListItem::Part {
                id: part_id.clone(),
                name: Some(instrument_name),
                abbreviation: None,
                instrument_names: Vec::new(),
                part_links: Vec::new(),
                name_display: None,
                abbreviation_display: None,
                instrument_sound: None,
                midi_instruments: vec![midi_instrument],
            });

            parts.push(Part {
                id: part_id,
                measures,
            });
        }
    }

    let mut score = Score {
        version: Some("4.0".to_string()),
        title: None,
        creator: None,
        concert_score: false,
        part_list,
        parts,
    };
    crate::auto_beam::auto_beam_score(&mut score);
    crate::auto_beam::expand_double_dotted_notes(&mut score);
    score
}

fn build_measures(
    notes: &[RawNote],
    boundaries: &[u64],
    parsed: &ParsedMidi,
    tpq: u32,
    is_drum: bool,
    clef: &crate::models::Clef,
    key_override: Option<i8>,
    is_human: bool,
) -> Vec<Measure> {
    if boundaries.len() < 2 {
        return Vec::new();
    }

    let mut measures = Vec::new();
    let mut carry_over: Vec<NoteEvent> = Vec::new();

    // Human MIDI gets a wider tolerance (32nd note) to absorb residual
    // timing imprecision after quantization; DAW exports use 64th note.
    let tolerance = if is_human {
        (tpq as u64 / 8).max(1)
    } else {
        (tpq as u64 / 16).max(1)
    };

    // Track previous values to detect mid-piece changes
    let mut prev_key_fifths = key_override.unwrap_or_else(|| parsed.key_sig_at(0));
    let mut prev_ts = parsed.time_sig_at(0);
    let mut prev_tempo_us = parsed.tempo_at(0);
    let mut prev_dynamics: Option<&'static str> = None;
    let mut first_measure = true;

    for window in boundaries.windows(2) {
        let m_start = window[0];
        let m_end = window[1];
        let measure_idx = measures.len();

        // Use the override key when the MIDI embedded no explicit key sig.
        let key_fifths = key_override.unwrap_or_else(|| parsed.key_sig_at(m_start));
        let (ts_num, ts_den) = parsed.time_sig_at(m_start);
        let tempo_us = parsed.tempo_at(m_start);
        let measure_len = m_end - m_start;

        // Collect notes for this measure from carry_over + new onsets
        let mut measure_notes: Vec<NoteEvent> = carry_over.drain(..).collect();
        for n in notes
            .iter()
            .filter(|n| n.start_tick >= m_start && n.start_tick < m_end)
        {
            measure_notes.push(NoteEvent {
                note: n.clone(),
                tie_stop: false,
                tie_start: false,
            });
        }

        // Split notes that extend past the barline
        let mut next_carry: Vec<NoteEvent> = Vec::new();
        for ev in &mut measure_notes {
            if ev.note.end_tick > m_end {
                let mut carried = ev.clone();
                carried.note.start_tick = m_end;
                carried.tie_stop = true;
                next_carry.push(carried);
                ev.note.end_tick = m_end;
                ev.tie_start = true;
            }
        }
        carry_over = next_carry;

        // ── Voice separation ─────────────────────────────────────────────────
        // Drum parts stay single-voice; pitched parts are split into two voices
        // when overlapping note durations indicate independent rhythmic layers.
        let (v1_events, v2_events, v3_events, v4_events, v5_events, v6_events) =
            separate_voices(measure_notes, m_start, tpq);

        // Re-sort each voice: start_tick asc, end_tick desc (longest note = leader), pitch desc
        let voice_sort = |a: &NoteEvent, b: &NoteEvent| {
            a.note
                .start_tick
                .cmp(&b.note.start_tick)
                .then(b.note.end_tick.cmp(&a.note.end_tick))
                .then(b.note.pitch.cmp(&a.note.pitch))
        };
        let mut v1_sorted = v1_events;
        v1_sorted.sort_by(voice_sort);
        let mut v2_sorted = v2_events;
        v2_sorted.sort_by(voice_sort);
        let mut v3_sorted = v3_events;
        v3_sorted.sort_by(voice_sort);
        let mut v4_sorted = v4_events;
        v4_sorted.sort_by(voice_sort);
        let mut v5_sorted = v5_events;
        v5_sorted.sort_by(voice_sort);
        let mut v6_sorted = v6_events;
        v6_sorted.sort_by(voice_sort);

        let chord_groups_v1 = group_chords(v1_sorted);
        let chord_groups_v2 = group_chords(v2_sorted);
        let chord_groups_v3 = group_chords(v3_sorted);
        let chord_groups_v4 = group_chords(v4_sorted);
        let chord_groups_v5 = group_chords(v5_sorted);
        let chord_groups_v6 = group_chords(v6_sorted);

        // ── Build element list ───────────────────────────────────────────────

        let mut elements: Vec<MeasureElement> = Vec::new();

        // Mid-piece attribute changes (key, time sig)
        if !first_measure {
            let key_changed = key_fifths != prev_key_fifths;
            let ts_changed = (ts_num, ts_den) != prev_ts;
            let tempo_changed = tempo_us != prev_tempo_us;

            if key_changed || ts_changed {
                elements.push(MeasureElement::Attributes(Attributes {
                    key: if key_changed {
                        Some(Key {
                            fifths: key_fifths as i32,
                            mode: None,
                            key_accidentals: Vec::new(),
                        })
                    } else {
                        None
                    },
                    time: if ts_changed {
                        Some(Time {
                            beats: ts_num.to_string(),
                            beat_type: ts_den as i32,
                        })
                    } else {
                        None
                    },
                    ..Default::default()
                }));
            }
            if tempo_changed {
                let bpm = 60_000_000.0 / tempo_us as f64;
                elements.push(MeasureElement::Sound(Sound {
                    tempo: Some(bpm as f32),
                    ..Default::default()
                }));
            }
        } else {
            // First measure: emit tempo as Sound before everything else
            let bpm = 60_000_000.0 / tempo_us as f64;
            elements.push(MeasureElement::Sound(Sound {
                tempo: Some(bpm as f32),
                ..Default::default()
            }));
        }

        // Measure-level dynamics (median velocity across all notes in the measure)
        let velocities: Vec<u8> = chord_groups_v1
            .iter()
            .chain(chord_groups_v2.iter())
            .chain(chord_groups_v3.iter())
            .chain(chord_groups_v4.iter())
            .chain(chord_groups_v5.iter())
            .chain(chord_groups_v6.iter())
            .flat_map(|g| g.iter().map(|e| e.note.velocity))
            .collect();
        if !velocities.is_empty() {
            let dyn_str = velocity_to_dynamics(median_velocity(&velocities));
            if prev_dynamics != Some(dyn_str) {
                elements.push(MeasureElement::Direction(Direction {
                    placement: Some("below".to_string()),
                    types: vec![DirectionType::Dynamics(vec![dyn_str.to_string()])],
                    staff: Some(1),
                }));
                prev_dynamics = Some(dyn_str);
            }
        }

        let beat_ticks = beat_unit_ticks(ts_num, ts_den, tpq);

        // Voice 1 notes and rests
        let v1_raw = fill_and_convert(
            &chord_groups_v1,
            m_start,
            m_end,
            tpq,
            key_fifths,
            is_drum,
            measure_len,
            tolerance,
            &clef.sign,
            1,
        );
        let mut v1 = beat_split_elements(v1_raw, m_start, beat_ticks, tpq);
        apply_tuplet_notations(&mut v1);
        elements.extend(v1);

        // Voice 2: rewind with Backup then lay out from measure start
        if !chord_groups_v2.is_empty() {
            elements.push(MeasureElement::Backup(measure_len as i32));
            let v2_raw = fill_and_convert(
                &chord_groups_v2,
                m_start,
                m_end,
                tpq,
                key_fifths,
                is_drum,
                measure_len,
                tolerance,
                &clef.sign,
                2,
            );
            let mut v2 = beat_split_elements(v2_raw, m_start, beat_ticks, tpq);
            apply_tuplet_notations(&mut v2);
            elements.extend(v2);
        }

        // Voice 3: rewind with Backup then lay out from measure start
        if !chord_groups_v3.is_empty() {
            elements.push(MeasureElement::Backup(measure_len as i32));
            let v3_raw = fill_and_convert(
                &chord_groups_v3,
                m_start,
                m_end,
                tpq,
                key_fifths,
                is_drum,
                measure_len,
                tolerance,
                &clef.sign,
                3,
            );
            let mut v3 = beat_split_elements(v3_raw, m_start, beat_ticks, tpq);
            apply_tuplet_notations(&mut v3);
            elements.extend(v3);
        }

        // Voice 4: overflow slot — notes that would cause force-chord in v3
        if !chord_groups_v4.is_empty() {
            elements.push(MeasureElement::Backup(measure_len as i32));
            let v4_raw = fill_and_convert(
                &chord_groups_v4,
                m_start,
                m_end,
                tpq,
                key_fifths,
                is_drum,
                measure_len,
                tolerance,
                &clef.sign,
                4,
            );
            let mut v4 = beat_split_elements(v4_raw, m_start, beat_ticks, tpq);
            apply_tuplet_notations(&mut v4);
            elements.extend(v4);
        }

        // Voice 5: second overflow slot — notes that would cause force-chord in v4
        if !chord_groups_v5.is_empty() {
            elements.push(MeasureElement::Backup(measure_len as i32));
            let v5_raw = fill_and_convert(
                &chord_groups_v5,
                m_start,
                m_end,
                tpq,
                key_fifths,
                is_drum,
                measure_len,
                tolerance,
                &clef.sign,
                5,
            );
            let mut v5 = beat_split_elements(v5_raw, m_start, beat_ticks, tpq);
            apply_tuplet_notations(&mut v5);
            elements.extend(v5);
        }

        // Voice 6: third overflow slot — notes that would cause force-chord in v5
        if !chord_groups_v6.is_empty() {
            elements.push(MeasureElement::Backup(measure_len as i32));
            let v6_raw = fill_and_convert(
                &chord_groups_v6,
                m_start,
                m_end,
                tpq,
                key_fifths,
                is_drum,
                measure_len,
                tolerance,
                &clef.sign,
                6,
            );
            let mut v6 = beat_split_elements(v6_raw, m_start, beat_ticks, tpq);
            apply_tuplet_notations(&mut v6);
            elements.extend(v6);
        }

        // ── Attributes on first measure ──────────────────────────────────────
        let attrs = if first_measure {
            Some(Attributes {
                divisions: Some(tpq as i32),
                key: Some(Key {
                    fifths: key_fifths as i32,
                    mode: None,
                    key_accidentals: Vec::new(),
                }),
                time: Some(Time {
                    beats: ts_num.to_string(),
                    beat_type: ts_den as i32,
                }),
                clefs: vec![clef.clone()],
                ..Default::default()
            })
        } else {
            None
        };

        // Pickup measure detection: if all notes of the part start after tick 0
        // and before the first barline, mark measure 0 as implicit
        let is_pickup = measure_idx == 0
            && notes
                .first()
                .map(|n| n.start_tick > 0 && n.start_tick < m_end)
                .unwrap_or(false);
        let number = if is_pickup {
            "0".to_string()
        } else {
            (measure_idx + 1).to_string()
        };

        measures.push(Measure {
            number,
            attributes: attrs,
            elements,
            // `implicit: false` here previously left the pickup measure
            // looking like a full-length measure downstream — MIDI
            // regeneration then padded it out to the full nominal duration
            // (e.g. 4 beats for 4/4) instead of its actual short length,
            // shifting every subsequent event later by the padding amount.
            implicit: is_pickup,
        });

        prev_key_fifths = key_fifths;
        prev_ts = (ts_num, ts_den);
        prev_tempo_us = tempo_us;
        first_measure = false;
    }

    measures
}

fn group_chords(events: Vec<NoteEvent>) -> Vec<Vec<NoteEvent>> {
    let mut groups: Vec<Vec<NoteEvent>> = Vec::new();
    for ev in events {
        if let Some(last) = groups.last_mut() {
            if last[0].note.start_tick == ev.note.start_tick {
                last.push(ev);
                continue;
            }
        }
        groups.push(vec![ev]);
    }
    groups
}

fn fill_and_convert(
    chord_groups: &[Vec<NoteEvent>],
    measure_start: u64,
    measure_end: u64,
    tpq: u32,
    key_fifths: i8,
    is_drum: bool,
    measure_len: u64,
    tolerance: u64,
    clef_sign: &str,
    voice: i32,
) -> Vec<MeasureElement> {
    let mut elements = Vec::new();
    let mut cursor = measure_start;

    // For secondary voices, anchor fill-in rests at the average pitch of the voice's notes
    // so they appear near the notes rather than at a fixed staff position.
    let rest_anchor: Option<(String, i32)> = if voice > 1 && !chord_groups.is_empty() {
        let pitches: Vec<u8> = chord_groups
            .iter()
            .flat_map(|g| g.iter().map(|e| e.note.pitch))
            .collect();
        let avg = (pitches.iter().map(|&p| p as u32).sum::<u32>() / pitches.len() as u32) as u8;
        let p = midi_to_pitch(avg, key_fifths);
        Some((p.step, p.octave))
    } else {
        None
    };

    for group in chord_groups {
        let group_start = group[0].note.start_tick;

        if group_start > cursor {
            let mut rests = make_rests(group_start - cursor, tpq, tolerance, voice);
            apply_rest_anchor(&mut rests, &rest_anchor);
            elements.extend(rests);
        }

        let group_end = group
            .iter()
            .map(|e| e.note.end_tick)
            .max()
            .unwrap_or(measure_end);
        // If group_start < cursor the note overlaps the previous group. Force it
        // as a chord of the previous group to avoid overflowing the cursor.
        let force_chord = group_start < cursor;

        for (i, ev) in group.iter().enumerate() {
            let note = note_from_event(
                ev,
                tpq,
                key_fifths,
                is_drum,
                i > 0 || force_chord,
                tolerance,
                clef_sign,
                voice,
            );
            elements.push(MeasureElement::Note(note));
        }

        if !force_chord {
            cursor = group_end;
        }
    }

    if cursor < measure_end {
        let gap = measure_end - cursor;
        if gap == measure_len && chord_groups.is_empty() {
            elements.extend(make_measure_rest(gap, tpq, voice));
        } else {
            let mut rests = make_rests(gap, tpq, tolerance, voice);
            apply_rest_anchor(&mut rests, &rest_anchor);
            elements.extend(rests);
        }
    }

    elements
}

/// Ticks per beat for the given time signature.
/// Simple time (4/4, 3/4, 2/4): beat = quarter * (4/denominator)
/// Compound time (6/8, 9/8, 12/8): beat = 3 × eighth
fn beat_unit_ticks(ts_num: u8, ts_den: u8, tpq: u32) -> u64 {
    let beat_type_ticks = tpq as u64 * 4 / ts_den.max(1) as u64;
    if ts_num % 3 == 0 && ts_num > 3 {
        3 * beat_type_ticks
    } else {
        beat_type_ticks
    }
}

/// Split notes and rests that cross beat boundaries.
/// Notes are split and connected with ties; rests are split into smaller rests.
fn beat_split_elements(
    elements: Vec<MeasureElement>,
    measure_start: u64,
    beat_ticks: u64,
    tpq: u32,
) -> Vec<MeasureElement> {
    if beat_ticks == 0 {
        return elements;
    }

    let mut result: Vec<MeasureElement> = Vec::new();
    let mut cursor = measure_start;
    let mut i = 0;

    while i < elements.len() {
        // Only Note elements with is_chord=false start a new time slot
        let is_leader = matches!(&elements[i], MeasureElement::Note(n) if !n.is_chord);
        if !is_leader {
            if let MeasureElement::Note(_) = &elements[i] {
                // is_chord note without preceding leader: pass through unchanged
            }
            result.push(elements[i].clone());
            i += 1;
            continue;
        }

        // Collect full chord group: this note + consecutive is_chord followers
        let mut j = i + 1;
        while j < elements.len() {
            if let MeasureElement::Note(cn) = &elements[j] {
                if cn.is_chord {
                    j += 1;
                    continue;
                }
            }
            break;
        }

        let leader = match &elements[i] {
            MeasureElement::Note(n) => n,
            _ => unreachable!(),
        };

        let dur = leader.duration as u64;
        let note_start = cursor;
        let note_end = cursor + dur;
        cursor = note_end;

        // Tuplet notes (time_modification is Some) are already a valid single
        // note value; splitting them at a beat boundary would break the tuplet
        // into pieces with wrong durations (e.g. a triplet quarter at tick 320
        // would become two triplet eighths).  Skip the split for those.
        let is_tuplet = leader.time_modification.is_some();
        let off_beat_start = (note_start - measure_start) % beat_ticks != 0;
        let off_beat_end = (note_end - measure_start) % beat_ticks != 0;
        let should_split = !is_tuplet && (off_beat_start || off_beat_end);

        // Find beat boundaries strictly inside (note_start, note_end).
        // Exclude boundaries that would produce a segment smaller than a 64th note
        // (tpq/16 ticks): decompose_duration can't represent such segments, which
        // causes beat_split_elements to emit zero elements for the first segment
        // while still marking the second segment as tie_stop — silencing the note.
        let min_seg = (tpq / 16) as u64; // = 30 ticks at tpq=480
        let first_beat_idx = (note_start - measure_start) / beat_ticks + 1;
        let mut boundaries: Vec<u64> = Vec::new();
        if should_split {
            let mut k = first_beat_idx;
            loop {
                let b = measure_start + k * beat_ticks;
                if b >= note_end {
                    break;
                }
                if b > note_start && b - note_start >= min_seg && note_end - b >= min_seg {
                    boundaries.push(b);
                }
                k += 1;
            }
        }

        if boundaries.is_empty() || leader.rest_measure {
            // No split needed
            result.extend_from_slice(&elements[i..j]);
            i = j;
            continue;
        }

        // Build segment list: [note_start, b0, b1, ..., note_end]
        let mut segs: Vec<u64> = Vec::with_capacity(boundaries.len() + 2);
        segs.push(note_start);
        segs.extend_from_slice(&boundaries);
        segs.push(note_end);
        let n_segs = segs.len() - 1;

        // Collect chord notes (as references)
        let chord: Vec<&Note> = (i..j)
            .filter_map(|k| {
                if let MeasureElement::Note(n) = &elements[k] {
                    Some(n)
                } else {
                    None
                }
            })
            .collect();
        let is_rest = leader.rest;

        // Each chord member may have a different duration (the leader is the longest).
        // Pre-compute each member's absolute end tick so we can clamp its contribution
        // to its own duration rather than always using the leader's segment length.
        let member_end_ticks: Vec<u64> = chord
            .iter()
            .map(|src| note_start + src.duration as u64)
            .collect();

        for s in 0..n_segs {
            let seg_start = segs[s];
            let seg_end = segs[s + 1];
            let seg_dur = seg_end - seg_start;
            let is_first_seg = s == 0;

            let parts = decompose_duration(seg_dur, tpq);
            let n_parts = parts.len();

            let mut cumulative: u64 = 0;
            for (p, (note_type, dot_count, tm)) in parts.into_iter().enumerate() {
                let is_first_part = p == 0;
                let is_last_part = p == n_parts - 1;
                let part_dur_u64 = note_type_to_ticks(&note_type, dot_count, tpq) as u64;
                let part_start = seg_start + cumulative;
                let part_end = part_start + part_dur_u64;
                cumulative += part_dur_u64;

                // Track which active member is first (becomes the chord leader for this part)
                let mut chord_idx = 0usize;

                for (ci, src) in chord.iter().enumerate() {
                    let src_end = member_end_ticks[ci];

                    // Skip members that ended before this part
                    if src_end <= part_start {
                        continue;
                    }

                    // Clamp to this member's own end tick if shorter than the leader
                    let member_part_end = part_end.min(src_end);
                    let member_part_dur = member_part_end - part_start;
                    // True when this is this member's last emission (its tie chain ends here).
                    // decompose_duration may leave a small tick remainder, making part_end < seg_end
                    // even at the last part.  Guard with is_last_part so the member's tie chain
                    // closes correctly even when cumulative < seg_dur.
                    let member_is_last =
                        part_end >= src_end || (is_last_part && src_end <= seg_end);

                    // Compute the timing duration for this member.
                    // For the chord leader (ci=0) at the last part of any segment: use the
                    // actual remaining ticks (seg_end - part_start) rather than part_dur_u64.
                    // decompose_duration may leave a remainder, causing part_dur_u64 < actual
                    // remaining ticks.  Using the raw remainder keeps p_tick on the MIDI
                    // engine's cursor aligned to segment boundaries and prevents cumulative
                    // start-beat drift in subsequent notes.
                    let timing_dur = if ci == 0 && is_last_part {
                        (seg_end - part_start) as i32
                    } else {
                        part_dur_u64 as i32
                    };

                    // Recompute note_type/duration for members shorter than the leader
                    let (effective_nt, effective_dc, effective_tm, effective_dur) =
                        if member_part_dur == part_dur_u64 {
                            (note_type.clone(), dot_count, tm.clone(), timing_dur)
                        } else {
                            let mparts = decompose_duration(member_part_dur, tpq);
                            if let Some((mt, md, mtm)) = mparts.into_iter().next() {
                                let md_dur = note_type_to_ticks(&mt, md, tpq) as i32;
                                (mt, md, mtm, md_dur)
                            } else {
                                (
                                    note_type.clone(),
                                    dot_count,
                                    tm.clone(),
                                    member_part_dur as i32,
                                )
                            }
                        };

                    let mut n = (*src).clone();
                    n.duration = effective_dur;
                    n.note_type = Some(effective_nt);
                    n.dot_count = effective_dc as i32;
                    n.time_modification = effective_tm;
                    n.beams = Vec::new();
                    n.is_chord = chord_idx > 0;
                    chord_idx += 1;

                    if !is_rest {
                        // Accidentals only on the very first note in the tie chain
                        if !(is_first_seg && is_first_part) {
                            n.accidental = None;
                        }

                        // Preserve incoming tie chain endpoints before clearing.
                        // A note arriving with tie_stop came from a previous barline
                        // carry-over; the first segment of the split must inherit it so
                        // that no spurious NoteOn is emitted in the MIDI re-export.
                        // Similarly, a note with tie_start whose last segment is split
                        // must pass tie_start to the last piece.
                        let had_tie_stop = n.notations.iter().any(
                            |no| matches!(no, Notation::Tied { note_type } if note_type == "stop"),
                        );
                        let had_tie_start = n.notations.iter().any(
                            |no| matches!(no, Notation::Tied { note_type } if note_type == "start"),
                        );

                        n.notations
                            .retain(|no| !matches!(no, Notation::Tied { .. }));
                        if !(is_first_seg && is_first_part) || had_tie_stop {
                            n.notations.push(Notation::Tied {
                                note_type: "stop".to_string(),
                            });
                        }
                        // tie_start unless this is this member's last emission
                        if !member_is_last || had_tie_start {
                            n.notations.push(Notation::Tied {
                                note_type: "start".to_string(),
                            });
                        }
                    }

                    result.push(MeasureElement::Note(n));
                }
            }
        }
        i = j;
    }

    result
}

fn apply_rest_anchor(elements: &mut [MeasureElement], anchor: &Option<(String, i32)>) {
    if let Some((step, octave)) = anchor {
        for el in elements {
            if let MeasureElement::Note(n) = el {
                if n.rest {
                    n.unpitched = Some(Unpitched {
                        display_step: step.clone(),
                        display_octave: *octave,
                        midi_number: None,
                    });
                }
            }
        }
    }
}

fn note_from_event(
    ev: &NoteEvent,
    tpq: u32,
    key_fifths: i8,
    is_drum: bool,
    is_chord: bool,
    tolerance: u64,
    clef_sign: &str,
    voice: i32,
) -> Note {
    let tick_dur = ev.note.end_tick.saturating_sub(ev.note.start_tick);
    let duration = tick_dur as i32;

    let (note_type, dot_count, time_modification) =
        ticks_to_note_value_tolerant(tick_dur, tpq, tolerance)
            .or_else(|| decompose_duration(tick_dur, tpq).into_iter().next())
            .unwrap_or_else(|| ("quarter".to_string(), 0, None));

    let (pitch, unpitched, notehead, stem) = if is_drum {
        let nh = drum_notehead(ev.note.pitch).map(|v| Notehead {
            value: v.to_string(),
            filled: Some(true),
        });
        (None, Some(drum_position(ev.note.pitch)), nh, None)
    } else {
        // Stem direction depends on clef midline AND voice.
        // Voice 1 (primary): up below the midline, down above.
        // Voice 2 (secondary): always the opposite direction from voice 1.
        // Treble ("G") midline = B4 (MIDI 71). Bass ("F") midline = D2 (MIDI 38).
        let s = match (clef_sign, voice) {
            ("G", 1) => {
                if ev.note.pitch < 71 {
                    "up"
                } else {
                    "down"
                }
            }
            ("G", _) => "down", // voice 2 in treble always stems down
            ("F", 1) => {
                if ev.note.pitch < 38 {
                    "up"
                } else {
                    "down"
                }
            }
            ("F", _) => "up", // voice 2 in bass always stems up
            _ => {
                if ev.note.pitch >= 60 {
                    "up"
                } else {
                    "down"
                }
            }
        };
        (
            Some(midi_to_pitch(ev.note.pitch, key_fifths)),
            None,
            None,
            Some(s.to_string()),
        )
    };

    let mut notations = Vec::new();
    if ev.tie_stop {
        notations.push(Notation::Tied {
            note_type: "stop".to_string(),
        });
    }
    if ev.tie_start {
        notations.push(Notation::Tied {
            note_type: "start".to_string(),
        });
    }

    Note {
        pitch,
        unpitched,
        duration,
        voice: Some(voice),
        staff: Some(1),
        stem,
        note_type: Some(note_type),
        notehead,
        rest: false,
        rest_measure: false,
        is_chord,
        is_cue: false,
        grace: None,
        dot_count: dot_count as i32,
        lyrics: Vec::new(),
        beams: Vec::new(),
        notations,
        accidental: None,
        time_modification,
        print_object: None,
        print_dot: None,
        harmonies: Vec::new(),
        instrument: None,
    }
}

fn make_rests(ticks: u64, tpq: u32, _tolerance: u64, voice: i32) -> Vec<MeasureElement> {
    let parts = decompose_duration(ticks, tpq);
    let mut elements = Vec::new();
    let mut total = 0u64;

    for (note_type, dot_count, tm) in parts {
        let dur_ticks = note_type_to_ticks(&note_type, dot_count, tpq);
        total += dur_ticks;
        elements.push(MeasureElement::Note(Note {
            pitch: None,
            unpitched: None,
            duration: dur_ticks as i32,
            voice: Some(voice),
            staff: Some(1),
            stem: None,
            note_type: Some(note_type),
            notehead: None,
            rest: true,
            rest_measure: false,
            is_chord: false,
            is_cue: false,
            grace: None,
            dot_count: dot_count as i32,
            lyrics: Vec::new(),
            beams: Vec::new(),
            notations: Vec::new(),
            accidental: None,
            time_modification: tm,
            print_object: None,
            print_dot: None,
            harmonies: Vec::new(),
            instrument: None,
        }));
    }

    // decompose_duration may leave a small remainder; absorb it into the last
    // rest's duration so the cursor stays exactly at the correct tick position.
    let remainder = ticks.saturating_sub(total);
    if remainder > 0 {
        if let Some(MeasureElement::Note(n)) = elements.last_mut() {
            n.duration += remainder as i32;
        } else {
            // No parts at all (ticks was too small to decompose); emit one tiny rest.
            elements.push(MeasureElement::Note(Note {
                pitch: None,
                unpitched: None,
                duration: ticks as i32,
                voice: Some(voice),
                staff: Some(1),
                stem: None,
                note_type: Some("64th".to_string()),
                notehead: None,
                rest: true,
                rest_measure: false,
                is_chord: false,
                is_cue: false,
                grace: None,
                dot_count: 0,
                lyrics: Vec::new(),
                beams: Vec::new(),
                notations: Vec::new(),
                accidental: None,
                time_modification: None,
                print_object: None,
                print_dot: None,
                harmonies: Vec::new(),
                instrument: None,
            }));
        }
    }

    elements
}

fn make_measure_rest(ticks: u64, _tpq: u32, voice: i32) -> Vec<MeasureElement> {
    // A single whole-measure rest regardless of time signature
    let dur = ticks as i32;
    vec![MeasureElement::Note(Note {
        pitch: None,
        unpitched: None,
        duration: dur,
        voice: Some(voice),
        staff: Some(1),
        stem: None,
        note_type: Some("whole".to_string()),
        notehead: None,
        rest: true,
        rest_measure: true,
        is_chord: false,
        is_cue: false,
        grace: None,
        dot_count: 0,
        lyrics: Vec::new(),
        beams: Vec::new(),
        notations: Vec::new(),
        accidental: None,
        time_modification: None,
        print_object: None,
        print_dot: None,
        harmonies: Vec::new(),
        instrument: None,
    })]
}

/// Convert a note_type + dot_count back to ticks (inverse of ticks_to_note_value).
fn note_type_to_ticks(note_type: &str, dot_count: u8, tpq: u32) -> u64 {
    let base: u64 = match note_type {
        "breve" => tpq as u64 * 8,
        "whole" => tpq as u64 * 4,
        "half" => tpq as u64 * 2,
        "quarter" => tpq as u64,
        "eighth" => tpq as u64 / 2,
        "16th" => tpq as u64 / 4,
        "32nd" => tpq as u64 / 8,
        "64th" => tpq as u64 / 16,
        _ => tpq as u64,
    };
    match dot_count {
        0 => base,
        1 => base * 3 / 2,
        2 => base * 7 / 4,
        _ => base,
    }
}

/// Scan a measure's elements and add `Notation::Tuplet` start/stop markers to
/// groups of notes that share the same `TimeModification`.
///
/// Groups are formed by collecting non-chord note/rest elements that carry a
/// `time_modification`, then slicing them into groups of `actual_notes` length.
/// The first element of each group receives a `"start"` tuplet notation and the
/// last receives a `"stop"` tuplet notation.  Only complete groups are marked.
fn apply_tuplet_notations(elements: &mut Vec<MeasureElement>) {
    // Collect (element_index, actual_notes, normal_notes) for each "beat unit"
    // (non-chord notes or rests that carry time_modification).
    let beat_units: Vec<(usize, i32, i32)> = elements
        .iter()
        .enumerate()
        .filter_map(|(i, el)| {
            if let MeasureElement::Note(n) = el {
                if !n.is_chord {
                    if let Some(tm) = &n.time_modification {
                        return Some((i, tm.actual_notes, tm.normal_notes));
                    }
                }
            }
            None
        })
        .collect();

    if beat_units.is_empty() {
        return;
    }

    // Walk through beat_units, grouping consecutive entries with the same
    // (actual, normal) pair into groups of `actual` length.
    let mut pos = 0;
    while pos < beat_units.len() {
        let (_, actual, normal) = beat_units[pos];
        if actual <= 0 {
            pos += 1;
            continue;
        }
        let group_size = actual as usize;
        let end = pos + group_size;
        if end > beat_units.len() {
            break;
        }
        // All entries in this window must have the same (actual, normal).
        let all_match = beat_units[pos..end]
            .iter()
            .all(|&(_, a, n)| a == actual && n == normal);
        if !all_match {
            pos += 1;
            continue;
        }

        let start_idx = beat_units[pos].0;
        let stop_idx = beat_units[end - 1].0;

        if let MeasureElement::Note(n) = &mut elements[start_idx] {
            n.notations.push(Notation::Tuplet {
                number: Some(1),
                note_type: "start".to_string(),
                bracket: None,
                placement: None,
                show_number: None,
                actual_notes: Some(actual),
                normal_notes: Some(normal),
            });
        }
        if let MeasureElement::Note(n) = &mut elements[stop_idx] {
            n.notations.push(Notation::Tuplet {
                number: Some(1),
                note_type: "stop".to_string(),
                bracket: None,
                placement: None,
                show_number: None,
                actual_notes: Some(actual),
                normal_notes: Some(normal),
            });
        }
        pos = end;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::midi_parser::event::{
        KeySigChange, ParsedMidi, PartInfo, RawNote, TempoChange, TimeSigChange,
    };

    fn make_parsed(tpq: u32, notes: Vec<RawNote>) -> ParsedMidi {
        let total_ticks = notes.iter().map(|n| n.end_tick).max().unwrap_or(0);
        ParsedMidi {
            tpq,
            format: 1,
            notes,
            tempo_changes: vec![TempoChange {
                tick: 0,
                us_per_beat: 500_000,
            }],
            time_sig_changes: vec![TimeSigChange {
                tick: 0,
                numerator: 4,
                denominator: 4,
            }],
            key_sig_changes: vec![KeySigChange {
                tick: 0,
                fifths: 0,
                minor: false,
            }],
            parts: vec![PartInfo {
                name: Some("Piano".into()),
                program: 0,
                is_drum: false,
            }],
            total_ticks,
        }
    }

    fn rawnote(pitch: u8, start: u64, end: u64) -> RawNote {
        RawNote {
            part_idx: 0,
            channel: 0,
            pitch,
            velocity: 80,
            start_tick: start,
            end_tick: end,
        }
    }

    #[test]
    fn test_build_single_quarter_note() {
        // C4 quarter note in 4/4 at 480 tpq
        let tpq = 480u32;
        let notes = vec![rawnote(60, 0, 480)];
        let parsed = make_parsed(tpq, notes);
        let score = build(&parsed);

        assert_eq!(score.parts.len(), 1);
        let part = &score.parts[0];
        // measure_boundaries for 4/4 at 480 tpq, total=480 → 1 measure [0,1920)
        assert!(!part.measures.is_empty());
        let measure = &part.measures[0];

        // Should have a sound element, possibly a direction, and notes
        let note_elements: Vec<_> = measure
            .elements
            .iter()
            .filter_map(|e| {
                if let MeasureElement::Note(n) = e {
                    Some(n)
                } else {
                    None
                }
            })
            .collect();

        // 1 real note + 1 rest (fill remaining 1440 ticks)
        let real_notes: Vec<_> = note_elements.iter().filter(|n| !n.rest).collect();
        assert_eq!(real_notes.len(), 1);
        let n = real_notes[0];
        assert_eq!(n.note_type.as_deref(), Some("quarter"));
        assert_eq!(n.duration, 480);
        assert!(!n.is_chord);

        let pitch = n.pitch.as_ref().unwrap();
        assert_eq!(pitch.step, "C");
        assert_eq!(pitch.octave, 4);
    }

    #[test]
    fn test_build_chord() {
        let tpq = 480u32;
        // C4 and E4 simultaneously
        let notes = vec![rawnote(60, 0, 480), rawnote(64, 0, 480)];
        let parsed = make_parsed(tpq, notes);
        let score = build(&parsed);

        let measure = &score.parts[0].measures[0];
        let note_els: Vec<_> = measure
            .elements
            .iter()
            .filter_map(|e| {
                if let MeasureElement::Note(n) = e {
                    Some(n)
                } else {
                    None
                }
            })
            .filter(|n| !n.rest)
            .collect();

        assert_eq!(note_els.len(), 2);
        // First note of chord: is_chord=false (highest pitch first after sort)
        assert!(!note_els[0].is_chord);
        // Second: is_chord=true
        assert!(note_els[1].is_chord);
    }

    #[test]
    fn test_build_tie_at_barline() {
        let tpq = 480u32;
        // Note spans from tick 1680 to 2400, crossing barline at 1920
        let notes = vec![rawnote(60, 1680, 2400)];
        let parsed = make_parsed(tpq, notes);
        let score = build(&parsed);

        let part = &score.parts[0];
        // Measure 1: note[1680, 1920] with tie_start
        // Measure 2: note[1920, 2400] with tie_stop
        let m1_notes: Vec<_> = part.measures[0]
            .elements
            .iter()
            .filter_map(|e| {
                if let MeasureElement::Note(n) = e {
                    Some(n)
                } else {
                    None
                }
            })
            .filter(|n| !n.rest)
            .collect();

        let m2_notes: Vec<_> = part.measures[1]
            .elements
            .iter()
            .filter_map(|e| {
                if let MeasureElement::Note(n) = e {
                    Some(n)
                } else {
                    None
                }
            })
            .filter(|n| !n.rest)
            .collect();

        assert_eq!(m1_notes.len(), 1, "measure 1 should have 1 real note");
        assert_eq!(m2_notes.len(), 1, "measure 2 should have 1 real note");

        let m1_note = m1_notes[0];
        let m2_note = m2_notes[0];

        let has_tie_start = m1_note
            .notations
            .iter()
            .any(|n| matches!(n, Notation::Tied { note_type } if note_type == "start"));
        let has_tie_stop = m2_note
            .notations
            .iter()
            .any(|n| matches!(n, Notation::Tied { note_type } if note_type == "stop"));

        assert!(has_tie_start, "first half should have tie start");
        assert!(has_tie_stop, "second half should have tie stop");
    }

    #[test]
    fn test_build_empty_measure_is_rest() {
        let tpq = 480u32;
        // Single note in first measure, second measure has no notes
        let notes = vec![rawnote(60, 0, 480)];
        let mut parsed = make_parsed(tpq, notes);
        // Force 2 measures by extending total_ticks
        parsed.total_ticks = 1920 * 2;
        let score = build(&parsed);

        let part = &score.parts[0];
        assert!(part.measures.len() >= 2);

        // Second measure should have a rest
        let m2 = &part.measures[1];
        let rests: Vec<_> = m2
            .elements
            .iter()
            .filter_map(|e| {
                if let MeasureElement::Note(n) = e {
                    Some(n)
                } else {
                    None
                }
            })
            .filter(|n| n.rest)
            .collect();
        assert!(!rests.is_empty(), "empty measure should have rest(s)");
    }

    #[test]
    fn test_build_part_list_name() {
        let tpq = 480u32;
        let notes = vec![rawnote(60, 0, 480)];
        let parsed = make_parsed(tpq, notes);
        let score = build(&parsed);

        if let PartListItem::Part { name, .. } = &score.part_list[0] {
            assert!(name.is_some());
        } else {
            panic!("expected PartListItem::Part");
        }
    }

    #[test]
    fn test_build_drum_part() {
        let tpq = 480u32;
        let notes = vec![RawNote {
            part_idx: 0,
            channel: 9,
            pitch: 38,
            velocity: 100,
            start_tick: 0,
            end_tick: 480,
        }];
        let total_ticks = 480;
        let parsed = ParsedMidi {
            tpq,
            format: 1,
            notes,
            tempo_changes: vec![TempoChange {
                tick: 0,
                us_per_beat: 500_000,
            }],
            time_sig_changes: vec![TimeSigChange {
                tick: 0,
                numerator: 4,
                denominator: 4,
            }],
            key_sig_changes: vec![KeySigChange {
                tick: 0,
                fifths: 0,
                minor: false,
            }],
            parts: vec![PartInfo {
                name: None,
                program: 0,
                is_drum: true,
            }],
            total_ticks,
        };
        let score = build(&parsed);

        let note_els: Vec<_> = score.parts[0].measures[0]
            .elements
            .iter()
            .filter_map(|e| {
                if let MeasureElement::Note(n) = e {
                    Some(n)
                } else {
                    None
                }
            })
            .filter(|n| !n.rest)
            .collect();

        assert_eq!(note_els.len(), 1);
        let n = note_els[0];
        assert!(n.pitch.is_none(), "drum note should have no pitch");
        assert!(n.unpitched.is_some(), "drum note should have unpitched");
    }

    #[test]
    fn test_build_attributes_on_first_measure() {
        let tpq = 480u32;
        let notes = vec![rawnote(60, 0, 480)];
        let parsed = make_parsed(tpq, notes);
        let score = build(&parsed);

        let m0 = &score.parts[0].measures[0];
        let attrs = m0
            .attributes
            .as_ref()
            .expect("first measure should have attributes");

        assert_eq!(attrs.divisions, Some(tpq as i32));
        assert!(attrs.key.is_some());
        assert!(attrs.time.is_some());
        assert!(!attrs.clefs.is_empty());
    }

    #[test]
    fn test_note_type_to_ticks_roundtrip() {
        let tpq = 480u32;
        for (nt, dots, expected) in &[
            ("quarter", 0u8, 480u64),
            ("quarter", 1, 720),
            ("quarter", 2, 840),
            ("half", 0, 960),
            ("half", 1, 1440),
            ("eighth", 0, 240),
            ("16th", 0, 120),
            ("32nd", 0, 60),
            ("64th", 0, 30),
            ("whole", 0, 1920),
        ] {
            let got = note_type_to_ticks(nt, *dots, tpq);
            assert_eq!(got, *expected, "{}+{} dots", nt, dots);
        }
    }

    #[test]
    fn test_velocity_to_dynamics() {
        assert_eq!(velocity_to_dynamics(0), "pppp");
        assert_eq!(velocity_to_dynamics(64), "mp");
        assert_eq!(velocity_to_dynamics(80), "mf");
        assert_eq!(velocity_to_dynamics(96), "f");
        assert_eq!(velocity_to_dynamics(127), "fff");
    }

    // ── Phase 6 tests ────────────────────────────────────────────────────────

    #[test]
    fn test_drum_cymbal_gets_x_notehead() {
        let tpq = 480u32;
        // MIDI 42 = Closed Hi-Hat → x notehead
        let notes = vec![RawNote {
            part_idx: 0,
            channel: 9,
            pitch: 42,
            velocity: 100,
            start_tick: 0,
            end_tick: 480,
        }];
        let total_ticks = 480;
        let parsed = ParsedMidi {
            tpq,
            format: 1,
            notes,
            tempo_changes: vec![TempoChange {
                tick: 0,
                us_per_beat: 500_000,
            }],
            time_sig_changes: vec![TimeSigChange {
                tick: 0,
                numerator: 4,
                denominator: 4,
            }],
            key_sig_changes: vec![KeySigChange {
                tick: 0,
                fifths: 0,
                minor: false,
            }],
            parts: vec![PartInfo {
                name: None,
                program: 0,
                is_drum: true,
            }],
            total_ticks,
        };
        let score = build(&parsed);

        let note_els: Vec<_> = score.parts[0].measures[0]
            .elements
            .iter()
            .filter_map(|e| {
                if let MeasureElement::Note(n) = e {
                    Some(n)
                } else {
                    None
                }
            })
            .filter(|n| !n.rest)
            .collect();

        assert_eq!(note_els.len(), 1);
        let n = note_els[0];
        let nh = n.notehead.as_ref().expect("hi-hat should have notehead");
        assert_eq!(nh.value, "x");
    }

    #[test]
    fn test_drum_snare_no_x_notehead() {
        let tpq = 480u32;
        // MIDI 38 = Acoustic Snare → normal (no x) notehead
        let notes = vec![RawNote {
            part_idx: 0,
            channel: 9,
            pitch: 38,
            velocity: 100,
            start_tick: 0,
            end_tick: 480,
        }];
        let total_ticks = 480;
        let parsed = ParsedMidi {
            tpq,
            format: 1,
            notes,
            tempo_changes: vec![TempoChange {
                tick: 0,
                us_per_beat: 500_000,
            }],
            time_sig_changes: vec![TimeSigChange {
                tick: 0,
                numerator: 4,
                denominator: 4,
            }],
            key_sig_changes: vec![KeySigChange {
                tick: 0,
                fifths: 0,
                minor: false,
            }],
            parts: vec![PartInfo {
                name: None,
                program: 0,
                is_drum: true,
            }],
            total_ticks,
        };
        let score = build(&parsed);

        let note_els: Vec<_> = score.parts[0].measures[0]
            .elements
            .iter()
            .filter_map(|e| {
                if let MeasureElement::Note(n) = e {
                    Some(n)
                } else {
                    None
                }
            })
            .filter(|n| !n.rest)
            .collect();

        let n = note_els[0];
        assert!(n.notehead.is_none(), "snare should have no x notehead");
    }

    #[test]
    fn test_mid_piece_timesig_change_emits_attributes() {
        use crate::midi_parser::event::{KeySigChange, PartInfo, TempoChange, TimeSigChange};
        let tpq = 480u32;
        // 4/4 → 3/4 at measure 2 (tick 1920)
        let notes = vec![rawnote(60, 0, 480), rawnote(60, 1920, 2400)];
        let total_ticks = 2400;
        let parsed = ParsedMidi {
            tpq,
            format: 1,
            notes,
            tempo_changes: vec![TempoChange {
                tick: 0,
                us_per_beat: 500_000,
            }],
            time_sig_changes: vec![
                TimeSigChange {
                    tick: 0,
                    numerator: 4,
                    denominator: 4,
                },
                TimeSigChange {
                    tick: 1920,
                    numerator: 3,
                    denominator: 4,
                },
            ],
            key_sig_changes: vec![KeySigChange {
                tick: 0,
                fifths: 0,
                minor: false,
            }],
            parts: vec![PartInfo {
                name: None,
                program: 0,
                is_drum: false,
            }],
            total_ticks,
        };
        let score = build(&parsed);

        let part = &score.parts[0];
        assert!(part.measures.len() >= 2, "need at least 2 measures");

        // Measure 2 should have a MeasureElement::Attributes with the new time sig
        let m2 = &part.measures[1];
        let has_attrs_element = m2.elements.iter().any(|e| {
            if let MeasureElement::Attributes(a) = e {
                a.time
                    .as_ref()
                    .map(|t| t.beat_type == 4 || t.beats == "3")
                    .unwrap_or(false)
            } else {
                false
            }
        });
        assert!(
            has_attrs_element,
            "measure 2 should have inline Attributes for time sig change"
        );
    }

    #[test]
    fn test_mid_piece_tempo_change_emits_sound() {
        use crate::midi_parser::event::{KeySigChange, PartInfo, TempoChange, TimeSigChange};
        let tpq = 480u32;
        let notes = vec![rawnote(60, 0, 480), rawnote(60, 1920, 2400)];
        let total_ticks = 2400;
        let parsed = ParsedMidi {
            tpq,
            format: 1,
            notes,
            tempo_changes: vec![
                TempoChange {
                    tick: 0,
                    us_per_beat: 500_000,
                }, // 120 BPM
                TempoChange {
                    tick: 1920,
                    us_per_beat: 400_000,
                }, // 150 BPM
            ],
            time_sig_changes: vec![TimeSigChange {
                tick: 0,
                numerator: 4,
                denominator: 4,
            }],
            key_sig_changes: vec![KeySigChange {
                tick: 0,
                fifths: 0,
                minor: false,
            }],
            parts: vec![PartInfo {
                name: None,
                program: 0,
                is_drum: false,
            }],
            total_ticks,
        };
        let score = build(&parsed);

        let part = &score.parts[0];
        assert!(part.measures.len() >= 2);

        // Measure 2 should contain a Sound element with the new tempo
        let m2 = &part.measures[1];
        let sound_el = m2.elements.iter().find_map(|e| {
            if let MeasureElement::Sound(s) = e {
                Some(s)
            } else {
                None
            }
        });
        assert!(
            sound_el.is_some(),
            "measure 2 should have Sound element for tempo change"
        );
        let bpm = sound_el.unwrap().tempo.unwrap();
        assert!(
            (bpm - 150.0).abs() < 1.0,
            "tempo should be ~150 BPM, got {}",
            bpm
        );
    }

    #[test]
    fn test_tuplet_notations_added() {
        // 6 triplet-eighth notes (160 ticks each at tpq=480) → 2 groups of 3
        let tpq = 480u32;
        let notes = vec![
            rawnote(60, 0, 160),
            rawnote(62, 160, 320),
            rawnote(64, 320, 480),
            rawnote(65, 480, 640),
            rawnote(67, 640, 800),
            rawnote(69, 800, 960),
        ];
        let parsed = make_parsed(tpq, notes);
        let score = build(&parsed);
        let m0 = &score.parts[0].measures[0];

        // Collect non-chord notes with tuplet notations
        let tuplet_starts: Vec<_> = m0.elements.iter().filter_map(|e| {
            if let MeasureElement::Note(n) = e {
                if n.notations.iter().any(|nt| matches!(nt, Notation::Tuplet { note_type, .. } if note_type == "start")) {
                    Some(n)
                } else { None }
            } else { None }
        }).collect();
        let tuplet_stops: Vec<_> = m0.elements.iter().filter_map(|e| {
            if let MeasureElement::Note(n) = e {
                if n.notations.iter().any(|nt| matches!(nt, Notation::Tuplet { note_type, .. } if note_type == "stop")) {
                    Some(n)
                } else { None }
            } else { None }
        }).collect();

        assert_eq!(tuplet_starts.len(), 2, "should have 2 tuplet start markers");
        assert_eq!(tuplet_stops.len(), 2, "should have 2 tuplet stop markers");
    }

    #[test]
    fn test_first_measure_has_sound_tempo() {
        let tpq = 480u32;
        let notes = vec![rawnote(60, 0, 480)];
        let parsed = make_parsed(tpq, notes);
        let score = build(&parsed);

        let m0 = &score.parts[0].measures[0];
        let has_sound = m0
            .elements
            .iter()
            .any(|e| matches!(e, MeasureElement::Sound(_)));
        assert!(
            has_sound,
            "first measure should always have Sound with tempo"
        );
    }
}
