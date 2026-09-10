use crate::instrument_maps::{get_program_from_name, get_program_from_sound};
use crate::models::{
    DirectionType, GroupSymbol, MeasureElement, MidiInstrument, Notation, Ornament, PartListItem,
    Pitch, Score, TechnicalMark, TimingEvent,
};
use midly::{MetaMessage, MidiMessage as MidlyMidiMessage, Smf, Track, TrackEvent};
use std::collections::{HashMap, HashSet};

pub struct MidiEngine;

const MIDI_TPQ: f32 = 480.0;
const MIDI_TPQ_U32: u32 = 480;

struct RawTrackEvent {
    tick: u32,
    kind: midly::TrackEventKind<'static>,
}

impl MidiEngine {
    pub fn generate_timing_map(score: &Score, timeline: &[usize]) -> Vec<TimingEvent> {
        let mut timing_map = Vec::new();
        let mut current_tempo = 120.0f32;
        let mut current_time_seconds = 0.0f32;
        let mut accumulated_beats_total = 0.0f32;

        let mut current_beats = 4.0f32;
        let mut current_beat_type = 4i32;
        let mut part_divisions = vec![1i32; score.parts.len()];

        for &m_idx in timeline {
            let measure_count = score.parts.first().map(|p| p.measures.len()).unwrap_or(0);
            if m_idx >= measure_count {
                continue;
            }

            let m_num = score
                .parts
                .first()
                .and_then(|p| p.measures.get(m_idx))
                .map(|m| m.number.clone())
                .unwrap_or_default();
            // println!("measure {}: timing_map {} sec", m_idx + 1, current_time_seconds);
            let measure_start_tempo = current_tempo;

            // Update Global Time Signature and Part-specific Divisions State.
            // Mid-piece changes are carried as a `MeasureElement::Attributes` inside
            // `measure.elements` (the `Measure.attributes` struct field is only ever
            // populated for each part's very first measure), so both must be scanned.
            for (p_idx, part) in score.parts.iter().enumerate() {
                if let Some(m) = part.measures.get(m_idx) {
                    let attr_iter = m
                        .attributes
                        .iter()
                        .chain(m.elements.iter().filter_map(|el| {
                            if let MeasureElement::Attributes(a) = el {
                                Some(a)
                            } else {
                                None
                            }
                        }));
                    for attr in attr_iter {
                        if let Some(time) = &attr.time {
                            current_beats = time
                                .beats
                                .split('+')
                                .map(|s| s.parse::<f32>().unwrap_or(0.0))
                                .sum();
                            current_beat_type = time.beat_type;
                        }
                        if let Some(div) = attr.divisions {
                            part_divisions[p_idx] = div;
                        }
                    }
                    // A tempo change at the very start of a measure (before any note)
                    // must take effect before `theoretical_duration_seconds` below is
                    // computed for *this* measure. Otherwise the stale pre-change tempo
                    // is used for the theoretical baseline, and since the final measure
                    // duration is `theoretical.max(actual)`, a tempo speed-up landing
                    // exactly on a measure boundary gets its duration inflated back to
                    // the slower, stale tempo — silently accumulating drift between the
                    // generated audio (tick-based, unaffected by this) and the on-screen
                    // playback cursor (seconds-based, driven by this timing map).
                    for el in &m.elements {
                        match el {
                            MeasureElement::Sound(s) => {
                                if let Some(t) = s.tempo {
                                    current_tempo = t;
                                }
                            }
                            MeasureElement::Direction(dir) => {
                                for dt in &dir.types {
                                    if let DirectionType::Metronome(met) = dt {
                                        if let Some(bpm) =
                                            met.bpm.as_ref().and_then(|s| s.parse::<f32>().ok())
                                        {
                                            let mut factor = 1.0;
                                            match met.beat_unit.as_str() {
                                                "half" => factor = 2.0,
                                                "whole" => factor = 4.0,
                                                "eighth" => factor = 0.5,
                                                "16th" => factor = 0.25,
                                                "32nd" => factor = 0.125,
                                                _ => {}
                                            }
                                            if met.beat_unit_dot > 0 {
                                                factor *= 1.5;
                                            }
                                            current_tempo = bpm * factor;
                                        }
                                    }
                                }
                            }
                            MeasureElement::Attributes(_) => {}
                            _ => break,
                        }
                    }
                }
            }

            let theoretical_duration_seconds =
                (current_beats * 4.0 / current_beat_type as f32) * (60.0 / current_tempo);
            let mut actual_measure_max_seconds = 0.0f32;
            let start_time_of_measure = current_time_seconds;

            // 1. Identify all interesting time points
            let mut time_points = std::collections::BTreeSet::new();
            for b in 0..current_beats.ceil() as i32 {
                time_points.insert(
                    (b as f64 * (MIDI_TPQ as f64 * 4.0 / current_beat_type as f64)).round() as i32,
                );
            }

            for (p_idx, part) in score.parts.iter().enumerate() {
                if let Some(measure) = part.measures.get(m_idx) {
                    let mut p_tick = 0;
                    let m_div = measure
                        .attributes
                        .as_ref()
                        .and_then(|a| a.divisions)
                        .unwrap_or(part_divisions[p_idx]);
                    for el in &measure.elements {
                        match el {
                            MeasureElement::Note(n) => {
                                if !n.is_chord {
                                    time_points.insert(p_tick);
                                    p_tick += (n.duration as f64 * MIDI_TPQ as f64 / m_div as f64)
                                        .round()
                                        as i32;
                                }
                            }
                            MeasureElement::Backup(d) => {
                                p_tick -=
                                    (*d as f64 * MIDI_TPQ as f64 / m_div as f64).round() as i32
                            }
                            MeasureElement::Forward(d) => {
                                p_tick +=
                                    (*d as f64 * MIDI_TPQ as f64 / m_div as f64).round() as i32
                            }
                            _ => {}
                        }
                    }
                }
            }

            // 2. Tempo changes
            let mut tempo_changes = std::collections::BTreeMap::new();
            for (p_idx, part) in score.parts.iter().enumerate() {
                if let Some(m) = part.measures.get(m_idx) {
                    let mut p_tick = 0;
                    let m_div = m
                        .attributes
                        .as_ref()
                        .and_then(|a| a.divisions)
                        .unwrap_or(part_divisions[p_idx]);
                    for el in &m.elements {
                        match el {
                            MeasureElement::Sound(s) => {
                                if let Some(t) = s.tempo {
                                    tempo_changes.insert(p_tick, t);
                                }
                            }
                            MeasureElement::Direction(dir) => {
                                for dt in &dir.types {
                                    if let DirectionType::Metronome(met) = dt {
                                        if let Some(bpm) =
                                            met.bpm.as_ref().and_then(|s| s.parse::<f32>().ok())
                                        {
                                            let mut factor = 1.0;
                                            match met.beat_unit.as_str() {
                                                "half" => factor = 2.0,
                                                "whole" => factor = 4.0,
                                                "eighth" => factor = 0.5,
                                                "16th" => factor = 0.25,
                                                "32nd" => factor = 0.125,
                                                _ => {}
                                            }
                                            if met.beat_unit_dot > 0 {
                                                factor *= 1.5;
                                            }
                                            tempo_changes.insert(p_tick, bpm * factor);
                                        }
                                    }
                                }
                            }
                            MeasureElement::Note(n) if !n.is_chord => {
                                p_tick += (n.duration as f64 * MIDI_TPQ as f64 / m_div as f64)
                                    .round() as i32
                            }
                            MeasureElement::Backup(d) => {
                                p_tick -=
                                    (*d as f64 * MIDI_TPQ as f64 / m_div as f64).round() as i32
                            }
                            MeasureElement::Forward(d) => {
                                p_tick +=
                                    (*d as f64 * MIDI_TPQ as f64 / m_div as f64).round() as i32
                            }
                            _ => {}
                        }
                    }
                }
            }

            // 3. Generate TimingEvents
            let sorted_points: Vec<i32> = time_points.into_iter().collect();
            let mut prev_tick = 0;
            for &t in &sorted_points {
                let duration_ticks = (t - prev_tick) as f32;
                current_time_seconds += duration_ticks * (60.0 / (current_tempo * MIDI_TPQ));

                if let Some(&new_tempo) = tempo_changes.get(&t) {
                    current_tempo = new_tempo;
                }

                let relative_beat = (t as f32 / MIDI_TPQ) * (current_beat_type as f32 / 4.0);
                timing_map.push(TimingEvent {
                    measure_index: m_idx,
                    measure_number: m_num.clone(),
                    beat_number: relative_beat + 1.0,
                    absolute_beat: accumulated_beats_total + relative_beat + 1.0,
                    time_seconds: current_time_seconds,
                    tick_offset: t,
                });
                prev_tick = t;
            }

            // 4. Update measure max seconds for next measure offset
            for (p_idx, part) in score.parts.iter().enumerate() {
                if let Some(measure) = part.measures.get(m_idx) {
                    let mut p_seconds = start_time_of_measure;
                    let mut p_tick = 0;
                    let mut local_tempo = measure_start_tempo;

                    let m_div = measure
                        .attributes
                        .as_ref()
                        .and_then(|a| a.divisions)
                        .unwrap_or(part_divisions[p_idx]);
                    for el in &measure.elements {
                        match el {
                            MeasureElement::Sound(s) => {
                                if let Some(t) = s.tempo {
                                    local_tempo = t;
                                }
                            }
                            MeasureElement::Direction(dir) => {
                                for dt in &dir.types {
                                    if let DirectionType::Metronome(met) = dt {
                                        if let Some(bpm) =
                                            met.bpm.as_ref().and_then(|s| s.parse::<f32>().ok())
                                        {
                                            let mut factor = 1.0;
                                            match met.beat_unit.as_str() {
                                                "half" => factor = 2.0,
                                                "whole" => factor = 4.0,
                                                "eighth" => factor = 0.5,
                                                "16th" => factor = 0.25,
                                                "32nd" => factor = 0.125,
                                                _ => {}
                                            }
                                            if met.beat_unit_dot > 0 {
                                                factor *= 1.5;
                                            }
                                            local_tempo = bpm * factor;
                                        }
                                    }
                                }
                            }
                            MeasureElement::Note(n) if !n.is_chord => {
                                p_seconds +=
                                    (n.duration as f32 / m_div as f32) * (60.0 / local_tempo);
                                p_tick += (n.duration as f64 * MIDI_TPQ as f64 / m_div as f64)
                                    .round() as i32;
                                if let Some(&new_t) = tempo_changes.get(&p_tick) {
                                    local_tempo = new_t;
                                }
                            }
                            MeasureElement::Backup(d) => {
                                p_seconds -= (*d as f32 / m_div as f32) * (60.0 / local_tempo);
                                p_tick -=
                                    (*d as f64 * MIDI_TPQ as f64 / m_div as f64).round() as i32;
                                if let Some(&new_t) = tempo_changes.get(&p_tick) {
                                    local_tempo = new_t;
                                }
                            }
                            MeasureElement::Forward(d) => {
                                p_seconds += (*d as f32 / m_div as f32) * (60.0 / local_tempo);
                                p_tick +=
                                    (*d as f64 * MIDI_TPQ as f64 / m_div as f64).round() as i32;
                                if let Some(&new_t) = tempo_changes.get(&p_tick) {
                                    local_tempo = new_t;
                                }
                            }
                            _ => {}
                        }
                    }
                    let consumed = p_seconds - start_time_of_measure;
                    if consumed > actual_measure_max_seconds {
                        actual_measure_max_seconds = consumed;
                    }
                }
            }

            let is_pickup = (m_idx == 0 || score.parts[0].measures[m_idx].implicit)
                && actual_measure_max_seconds < theoretical_duration_seconds - 0.001;
            let final_measure_duration = if is_pickup {
                actual_measure_max_seconds
            } else {
                theoretical_duration_seconds.max(actual_measure_max_seconds)
            };

            current_time_seconds = start_time_of_measure + final_measure_duration;
            accumulated_beats_total += current_beats;
        }
        timing_map
    }

    pub fn generate_smf(score: &Score, timeline: &[usize]) -> Smf<'static> {
        let mut smf = Smf::new(midly::Header::new(
            midly::Format::Parallel,
            midly::Timing::Metrical((MIDI_TPQ_U32 as u16).into()),
        ));

        // ── Identify brace-group membership ──────────────────────────────────
        // Parts inside a PartGroup with GroupSymbol::Brace represent a grand-staff
        // instrument (piano, harp, …).  They must share a single MIDI channel and
        // be exported as one track so the roundtrip note count matches the original.
        let mut brace_group_of: HashMap<String, i32> = HashMap::new(); // part_id → group#
        {
            let mut cur_group: Option<i32> = None;
            let mut cur_is_brace = false;
            for item in &score.part_list {
                match item {
                    PartListItem::Group(g) if g.group_type == "start" => {
                        cur_group = Some(g.number);
                        cur_is_brace = matches!(g.symbol, Some(GroupSymbol::Brace));
                    }
                    PartListItem::Group(g) if g.group_type == "stop" => {
                        if cur_group == Some(g.number) {
                            cur_group = None;
                            cur_is_brace = false;
                        }
                    }
                    PartListItem::Part { id, .. } => {
                        if cur_is_brace {
                            if let Some(gnum) = cur_group {
                                brace_group_of.insert(id.clone(), gnum);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        let mut channel_map = HashMap::new();
        let mut original_programs = HashMap::new();
        let mut part_midi_map = HashMap::new(); // part_id -> Vec<MidiInstrument>
        let mut ch_idx = 0;
        let mut group_channel: HashMap<i32, u8> = HashMap::new(); // group# → shared channel

        for part_item in &score.part_list {
            if let PartListItem::Part {
                id,
                instrument_sound,
                instrument_names,
                midi_instruments,
                name,
                ..
            } = part_item
            {
                let first_midi = midi_instruments.first();
                let mut program = Self::get_program_number(
                    instrument_sound.as_deref(),
                    first_midi,
                    name.as_deref(),
                    instrument_names,
                );

                let channel = if program >= 128 {
                    program = 0; // Standard drum kit
                    9
                } else if first_midi.and_then(|m| m.channel) == Some(10) {
                    program = 0;
                    9
                } else if let Some(&gnum) = brace_group_of.get(id) {
                    // Grand-staff group: all parts share the same channel
                    if let Some(&gc) = group_channel.get(&gnum) {
                        gc // reuse existing channel without consuming another slot
                    } else {
                        let c = if ch_idx == 9 { 10 } else { ch_idx } as u8;
                        ch_idx += 1;
                        group_channel.insert(gnum, c);
                        c
                    }
                } else {
                    let c = if ch_idx == 9 { 10 } else { ch_idx } as u8;
                    ch_idx += 1;
                    c
                };

                channel_map.insert(id.clone(), channel);
                original_programs.insert(id.clone(), program as u8);
                part_midi_map.insert(id.clone(), midi_instruments.clone());
            }
        }

        // Use absolute ticks for initial collection
        let mut meta_events: Vec<RawTrackEvent> = Vec::new();
        meta_events.push(RawTrackEvent {
            tick: 0,
            kind: midly::TrackEventKind::Meta(MetaMessage::TrackName(b"Tempo Track")),
        });

        let mut part_raw_events: Vec<Vec<RawTrackEvent>> = score
            .parts
            .iter()
            .map(|part| {
                let mut events = Vec::new();
                if let Some(&channel) = channel_map.get(&part.id) {
                    let program = *original_programs.get(&part.id).unwrap_or(&0);
                    events.push(RawTrackEvent {
                        tick: 0,
                        kind: midly::TrackEventKind::Midi {
                            channel: channel.into(),
                            message: MidlyMidiMessage::ProgramChange {
                                program: program.into(),
                            },
                        },
                    });
                }
                events
            })
            .collect();
        let mut part_velocities = vec![92u8; score.parts.len()];
        // Tracks whether each part is currently in pizzicato mode (from note attribute).
        // Used to insert program change only at the transition point.
        let mut part_pizzicato_state = vec![false; score.parts.len()];

        let mut global_tick_of_measure_start = 0u32;
        let midi_tpq = MIDI_TPQ_U32;

        let mut current_divisions = vec![1i32; score.parts.len()];
        let mut current_beats = 4.0f32;
        let mut current_beat_type = 4i32;
        let mut current_key_fifths = 0i32;
        let mut current_transpose_states: Vec<Option<crate::models::Transpose>> =
            vec![None; score.parts.len()];
        // Tracks the last (beats, beat_type) pair we actually wrote a MIDI
        // TimeSignature meta-event for, so a change is emitted exactly once
        // at the tick it takes effect (not re-emitted every measure it holds).
        let mut last_emitted_time_sig: Option<(f32, i32)> = None;

        for &m_idx in timeline {
            // let measure_seconds = (global_tick_of_measure_start as f32 / midi_tpq as f32) * 0.5;
            // println!("measure {}: smf {} sec", m_idx + 1, measure_seconds);
            // 1. Update Global Time Signature and Key State (from any part that has it).
            // Mid-piece changes are carried as a `MeasureElement::Attributes` inside
            // `measure.elements`, not on the `Measure.attributes` struct field (that
            // field is only ever populated for each part's very first measure) — so
            // both locations must be scanned or every attribute change after measure 0
            // is silently dropped during MIDI regeneration.
            for (p_idx, part) in score.parts.iter().enumerate() {
                if let Some(m) = part.measures.get(m_idx) {
                    let attr_iter = m
                        .attributes
                        .iter()
                        .chain(m.elements.iter().filter_map(|el| {
                            if let MeasureElement::Attributes(a) = el {
                                Some(a)
                            } else {
                                None
                            }
                        }));
                    for attr in attr_iter {
                        if let Some(time) = &attr.time {
                            current_beats = time
                                .beats
                                .split('+')
                                .map(|s| s.parse::<f32>().unwrap_or(0.0))
                                .sum();
                            current_beat_type = time.beat_type;
                            if last_emitted_time_sig != Some((current_beats, current_beat_type)) {
                                last_emitted_time_sig = Some((current_beats, current_beat_type));
                                // MIDI encodes the denominator as a power of 2
                                // (den_pow such that denominator = 1 << den_pow),
                                // matching how the parser decodes it back in
                                // midi_parser/event.rs.
                                let den_pow = (current_beat_type.max(1) as u32)
                                    .trailing_zeros()
                                    .min(255) as u8;
                                meta_events.push(RawTrackEvent {
                                    tick: global_tick_of_measure_start,
                                    kind: midly::TrackEventKind::Meta(
                                        MetaMessage::TimeSignature(
                                            current_beats.round().clamp(1.0, 255.0) as u8,
                                            den_pow,
                                            24,
                                            8,
                                        ),
                                    ),
                                });
                            }
                        }
                        if let Some(key) = &attr.key {
                            current_key_fifths = key.fifths;
                        }
                        if attr.transpose.is_some() {
                            current_transpose_states[p_idx] = attr.transpose.clone();
                        }
                    }
                }
            }

            let theoretical_duration_ticks =
                (current_beats * (4.0 / current_beat_type as f32) * midi_tpq as f32).round() as u32;
            let mut actual_measure_max_ticks = 0u32;

            for (p_idx, part) in score.parts.iter().enumerate() {
                let mut tremolo_stop_skip = false;
                if let Some(measure) = part.measures.get(m_idx) {
                    // 2. Update Part-specific Divisions and Key State
                    let attr_iter =
                        measure
                            .attributes
                            .iter()
                            .chain(measure.elements.iter().filter_map(|el| {
                                if let MeasureElement::Attributes(a) = el {
                                    Some(a)
                                } else {
                                    None
                                }
                            }));
                    for attr in attr_iter {
                        if let Some(div) = attr.divisions {
                            current_divisions[p_idx] = div;
                        }
                        if let Some(key) = &attr.key {
                            current_key_fifths = key.fifths;
                        }
                    }

                    let tick_scale = midi_tpq as f32 / current_divisions[p_idx] as f32;
                    let ch = *channel_map.get(&part.id).unwrap_or(&0);
                    let mut p_tick = global_tick_of_measure_start;
                    let mut chord_stagger: u32 = 0;
                    let mut chord_stagger_step: u32 = 0;
                    let mut grace_stagger_tick: u32 = 0;
                    let mut onset_base_tick = p_tick;

                    for (el_idx, el) in measure.elements.iter().enumerate() {
                        match el {
                            MeasureElement::Sound(s) => {
                                if let Some(t) = s.tempo {
                                    if p_idx == 0 {
                                        let tempo_val = (60_000_000.0 / t) as u32;
                                        meta_events.push(RawTrackEvent {
                                            tick: p_tick,
                                            kind: midly::TrackEventKind::Meta(MetaMessage::Tempo(
                                                tempo_val.into(),
                                            )),
                                        });
                                    }
                                }
                            }
                            MeasureElement::Direction(dir) => {
                                for dt in &dir.types {
                                    match dt {
                                        DirectionType::Metronome(met) => {
                                            if let Some(bpm_str) = &met.bpm {
                                                if let Ok(bpm) = bpm_str.parse::<f32>() {
                                                    let mut factor = 1.0;
                                                    match met.beat_unit.as_str() {
                                                        "half" => factor = 2.0,
                                                        "whole" => factor = 4.0,
                                                        "eighth" => factor = 0.5,
                                                        "16th" => factor = 0.25,
                                                        "32nd" => factor = 0.125,
                                                        _ => {}
                                                    }
                                                    if met.beat_unit_dot > 0 {
                                                        factor *= 1.5;
                                                    }
                                                    let adjusted_bpm = bpm * factor;

                                                    if p_idx == 0 {
                                                        let tempo_val =
                                                            (60_000_000.0 / adjusted_bpm) as u32;
                                                        meta_events.push(RawTrackEvent {
                                                            tick: p_tick,
                                                            kind: midly::TrackEventKind::Meta(
                                                                MetaMessage::Tempo(
                                                                    tempo_val.into(),
                                                                ),
                                                            ),
                                                        });
                                                    }
                                                }
                                            }
                                        }
                                        DirectionType::Dynamics(dyns) => {
                                            if let Some(d) = dyns.first() {
                                                part_velocities[p_idx] =
                                                    Self::dynamic_to_velocity(d);
                                            }
                                        }
                                        DirectionType::Words(text) => {
                                            let text = text.to_lowercase();
                                            let is_pizz = text.starts_with("pizz")
                                                || text.contains(" pizz")
                                                || text.contains("\tpizz");
                                            if is_pizz {
                                                part_raw_events[p_idx].push(RawTrackEvent {
                                                    tick: p_tick,
                                                    kind: midly::TrackEventKind::Midi {
                                                        channel: ch.into(),
                                                        message: MidlyMidiMessage::ProgramChange {
                                                            program: 45.into(),
                                                        },
                                                    },
                                                });
                                            } else if text.contains("arco") {
                                                let prog =
                                                    *original_programs.get(&part.id).unwrap_or(&0);
                                                part_raw_events[p_idx].push(RawTrackEvent {
                                                    tick: p_tick,
                                                    kind: midly::TrackEventKind::Midi {
                                                        channel: ch.into(),
                                                        message: MidlyMidiMessage::ProgramChange {
                                                            program: prog.into(),
                                                        },
                                                    },
                                                });
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            MeasureElement::Note(note) => {
                                if !note.is_chord {
                                    onset_base_tick = p_tick + grace_stagger_tick;
                                    if note.grace.is_some() {
                                        grace_stagger_tick += midi_tpq / 8; // 32nd note
                                    }
                                    chord_stagger = 0;
                                    chord_stagger_step = 0;
                                    tremolo_stop_skip = false;

                                    // Look ahead to count chord size and check for arpeggio
                                    let mut chord_size = 1;
                                    let mut has_arpeggio = note
                                        .notations
                                        .iter()
                                        .any(|n| matches!(n, Notation::Arpeggiate { .. }));
                                    let mut next = el_idx + 1;
                                    while next < measure.elements.len() {
                                        if let MeasureElement::Note(n) = &measure.elements[next] {
                                            if n.is_chord {
                                                chord_size += 1;
                                                if n.notations.iter().any(|not| {
                                                    matches!(not, Notation::Arpeggiate { .. })
                                                }) {
                                                    has_arpeggio = true;
                                                }
                                                next += 1;
                                                continue;
                                            }
                                        }
                                        break;
                                    }

                                    if has_arpeggio && chord_size > 1 {
                                        let duration_ticks =
                                            (note.duration as f32 * tick_scale) as u32;
                                        // Standard rhythmic units in ticks (at 480 tpq)
                                        // 480 ticks/quarter.
                                        let unit_ticks = [60, 40, 30, 20, 15, 10, 7]; // 8th, 12th, 16th, 24th, 32nd, 48th, 64th
                                        let mut stagger_tick = 7;
                                        for &u in &unit_ticks {
                                            if (chord_size - 1) as u32 * u <= duration_ticks / 2 {
                                                stagger_tick = u;
                                                break;
                                            }
                                        }
                                        chord_stagger_step = stagger_tick;
                                    }

                                    if note.notations.iter().any(|not| matches!(not, Notation::Ornaments(o) if o.iter().any(|orn| matches!(orn, Ornament::Tremolo { tremolo_type: t, .. } if t == "stop")))) {
                                        tremolo_stop_skip = true;
                                    }

                                    // Handle <note pizzicato="yes"> attribute and
                                    // <technical><pizzicato/> element: emit program change at
                                    // the transition point only (not on every chord note).
                                    if let Some(&ch) = channel_map.get(&part.id) {
                                        let note_is_pizz = note.notations.iter().any(|not| {
                                            if let Notation::Technical(marks) = not {
                                                marks.iter().any(|m| {
                                                    matches!(m, TechnicalMark::Pizzicato { .. })
                                                })
                                            } else {
                                                false
                                            }
                                        });
                                        if note_is_pizz && !part_pizzicato_state[p_idx] {
                                            part_pizzicato_state[p_idx] = true;
                                            part_raw_events[p_idx].push(RawTrackEvent {
                                                tick: p_tick,
                                                kind: midly::TrackEventKind::Midi {
                                                    channel: ch.into(),
                                                    message: MidlyMidiMessage::ProgramChange {
                                                        program: 45.into(),
                                                    },
                                                },
                                            });
                                        } else if !note_is_pizz && part_pizzicato_state[p_idx] {
                                            part_pizzicato_state[p_idx] = false;
                                            let prog =
                                                *original_programs.get(&part.id).unwrap_or(&0);
                                            part_raw_events[p_idx].push(RawTrackEvent {
                                                tick: p_tick,
                                                kind: midly::TrackEventKind::Midi {
                                                    channel: ch.into(),
                                                    message: MidlyMidiMessage::ProgramChange {
                                                        program: prog.into(),
                                                    },
                                                },
                                            });
                                        }
                                    }
                                }

                                let grace_unit = midi_tpq / 8; // 32nd note
                                let duration = if note.grace.is_some() {
                                    grace_unit
                                } else {
                                    ((note.duration as f32 * tick_scale) as u32
                                        - grace_stagger_tick)
                                        .max(1)
                                };

                                if !note.rest && !tremolo_stop_skip {
                                    let key = if let Some(pitch) = &note.pitch {
                                        Some(Self::pitch_to_midi(
                                            pitch,
                                            current_transpose_states[p_idx].as_ref(),
                                        ))
                                    } else if let Some(unp) = &note.unpitched {
                                        // Prefer the MIDI pitch stored directly on the note
                                        // (set by the MIDI parser via drum_position). Fall back
                                        // to the per-instrument midi_unpitched from the part list,
                                        // then default to snare (38).
                                        let mut k = unp.midi_number;
                                        if k.is_none() {
                                            if let Some(inst_id) = &note.instrument {
                                                if let Some(midi_list) = part_midi_map.get(&part.id)
                                                {
                                                    if let Some(mi) =
                                                        midi_list.iter().find(|m| &m.id == inst_id)
                                                    {
                                                        k = mi.midi_unpitched.map(|u| u as u8);
                                                    }
                                                }
                                            }
                                        }
                                        if k.is_none() {
                                            if let Some(midi_list) = part_midi_map.get(&part.id) {
                                                k = midi_list
                                                    .iter()
                                                    .find_map(|m| m.midi_unpitched)
                                                    .map(|u| u as u8);
                                            }
                                        }
                                        k.or(Some(38))
                                    } else {
                                        None
                                    };

                                    if let Some(key) = key {
                                        let vel = part_velocities[p_idx];

                                        let onset_tick = onset_base_tick + chord_stagger;
                                        if chord_stagger_step > 0 {
                                            chord_stagger += chord_stagger_step;
                                        }

                                        let mut played = false;
                                        let mut tremolo_bars = 0;
                                        for notation in &note.notations {
                                            if let Notation::Ornaments(orn_list) = notation {
                                                for orn in orn_list {
                                                    if let Ornament::Tremolo { bars, .. } = orn {
                                                        tremolo_bars = *bars;
                                                    }
                                                }
                                            }
                                        }

                                        if tremolo_bars > 0 {
                                            let mut tremolo_type = "single".to_string();
                                            let mut second_key_opt: Option<u8> = None;
                                            let mut total_duration_ticks = duration;

                                            for notation in &note.notations {
                                                if let Notation::Ornaments(orn_list) = notation {
                                                    for orn in orn_list {
                                                        if let Ornament::Tremolo {
                                                            tremolo_type: t,
                                                            ..
                                                        } = orn
                                                        {
                                                            tremolo_type = t.clone();
                                                        }
                                                    }
                                                }
                                            }

                                            if tremolo_type == "start" {
                                                let mut next = el_idx + 1;
                                                let mut found_second_chord = false;
                                                while next < measure.elements.len() {
                                                    if let MeasureElement::Note(n) =
                                                        &measure.elements[next]
                                                    {
                                                        if !n.is_chord {
                                                            found_second_chord = true;
                                                        }
                                                        if found_second_chord {
                                                            if second_key_opt.is_none() {
                                                                if let Some(pitch) = &n.pitch {
                                                                    second_key_opt = Some(Self::pitch_to_midi(pitch, current_transpose_states[p_idx].as_ref()));
                                                                }
                                                            }
                                                            total_duration_ticks +=
                                                                (n.duration as f32 * tick_scale)
                                                                    as u32;
                                                            if n.notations.iter().any(|not| matches!(not, Notation::Ornaments(o) if o.iter().any(|orn| matches!(orn, Ornament::Tremolo { tremolo_type: t, .. } if t == "stop")))) {
                                                                break;
                                                            }
                                                        }
                                                    }
                                                    next += 1;
                                                }
                                            }

                                            if tremolo_type != "stop" {
                                                Self::generate_tremolo_ticks(
                                                    &mut part_raw_events[p_idx],
                                                    ch,
                                                    key,
                                                    onset_tick,
                                                    total_duration_ticks,
                                                    vel,
                                                    tremolo_bars,
                                                    midi_tpq,
                                                    &tremolo_type,
                                                    second_key_opt,
                                                );
                                            }
                                            played = true;
                                        } else {
                                            for notation in &note.notations {
                                                if let Notation::Ornaments(orn_list) = notation {
                                                    for orn in orn_list {
                                                        match orn {
                                                            Ornament::TrillMark => {
                                                                Self::generate_trill_ticks(
                                                                    &mut part_raw_events[p_idx],
                                                                    ch,
                                                                    key,
                                                                    onset_tick,
                                                                    duration,
                                                                    vel,
                                                                    midi_tpq,
                                                                    current_key_fifths,
                                                                );
                                                                played = true;
                                                            }
                                                            Ornament::Turn => {
                                                                Self::generate_turn_ticks(
                                                                    &mut part_raw_events[p_idx],
                                                                    ch,
                                                                    key,
                                                                    onset_tick,
                                                                    duration,
                                                                    vel,
                                                                    midi_tpq,
                                                                    current_key_fifths,
                                                                    false,
                                                                );
                                                                played = true;
                                                            }
                                                            Ornament::DelayedTurn => {
                                                                let delayed_onset =
                                                                    onset_tick + duration / 2;
                                                                Self::generate_turn_ticks(
                                                                    &mut part_raw_events[p_idx],
                                                                    ch,
                                                                    key,
                                                                    delayed_onset,
                                                                    duration / 2,
                                                                    vel,
                                                                    midi_tpq,
                                                                    current_key_fifths,
                                                                    false,
                                                                );
                                                                played = true;
                                                            }
                                                            Ornament::InvertedTurn => {
                                                                Self::generate_turn_ticks(
                                                                    &mut part_raw_events[p_idx],
                                                                    ch,
                                                                    key,
                                                                    onset_tick,
                                                                    duration,
                                                                    vel,
                                                                    midi_tpq,
                                                                    current_key_fifths,
                                                                    true,
                                                                );
                                                                played = true;
                                                            }
                                                            Ornament::DelayedInvertedTurn => {
                                                                let delayed_onset =
                                                                    onset_tick + duration / 2;
                                                                Self::generate_turn_ticks(
                                                                    &mut part_raw_events[p_idx],
                                                                    ch,
                                                                    key,
                                                                    delayed_onset,
                                                                    duration / 2,
                                                                    vel,
                                                                    midi_tpq,
                                                                    current_key_fifths,
                                                                    true,
                                                                );
                                                                played = true;
                                                            }
                                                            Ornament::Mordent { long } => {
                                                                Self::generate_mordent_ticks(
                                                                    &mut part_raw_events[p_idx],
                                                                    ch,
                                                                    key,
                                                                    onset_tick,
                                                                    duration,
                                                                    vel,
                                                                    *long,
                                                                    false,
                                                                    midi_tpq,
                                                                    current_key_fifths,
                                                                );
                                                                played = true;
                                                            }
                                                            Ornament::InvertedMordent { long } => {
                                                                Self::generate_mordent_ticks(
                                                                    &mut part_raw_events[p_idx],
                                                                    ch,
                                                                    key,
                                                                    onset_tick,
                                                                    duration,
                                                                    vel,
                                                                    *long,
                                                                    true,
                                                                    midi_tpq,
                                                                    current_key_fifths,
                                                                );
                                                                played = true;
                                                            }
                                                            _ => {}
                                                        }
                                                    }
                                                }
                                            }
                                        }

                                        if !played {
                                            let mut is_tie_start = false;
                                            let mut is_tie_stop = false;
                                            for notation in &note.notations {
                                                if let Notation::Tied { note_type } = notation {
                                                    if note_type == "start" {
                                                        is_tie_start = true;
                                                    }
                                                    if note_type == "stop" {
                                                        is_tie_stop = true;
                                                    }
                                                }
                                            }

                                            if !is_tie_stop {
                                                part_raw_events[p_idx].push(RawTrackEvent {
                                                    tick: onset_tick,
                                                    kind: midly::TrackEventKind::Midi {
                                                        channel: ch.into(),
                                                        message: MidlyMidiMessage::NoteOn {
                                                            key: key.into(),
                                                            vel: vel.into(),
                                                        },
                                                    },
                                                });
                                            }

                                            if !is_tie_start {
                                                let note_off_tick = onset_base_tick + duration;
                                                part_raw_events[p_idx].push(RawTrackEvent {
                                                    tick: note_off_tick.max(onset_tick + 1),
                                                    kind: midly::TrackEventKind::Midi {
                                                        channel: ch.into(),
                                                        message: MidlyMidiMessage::NoteOff {
                                                            key: key.into(),
                                                            vel: 0.into(),
                                                        },
                                                    },
                                                });
                                            }
                                        }
                                    }
                                }
                                if !note.is_chord {
                                    p_tick += duration;
                                    grace_stagger_tick = 0;
                                }
                            }
                            MeasureElement::Backup(d) => {
                                let b_tick = (*d as f32 * tick_scale) as u32;
                                // A full-measure Backup (b_tick ≥ theoretical) should always
                                // reset to the exact measure start regardless of voice-1 overflow.
                                if b_tick >= theoretical_duration_ticks {
                                    p_tick = global_tick_of_measure_start;
                                } else {
                                    p_tick = p_tick.saturating_sub(b_tick);
                                }
                                grace_stagger_tick = 0;
                            }
                            MeasureElement::Forward(d) => {
                                p_tick += (*d as f32 * tick_scale) as u32;
                                grace_stagger_tick = 0;
                            }
                            _ => {}
                        }
                    }
                    let measure_consumed_ticks = p_tick - global_tick_of_measure_start;
                    if measure_consumed_ticks > actual_measure_max_ticks {
                        actual_measure_max_ticks = measure_consumed_ticks;
                    }
                }
            }

            // 3. Determine Final Measure Duration
            // If it's a pickup measure (m_idx==0 or marked implicit) and it's shorter than theoretical, use actual.
            // Otherwise, ensure it lasts at least as long as the time signature.
            let is_pickup = (m_idx == 0 || score.parts[0].measures[m_idx].implicit)
                && actual_measure_max_ticks < theoretical_duration_ticks;

            let final_measure_duration = if is_pickup {
                actual_measure_max_ticks
            } else {
                theoretical_duration_ticks
            };

            global_tick_of_measure_start += final_measure_duration;
        }

        // Convert raw events to tracks with delta times.
        // Brace-group parts (grand staff) are merged into a single track.
        smf.tracks.push(Self::finalize_track(meta_events));

        let mut added: HashSet<usize> = HashSet::new();
        for (i, part) in score.parts.iter().enumerate() {
            if added.contains(&i) {
                continue;
            }
            added.insert(i);

            let mut raw = std::mem::take(&mut part_raw_events[i]);

            if let Some(&gnum) = brace_group_of.get(&part.id) {
                // Absorb all sibling parts in the same brace group
                for (j, other) in score.parts.iter().enumerate() {
                    if j != i && !added.contains(&j) && brace_group_of.get(&other.id) == Some(&gnum)
                    {
                        raw.extend(std::mem::take(&mut part_raw_events[j]));
                        added.insert(j);
                    }
                }
            }

            smf.tracks.push(Self::finalize_track(raw));
        }
        smf
    }

    fn finalize_track(mut raw_events: Vec<RawTrackEvent>) -> Track<'static> {
        // Sort by tick. Stable sort is important to keep relative order of simultaneous events.
        raw_events.sort_by_key(|e| e.tick);

        let mut track = Track::new();
        let mut last_tick = 0u32;
        for raw in raw_events {
            let delta = raw.tick - last_tick;
            track.push(TrackEvent {
                delta: delta.into(),
                kind: raw.kind,
            });
            last_tick = raw.tick;
        }
        track.push(TrackEvent {
            delta: 0.into(),
            kind: midly::TrackEventKind::Meta(MetaMessage::EndOfTrack),
        });
        track
    }

    fn generate_trill_ticks(
        raw_events: &mut Vec<RawTrackEvent>,
        channel: u8,
        key: u8,
        onset: u32,
        duration: u32,
        velocity: u8,
        tpq: u32,
        fifths: i32,
    ) {
        let step = tpq / 4; // 16th note
        if step == 0 {
            return;
        }
        let mut t = 0;
        let mut toggle = false;
        let upper = Self::get_diatonic_neighbor(key, fifths, true);
        while t < duration {
            let k = if toggle { upper } else { key };
            raw_events.push(RawTrackEvent {
                tick: onset + t,
                kind: midly::TrackEventKind::Midi {
                    channel: channel.into(),
                    message: MidlyMidiMessage::NoteOn {
                        key: k.into(),
                        vel: velocity.into(),
                    },
                },
            });
            raw_events.push(RawTrackEvent {
                tick: (onset + t + (step as f32 * 0.9) as u32).min(onset + duration),
                kind: midly::TrackEventKind::Midi {
                    channel: channel.into(),
                    message: MidlyMidiMessage::NoteOff {
                        key: k.into(),
                        vel: 0.into(),
                    },
                },
            });
            t += step;
            toggle = !toggle;
        }
    }

    fn generate_turn_ticks(
        raw_events: &mut Vec<RawTrackEvent>,
        channel: u8,
        key: u8,
        onset: u32,
        duration: u32,
        velocity: u8,
        tpq: u32,
        fifths: i32,
        inverted: bool,
    ) {
        let step = tpq / 8; // 32nd note
        if step == 0 {
            return;
        }
        let upper = Self::get_diatonic_neighbor(key, fifths, true);
        let lower = Self::get_diatonic_neighbor(key, fifths, false);

        let sequence = if inverted {
            vec![lower, key, upper, key]
        } else {
            vec![upper, key, lower, key]
        };

        for (i, &k) in sequence.iter().enumerate() {
            let t = i as u32 * step;
            if t >= duration {
                break;
            }
            raw_events.push(RawTrackEvent {
                tick: onset + t,
                kind: midly::TrackEventKind::Midi {
                    channel: channel.into(),
                    message: MidlyMidiMessage::NoteOn {
                        key: k.into(),
                        vel: velocity.into(),
                    },
                },
            });
            raw_events.push(RawTrackEvent {
                tick: (onset + t + (step as f32 * 0.9) as u32).min(onset + duration),
                kind: midly::TrackEventKind::Midi {
                    channel: channel.into(),
                    message: MidlyMidiMessage::NoteOff {
                        key: k.into(),
                        vel: 0.into(),
                    },
                },
            });
        }

        let last_t = sequence.len() as u32 * step;
        if last_t < duration {
            raw_events.push(RawTrackEvent {
                tick: onset + last_t,
                kind: midly::TrackEventKind::Midi {
                    channel: channel.into(),
                    message: MidlyMidiMessage::NoteOn {
                        key: key.into(),
                        vel: velocity.into(),
                    },
                },
            });
            raw_events.push(RawTrackEvent {
                tick: onset + duration,
                kind: midly::TrackEventKind::Midi {
                    channel: channel.into(),
                    message: MidlyMidiMessage::NoteOff {
                        key: key.into(),
                        vel: 0.into(),
                    },
                },
            });
        }
    }

    fn generate_mordent_ticks(
        raw_events: &mut Vec<RawTrackEvent>,
        channel: u8,
        key: u8,
        onset: u32,
        duration: u32,
        velocity: u8,
        _long: bool,
        inverted: bool,
        tpq: u32,
        fifths: i32,
    ) {
        let step = tpq / 16; // 64th note
        if step == 0 {
            return;
        }
        let aux_key = if inverted {
            Self::get_diatonic_neighbor(key, fifths, true)
        } else {
            Self::get_diatonic_neighbor(key, fifths, false)
        };
        raw_events.push(RawTrackEvent {
            tick: onset,
            kind: midly::TrackEventKind::Midi {
                channel: channel.into(),
                message: MidlyMidiMessage::NoteOn {
                    key: key.into(),
                    vel: velocity.into(),
                },
            },
        });
        raw_events.push(RawTrackEvent {
            tick: onset + (step as f32 * 0.8) as u32,
            kind: midly::TrackEventKind::Midi {
                channel: channel.into(),
                message: MidlyMidiMessage::NoteOff {
                    key: key.into(),
                    vel: 0.into(),
                },
            },
        });
        raw_events.push(RawTrackEvent {
            tick: onset + step,
            kind: midly::TrackEventKind::Midi {
                channel: channel.into(),
                message: MidlyMidiMessage::NoteOn {
                    key: aux_key.into(),
                    vel: velocity.into(),
                },
            },
        });
        raw_events.push(RawTrackEvent {
            tick: onset + (step as f32 * 1.8) as u32,
            kind: midly::TrackEventKind::Midi {
                channel: channel.into(),
                message: MidlyMidiMessage::NoteOff {
                    key: aux_key.into(),
                    vel: 0.into(),
                },
            },
        });
        raw_events.push(RawTrackEvent {
            tick: onset + step * 2,
            kind: midly::TrackEventKind::Midi {
                channel: channel.into(),
                message: MidlyMidiMessage::NoteOn {
                    key: key.into(),
                    vel: velocity.into(),
                },
            },
        });
        raw_events.push(RawTrackEvent {
            tick: onset + duration,
            kind: midly::TrackEventKind::Midi {
                channel: channel.into(),
                message: MidlyMidiMessage::NoteOff {
                    key: key.into(),
                    vel: 0.into(),
                },
            },
        });
    }

    fn generate_tremolo_ticks(
        raw_events: &mut Vec<RawTrackEvent>,
        channel: u8,
        key: u8,
        onset: u32,
        duration: u32,
        velocity: u8,
        bars: i32,
        tpq: u32,
        tremolo_type: &str,
        second_key: Option<u8>,
    ) {
        let divisor = match bars {
            1 => 2, // 8th
            2 => 4, // 16th
            3 => 8, // 32nd
            _ => 4,
        };
        let step = tpq / divisor;
        if step == 0 {
            return;
        }
        let mut t = 0;
        let mut toggle = false;
        while t < duration {
            let current_key = if tremolo_type == "start" && second_key.is_some() && toggle {
                second_key.unwrap()
            } else {
                key
            };
            raw_events.push(RawTrackEvent {
                tick: onset + t,
                kind: midly::TrackEventKind::Midi {
                    channel: channel.into(),
                    message: MidlyMidiMessage::NoteOn {
                        key: current_key.into(),
                        vel: velocity.into(),
                    },
                },
            });
            raw_events.push(RawTrackEvent {
                tick: (onset + t + (step as f32 * 0.9) as u32).min(onset + duration),
                kind: midly::TrackEventKind::Midi {
                    channel: channel.into(),
                    message: MidlyMidiMessage::NoteOff {
                        key: current_key.into(),
                        vel: 0.into(),
                    },
                },
            });
            t += step;
            toggle = !toggle;
        }
    }

    pub fn get_program_number(
        sound: Option<&str>,
        midi: Option<&MidiInstrument>,
        name: Option<&str>,
        instrument_names: &[String],
    ) -> i32 {
        let mut p = -1;

        // 1. Try mapping from instrument-sound ID (Highest Priority)
        if let Some(s) = sound {
            p = get_program_from_sound(s);
        }

        // 2. Try midi-program from XML
        if p < 0 {
            if let Some(mi) = midi {
                if let Some(prog) = mi.program {
                    return (prog - 1).clamp(0, 127);
                }
            }
        }

        // 3. Try mapping from instrument-names (score-instrument)
        if p < 0 {
            for in_name in instrument_names {
                p = get_program_from_name(in_name);
                if p >= 0 {
                    break;
                }
            }
        }

        // 4. Try mapping from part-name
        if p < 0 {
            if let Some(n) = name {
                p = get_program_from_name(n);
            }
        }

        if p == 300 {
            return 14;
        } // Map wind chimes to Tubular Bells
        if p < 0 {
            return 0;
        } // Default to Piano

        p
    }

    fn get_scale_notes(fifths: i32) -> Vec<u8> {
        let tonic = (fifths * 7).rem_euclid(12);
        vec![0, 2, 4, 5, 7, 9, 11]
            .into_iter()
            .map(|d| ((tonic + d) % 12) as u8)
            .collect()
    }

    fn get_diatonic_neighbor(key: u8, fifths: i32, up: bool) -> u8 {
        let scale = Self::get_scale_notes(fifths);
        if up {
            for m in (key + 1)..(key + 3) {
                if scale.contains(&(m % 12)) {
                    return m;
                }
            }
            key + 1
        } else {
            for m in (key.saturating_sub(2)..key).rev() {
                if scale.contains(&(m % 12)) {
                    return m;
                }
            }
            key.saturating_sub(1)
        }
    }

    fn pitch_to_midi(pitch: &Pitch, transpose: Option<&crate::models::Transpose>) -> u8 {
        let step_val = match pitch.step.as_str() {
            "C" => 0,
            "D" => 2,
            "E" => 4,
            "F" => 5,
            "G" => 7,
            "A" => 9,
            "B" => 11,
            _ => 0,
        };
        let alter = pitch.alter.unwrap_or(0.0) as i32;
        let octave = pitch.octave + 1;

        let chromatic_shift = transpose.and_then(|t| t.chromatic).unwrap_or(0);
        let octave_shift = if chromatic_shift == 0 {
            transpose.and_then(|t| t.octave_change).unwrap_or(0) * 12
        } else {
            0
        };

        (octave * 12 + step_val + alter + chromatic_shift + octave_shift).clamp(0, 127) as u8
    }

    // Raised and compressed vs. a naive linear split of the 0-127 range: many
    // SF2 patches use velocity-layered samples where low velocities (below
    // ~40) trigger near-silent layers, making "pp"/"p" effectively inaudible
    // during playback. Lifting the floor and narrowing the spread keeps the
    // relative loudness ordering intact while ensuring every dynamic level
    // stays clearly audible.
    fn dynamic_to_velocity(d: &str) -> u8 {
        match d {
            "ppp" => 40,
            "pp" => 55,
            "p" => 68,
            "mp" => 80,
            "mf" => 92,
            "f" => 104,
            "ff" => 116,
            "fff" => 127,
            _ => 92,
        }
    }
}
