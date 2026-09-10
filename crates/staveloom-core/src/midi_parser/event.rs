use std::collections::{BTreeSet, HashMap, VecDeque};

use midly::{Format, MetaMessage, MidiMessage, Smf, Timing, TrackEventKind};

use super::MidiParseError;

// ---------------------------------------------------------------------------
// Public types — used by later phases (quantizer, builder)
// ---------------------------------------------------------------------------

/// A fully matched note event: one NoteOn + one NoteOff pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawNote {
    /// Zero-based part index.
    /// Format 1: `track_index - 1`  (track 0 is the tempo map)
    /// Format 0: position of this channel in the sorted set of used channels
    pub part_idx: usize,
    pub channel: u8,
    pub pitch: u8,    // 0–127
    pub velocity: u8, // NoteOn velocity (the start velocity)
    pub start_tick: u64,
    pub end_tick: u64,
}

/// A tempo change: μs per quarter-note beat.
#[derive(Debug, Clone)]
pub struct TempoChange {
    pub tick: u64,
    pub us_per_beat: u32, // 500 000 = 120 BPM
}

/// A time-signature change.
#[derive(Debug, Clone)]
pub struct TimeSigChange {
    pub tick: u64,
    pub numerator: u8,
    pub denominator: u8, // actual denominator (4, 8, 16 …), not the MIDI power-of-2
}

/// A key-signature change.
#[derive(Debug, Clone)]
pub struct KeySigChange {
    pub tick: u64,
    pub fifths: i8, // –7 (7 flats) … +7 (7 sharps)
    pub minor: bool,
}

/// Metadata for one part (one track in Format 1 / one channel group in Format 0).
#[derive(Debug, Clone, Default)]
pub struct PartInfo {
    pub name: Option<String>,
    pub program: u8, // last observed ProgramChange (default 0 = Acoustic Grand Piano)
    pub is_drum: bool, // true when channel 9 is present in this part
}

/// Everything extracted from a raw MIDI binary.
#[derive(Debug)]
pub struct ParsedMidi {
    pub tpq: u32,
    pub format: u16,
    pub notes: Vec<RawNote>, // sorted: (part_idx, start_tick, pitch)
    pub tempo_changes: Vec<TempoChange>, // sorted by tick; [0].tick == 0 always
    pub time_sig_changes: Vec<TimeSigChange>, // sorted by tick; [0].tick == 0 always
    pub key_sig_changes: Vec<KeySigChange>, // sorted by tick; [0].tick == 0 always
    pub parts: Vec<PartInfo>,
    pub total_ticks: u64,
}

impl ParsedMidi {
    /// Active tempo (μs/beat) at `tick`.
    pub fn tempo_at(&self, tick: u64) -> u32 {
        self.tempo_changes
            .iter()
            .rev()
            .find(|c| c.tick <= tick)
            .map_or(500_000, |c| c.us_per_beat)
    }

    /// Active time signature `(numerator, denominator)` at `tick`.
    pub fn time_sig_at(&self, tick: u64) -> (u8, u8) {
        self.time_sig_changes
            .iter()
            .rev()
            .find(|c| c.tick <= tick)
            .map_or((4, 4), |c| (c.numerator, c.denominator))
    }

    /// Active key-signature (fifths) at `tick`.
    pub fn key_sig_at(&self, tick: u64) -> i8 {
        self.key_sig_changes
            .iter()
            .rev()
            .find(|c| c.tick <= tick)
            .map_or(0, |c| c.fifths)
    }

    /// Convert an absolute tick position to wall-clock seconds.
    ///
    /// Accounts for every tempo change up to `tick`.
    pub fn tick_to_seconds(&self, tick: u64) -> f64 {
        let mut seconds = 0.0_f64;
        let mut prev_tick = 0u64;
        let mut prev_us = 500_000u32;

        for change in &self.tempo_changes {
            if change.tick >= tick {
                break;
            }
            let elapsed = change.tick - prev_tick;
            seconds += elapsed as f64 * prev_us as f64 / (self.tpq as f64 * 1_000_000.0);
            prev_tick = change.tick;
            prev_us = change.us_per_beat;
        }

        let elapsed = tick - prev_tick;
        seconds += elapsed as f64 * prev_us as f64 / (self.tpq as f64 * 1_000_000.0);
        seconds
    }

    /// Measure-start tick boundaries for the entire file.
    ///
    /// The returned list always starts with `0`.  The last entry equals or
    /// exceeds `total_ticks` so that every note has a containing measure.
    ///
    /// ```
    /// // 4/4 at TPQ=480, total_ticks=1920 → [0, 1920]
    /// // 3/4 at TPQ=480, total_ticks=1440 → [0, 1440]
    /// ```
    pub fn measure_boundaries(&self) -> Vec<u64> {
        let mut boundaries = Vec::new();
        let mut tick = 0u64;
        loop {
            boundaries.push(tick);
            if tick >= self.total_ticks {
                break;
            }
            let (num, den) = self.time_sig_at(tick);
            // ticks_per_measure = numerator * (4 / denominator) * tpq
            let measure_len = num as u64 * self.tpq as u64 * 4 / den as u64;
            if measure_len == 0 {
                break; // safety: avoid infinite loop on degenerate time sig
            }
            tick += measure_len;
        }
        boundaries
    }
}

// ---------------------------------------------------------------------------
// Parser implementation
// ---------------------------------------------------------------------------

pub(super) struct SmfParser;

impl SmfParser {
    pub(super) fn parse(bytes: &[u8]) -> Result<ParsedMidi, MidiParseError> {
        if bytes.len() < 8 {
            return Err(MidiParseError::EmptyFile);
        }

        let smf = Smf::parse(bytes).map_err(|e| MidiParseError::MidlyError(e.to_string()))?;

        let tpq = match smf.header.timing {
            Timing::Metrical(tpq) => {
                let v = tpq.as_int() as u32;
                if v == 0 {
                    return Err(MidiParseError::InvalidFormat(
                        "TPQ (ticks-per-quarter-note) cannot be zero".into(),
                    ));
                }
                v
            }
            Timing::Timecode(_, _) => {
                return Err(MidiParseError::InvalidFormat(
                    "SMPTE timecode timing is not supported; use metrical (PPQ) timing".into(),
                ));
            }
        };

        match smf.header.format {
            Format::SingleTrack => Self::parse_format0(&smf, tpq),
            Format::Parallel => Self::parse_format1(&smf, tpq),
            Format::Sequential => Err(MidiParseError::UnsupportedFormat(2)),
        }
    }

    // -----------------------------------------------------------------------
    // Format 0: single track, channels multiplexed
    // -----------------------------------------------------------------------

    fn parse_format0(smf: &Smf<'_>, tpq: u32) -> Result<ParsedMidi, MidiParseError> {
        if smf.tracks.is_empty() {
            return Err(MidiParseError::EmptyFile);
        }

        let mut tempo_changes = Vec::<TempoChange>::new();
        let mut time_sig_changes = Vec::<TimeSigChange>::new();
        let mut key_sig_changes = Vec::<KeySigChange>::new();

        // channel → last seen program number
        let mut channel_programs: HashMap<u8, u8> = HashMap::new();
        // NoteOn stack: (channel, pitch) → queue of (start_tick, velocity)
        let mut active: HashMap<(u8, u8), VecDeque<(u64, u8)>> = HashMap::new();
        let mut notes: Vec<RawNote> = Vec::new();
        let mut current_tick = 0u64;
        let mut total_ticks = 0u64;
        // Track name / text for this single-track file
        let mut piece_name: Option<String> = None;

        for event in smf.tracks[0].iter() {
            current_tick += event.delta.as_int() as u64;
            total_ticks = total_ticks.max(current_tick);

            match &event.kind {
                TrackEventKind::Midi { channel, message } => {
                    let ch = channel.as_int();
                    match message {
                        MidiMessage::NoteOn { key, vel } => {
                            let pitch = key.as_int();
                            let vel = vel.as_int();
                            if vel == 0 {
                                // NoteOn vel=0 is a NoteOff (MIDI running-status convention)
                                Self::pop_note(
                                    &mut active,
                                    &mut notes,
                                    ch,
                                    pitch,
                                    ch as usize, // temporary part_idx, remapped later
                                    current_tick,
                                );
                            } else {
                                active
                                    .entry((ch, pitch))
                                    .or_default()
                                    .push_back((current_tick, vel));
                            }
                        }
                        MidiMessage::NoteOff { key, .. } => {
                            Self::pop_note(
                                &mut active,
                                &mut notes,
                                ch,
                                key.as_int(),
                                ch as usize,
                                current_tick,
                            );
                        }
                        MidiMessage::ProgramChange { program } => {
                            channel_programs.insert(ch, program.as_int());
                        }
                        _ => {}
                    }
                }
                TrackEventKind::Meta(meta) => {
                    Self::collect_meta(
                        meta,
                        current_tick,
                        &mut tempo_changes,
                        &mut time_sig_changes,
                        &mut key_sig_changes,
                    );
                    match meta {
                        MetaMessage::TrackName(n) | MetaMessage::Text(n) => {
                            if piece_name.is_none() {
                                if let Ok(s) = std::str::from_utf8(n) {
                                    if !s.trim().is_empty() {
                                        piece_name = Some(s.trim().to_string());
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }

        // Drain any still-open NoteOn events (shouldn't happen in well-formed files)
        Self::drain_active(&mut active, &mut notes, total_ticks);

        // Build channel → contiguous part_idx mapping
        let used_channels: Vec<u8> = notes
            .iter()
            .map(|n| n.channel)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();

        if used_channels.is_empty() {
            return Err(MidiParseError::EmptyFile);
        }

        // Remap temporary part_idx (= channel number) → contiguous index
        for note in &mut notes {
            note.part_idx = used_channels
                .iter()
                .position(|&ch| ch == note.channel)
                .unwrap_or(0);
        }
        notes.sort_by_key(|n| (n.part_idx, n.start_tick, n.pitch));

        let parts: Vec<PartInfo> = used_channels
            .iter()
            .map(|&ch| PartInfo {
                name: piece_name
                    .clone()
                    .or_else(|| Some(format!("Ch {}", ch + 1))),
                program: *channel_programs.get(&ch).unwrap_or(&0),
                is_drum: ch == 9,
            })
            .collect();

        Self::ensure_defaults(
            &mut tempo_changes,
            &mut time_sig_changes,
            &mut key_sig_changes,
        );

        Ok(ParsedMidi {
            tpq,
            format: 0,
            notes,
            tempo_changes,
            time_sig_changes,
            key_sig_changes,
            parts,
            total_ticks,
        })
    }

    // -----------------------------------------------------------------------
    // Format 1: track 0 = tempo map, tracks 1+ = instruments
    // -----------------------------------------------------------------------

    fn parse_format1(smf: &Smf<'_>, tpq: u32) -> Result<ParsedMidi, MidiParseError> {
        if smf.tracks.is_empty() {
            return Err(MidiParseError::EmptyFile);
        }

        let mut tempo_changes = Vec::<TempoChange>::new();
        let mut time_sig_changes = Vec::<TimeSigChange>::new();
        let mut key_sig_changes = Vec::<KeySigChange>::new();
        let mut all_notes: Vec<RawNote> = Vec::new();
        let mut parts: Vec<PartInfo> = Vec::new();
        let mut total_ticks = 0u64;

        for (track_idx, track) in smf.tracks.iter().enumerate() {
            let is_tempo_track = track_idx == 0;
            let part_idx = track_idx.saturating_sub(1);

            let mut current_tick = 0u64;
            let mut track_name: Option<String> = None;
            let mut track_program: u8 = 0;
            let mut track_is_drum = false;

            // NoteOn stack for this track: (channel, pitch) → queue of (start_tick, velocity)
            let mut active: HashMap<(u8, u8), VecDeque<(u64, u8)>> = HashMap::new();
            let mut track_notes: Vec<RawNote> = Vec::new();

            for event in track.iter() {
                current_tick += event.delta.as_int() as u64;
                total_ticks = total_ticks.max(current_tick);

                match &event.kind {
                    TrackEventKind::Midi { channel, message } => {
                        if is_tempo_track {
                            // Track 0 should only carry meta events; skip MIDI events
                            continue;
                        }
                        let ch = channel.as_int();
                        if ch == 9 {
                            track_is_drum = true;
                        }

                        match message {
                            MidiMessage::NoteOn { key, vel } => {
                                let pitch = key.as_int();
                                let vel = vel.as_int();
                                if vel == 0 {
                                    Self::pop_note(
                                        &mut active,
                                        &mut track_notes,
                                        ch,
                                        pitch,
                                        part_idx,
                                        current_tick,
                                    );
                                } else {
                                    active
                                        .entry((ch, pitch))
                                        .or_default()
                                        .push_back((current_tick, vel));
                                }
                            }
                            MidiMessage::NoteOff { key, .. } => {
                                Self::pop_note(
                                    &mut active,
                                    &mut track_notes,
                                    ch,
                                    key.as_int(),
                                    part_idx,
                                    current_tick,
                                );
                            }
                            MidiMessage::ProgramChange { program } => {
                                track_program = program.as_int();
                                if channel.as_int() == 9 {
                                    track_is_drum = true;
                                }
                            }
                            _ => {}
                        }
                    }
                    TrackEventKind::Meta(meta) => {
                        // Tempo/timesig/keysig from any track (some files put them in track 1+)
                        Self::collect_meta(
                            meta,
                            current_tick,
                            &mut tempo_changes,
                            &mut time_sig_changes,
                            &mut key_sig_changes,
                        );
                        // Track name: prefer TrackName, fall back to InstrumentName
                        match meta {
                            MetaMessage::TrackName(n) => {
                                if let Ok(s) = std::str::from_utf8(n) {
                                    if !s.trim().is_empty() {
                                        track_name = Some(s.trim().to_string());
                                    }
                                }
                            }
                            MetaMessage::InstrumentName(n) => {
                                if track_name.is_none() {
                                    if let Ok(s) = std::str::from_utf8(n) {
                                        if !s.trim().is_empty() {
                                            track_name = Some(s.trim().to_string());
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }

            // Close any still-open NoteOn events (buggy MIDI files)
            for ((ch, pitch), mut stack) in active.drain() {
                while let Some((start, vel)) = stack.pop_front() {
                    track_notes.push(RawNote {
                        part_idx,
                        channel: ch,
                        pitch,
                        velocity: vel,
                        start_tick: start,
                        end_tick: current_tick,
                    });
                }
            }

            if !is_tempo_track {
                track_notes.sort_by_key(|n| (n.start_tick, n.pitch));
                all_notes.extend(track_notes);
                parts.push(PartInfo {
                    name: track_name,
                    program: track_program,
                    is_drum: track_is_drum,
                });
            }
        }

        if parts.is_empty() {
            return Err(MidiParseError::EmptyFile);
        }

        Self::ensure_defaults(
            &mut tempo_changes,
            &mut time_sig_changes,
            &mut key_sig_changes,
        );
        all_notes.sort_by_key(|n| (n.part_idx, n.start_tick, n.pitch));

        Ok(ParsedMidi {
            tpq,
            format: 1,
            notes: all_notes,
            tempo_changes,
            time_sig_changes,
            key_sig_changes,
            parts,
            total_ticks,
        })
    }

    // -----------------------------------------------------------------------
    // Shared helpers
    // -----------------------------------------------------------------------

    /// Extract tempo, time-signature, and key-signature meta events.
    fn collect_meta(
        meta: &MetaMessage<'_>,
        tick: u64,
        tempo_changes: &mut Vec<TempoChange>,
        time_sig_changes: &mut Vec<TimeSigChange>,
        key_sig_changes: &mut Vec<KeySigChange>,
    ) {
        match meta {
            MetaMessage::Tempo(us) => {
                tempo_changes.push(TempoChange {
                    tick,
                    us_per_beat: us.as_int(),
                });
            }
            MetaMessage::TimeSignature(num, den_pow, _, _) => {
                // MIDI encodes denominator as a negative power of 2:
                //   den_pow=2 → denominator=4 (quarter note)
                //   den_pow=3 → denominator=8 (eighth note)
                let denominator = 1u8.checked_shl(*den_pow as u32).unwrap_or(4);
                time_sig_changes.push(TimeSigChange {
                    tick,
                    numerator: *num,
                    denominator,
                });
            }
            MetaMessage::KeySignature(fifths, minor) => {
                key_sig_changes.push(KeySigChange {
                    tick,
                    fifths: *fifths,
                    minor: *minor,
                });
            }
            _ => {}
        }
    }

    /// Match a NoteOff (or NoteOn vel=0) to its pending NoteOn and emit a `RawNote`.
    ///
    /// Uses FIFO order so nested same-pitch events (tremolos) resolve correctly.
    fn pop_note(
        active: &mut HashMap<(u8, u8), VecDeque<(u64, u8)>>,
        notes: &mut Vec<RawNote>,
        ch: u8,
        pitch: u8,
        part_idx: usize,
        end_tick: u64,
    ) {
        if let Some(stack) = active.get_mut(&(ch, pitch)) {
            if let Some((start, vel)) = stack.pop_front() {
                notes.push(RawNote {
                    part_idx,
                    channel: ch,
                    pitch,
                    velocity: vel,
                    start_tick: start,
                    end_tick,
                });
            }
        }
    }

    /// Close all still-open NoteOn events (orphaned NoteOns), extending them to `end_tick`.
    fn drain_active(
        active: &mut HashMap<(u8, u8), VecDeque<(u64, u8)>>,
        notes: &mut Vec<RawNote>,
        end_tick: u64,
    ) {
        for ((ch, pitch), mut stack) in active.drain() {
            while let Some((start, vel)) = stack.pop_front() {
                notes.push(RawNote {
                    part_idx: ch as usize, // temporary; remapped by caller
                    channel: ch,
                    pitch,
                    velocity: vel,
                    start_tick: start,
                    end_tick,
                });
            }
        }
    }

    /// Ensure there is always an entry at tick=0 for each map type.
    ///
    /// MIDI files that omit the initial tempo/time-sig/key-sig use the standard
    /// defaults (120 BPM, 4/4, C major).
    fn ensure_defaults(
        tempo_changes: &mut Vec<TempoChange>,
        time_sig_changes: &mut Vec<TimeSigChange>,
        key_sig_changes: &mut Vec<KeySigChange>,
    ) {
        tempo_changes.sort_by_key(|c| c.tick);
        time_sig_changes.sort_by_key(|c| c.tick);
        key_sig_changes.sort_by_key(|c| c.tick);

        if tempo_changes.is_empty() || tempo_changes[0].tick > 0 {
            tempo_changes.insert(
                0,
                TempoChange {
                    tick: 0,
                    us_per_beat: 500_000,
                },
            );
        }
        if time_sig_changes.is_empty() || time_sig_changes[0].tick > 0 {
            time_sig_changes.insert(
                0,
                TimeSigChange {
                    tick: 0,
                    numerator: 4,
                    denominator: 4,
                },
            );
        }
        if key_sig_changes.is_empty() || key_sig_changes[0].tick > 0 {
            key_sig_changes.insert(
                0,
                KeySigChange {
                    tick: 0,
                    fifths: 0,
                    minor: false,
                },
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    // ------------------------------------------------------------------
    // MIDI byte-building helpers
    // ------------------------------------------------------------------

    pub(crate) fn var_len(mut n: u32) -> Vec<u8> {
        let mut buf = vec![(n & 0x7F) as u8];
        n >>= 7;
        while n > 0 {
            buf.insert(0, ((n & 0x7F) | 0x80) as u8);
            n >>= 7;
        }
        buf
    }

    pub(crate) fn midi_header(format: u16, num_tracks: u16, tpq: u16) -> Vec<u8> {
        let mut h = b"MThd".to_vec();
        h.extend_from_slice(&6u32.to_be_bytes());
        h.extend_from_slice(&format.to_be_bytes());
        h.extend_from_slice(&num_tracks.to_be_bytes());
        h.extend_from_slice(&tpq.to_be_bytes());
        h
    }

    pub(crate) fn midi_track(events: Vec<u8>) -> Vec<u8> {
        let mut t = b"MTrk".to_vec();
        t.extend_from_slice(&(events.len() as u32).to_be_bytes());
        t.extend(events);
        t
    }

    pub(crate) fn ev_note_on(delta: u32, ch: u8, pitch: u8, vel: u8) -> Vec<u8> {
        let mut e = var_len(delta);
        e.extend_from_slice(&[0x90 | (ch & 0x0F), pitch, vel]);
        e
    }

    pub(crate) fn ev_note_off(delta: u32, ch: u8, pitch: u8) -> Vec<u8> {
        let mut e = var_len(delta);
        e.extend_from_slice(&[0x80 | (ch & 0x0F), pitch, 0x00]);
        e
    }

    pub(crate) fn ev_tempo(delta: u32, us: u32) -> Vec<u8> {
        let mut e = var_len(delta);
        e.extend_from_slice(&[
            0xFF,
            0x51,
            0x03,
            ((us >> 16) & 0xFF) as u8,
            ((us >> 8) & 0xFF) as u8,
            (us & 0xFF) as u8,
        ]);
        e
    }

    pub(crate) fn ev_time_sig(delta: u32, num: u8, den_pow: u8) -> Vec<u8> {
        let mut e = var_len(delta);
        e.extend_from_slice(&[0xFF, 0x58, 0x04, num, den_pow, 0x18, 0x08]);
        e
    }

    pub(crate) fn ev_key_sig(delta: u32, fifths: i8, minor: bool) -> Vec<u8> {
        let mut e = var_len(delta);
        e.extend_from_slice(&[0xFF, 0x59, 0x02, fifths as u8, minor as u8]);
        e
    }

    pub(crate) fn ev_prog_change(delta: u32, ch: u8, program: u8) -> Vec<u8> {
        let mut e = var_len(delta);
        e.extend_from_slice(&[0xC0 | (ch & 0x0F), program]);
        e
    }

    pub(crate) fn ev_track_name(delta: u32, name: &str) -> Vec<u8> {
        let bytes = name.as_bytes();
        let mut e = var_len(delta);
        e.extend_from_slice(&[0xFF, 0x03]);
        e.extend(var_len(bytes.len() as u32));
        e.extend_from_slice(bytes);
        e
    }

    pub(crate) fn ev_eot(delta: u32) -> Vec<u8> {
        let mut e = var_len(delta);
        e.extend_from_slice(&[0xFF, 0x2F, 0x00]);
        e
    }

    /// Build a Format 1 MIDI: tempo track + one note track with 4 quarter notes.
    ///
    /// Notes: C4(60), D4(62), E4(64), F4(65)  at `tpq` ticks each.
    pub(crate) fn make_f1_4notes(tpq: u16) -> Vec<u8> {
        let t = tpq as u32;

        let mut track0 = Vec::new();
        track0.extend(ev_tempo(0, 500_000)); // 120 BPM
        track0.extend(ev_time_sig(0, 4, 2)); // 4/4  (den_pow=2 → denominator=4)
        track0.extend(ev_key_sig(0, 0, false)); // C major
        track0.extend(ev_eot(0));

        let mut track1 = Vec::new();
        track1.extend(ev_track_name(0, "Piano"));
        track1.extend(ev_prog_change(0, 0, 0)); // Acoustic Grand Piano
        track1.extend(ev_note_on(0, 0, 60, 64));
        track1.extend(ev_note_off(t, 0, 60));
        track1.extend(ev_note_on(0, 0, 62, 64));
        track1.extend(ev_note_off(t, 0, 62));
        track1.extend(ev_note_on(0, 0, 64, 64));
        track1.extend(ev_note_off(t, 0, 64));
        track1.extend(ev_note_on(0, 0, 65, 64));
        track1.extend(ev_note_off(t, 0, 65));
        track1.extend(ev_eot(0));

        let mut midi = midi_header(1, 2, tpq);
        midi.extend(midi_track(track0));
        midi.extend(midi_track(track1));
        midi
    }

    // ------------------------------------------------------------------
    // Tests
    // ------------------------------------------------------------------

    #[test]
    fn test_format1_basic() {
        let bytes = make_f1_4notes(480);
        let r = SmfParser::parse(&bytes).expect("parse must succeed");

        assert_eq!(r.tpq, 480);
        assert_eq!(r.format, 1);
        assert_eq!(r.notes.len(), 4);
        assert_eq!(r.parts.len(), 1);
    }

    #[test]
    fn test_format1_note_properties() {
        let bytes = make_f1_4notes(480);
        let r = SmfParser::parse(&bytes).unwrap();
        let n = &r.notes;

        assert_eq!(
            (n[0].pitch, n[0].velocity, n[0].start_tick, n[0].end_tick),
            (60, 64, 0, 480)
        );
        assert_eq!((n[1].pitch, n[1].start_tick, n[1].end_tick), (62, 480, 960));
        assert_eq!(
            (n[2].pitch, n[2].start_tick, n[2].end_tick),
            (64, 960, 1440)
        );
        assert_eq!(
            (n[3].pitch, n[3].start_tick, n[3].end_tick),
            (65, 1440, 1920)
        );
    }

    #[test]
    fn test_format1_part_info() {
        let bytes = make_f1_4notes(480);
        let r = SmfParser::parse(&bytes).unwrap();

        assert_eq!(r.parts[0].name, Some("Piano".into()));
        assert_eq!(r.parts[0].program, 0);
        assert!(!r.parts[0].is_drum);
    }

    #[test]
    fn test_format1_total_ticks() {
        let bytes = make_f1_4notes(480);
        let r = SmfParser::parse(&bytes).unwrap();
        // 4 quarter notes at TPQ=480 → last NoteOff at tick 1920
        assert_eq!(r.total_ticks, 1920);
    }

    #[test]
    fn test_tempo_at() {
        let bytes = make_f1_4notes(480);
        let r = SmfParser::parse(&bytes).unwrap();

        assert_eq!(r.tempo_at(0), 500_000);
        assert_eq!(r.tempo_at(9999), 500_000);
    }

    #[test]
    fn test_time_sig_at_44() {
        let bytes = make_f1_4notes(480);
        let r = SmfParser::parse(&bytes).unwrap();

        assert_eq!(r.time_sig_at(0), (4, 4));
        assert_eq!(r.time_sig_at(9999), (4, 4));
    }

    #[test]
    fn test_key_sig_at_c_major() {
        let bytes = make_f1_4notes(480);
        let r = SmfParser::parse(&bytes).unwrap();
        assert_eq!(r.key_sig_at(0), 0);
    }

    #[test]
    fn test_tick_to_seconds_120bpm() {
        let bytes = make_f1_4notes(480);
        let r = SmfParser::parse(&bytes).unwrap();

        // 120 BPM, TPQ=480 → 480 ticks = 1 beat = 0.5 s
        let s = r.tick_to_seconds(480);
        assert!((s - 0.5).abs() < 1e-9, "Expected 0.5s, got {s}");

        // 1920 ticks = 4 beats = 2.0 s
        let s = r.tick_to_seconds(1920);
        assert!((s - 2.0).abs() < 1e-9, "Expected 2.0s, got {s}");
    }

    #[test]
    fn test_measure_boundaries_44() {
        let bytes = make_f1_4notes(480);
        let r = SmfParser::parse(&bytes).unwrap();
        // 4/4, TPQ=480 → measure_len=1920, total_ticks=1920 → one measure
        assert_eq!(r.measure_boundaries(), vec![0, 1920]);
    }

    #[test]
    fn test_measure_boundaries_34() {
        // 3 quarter notes in 3/4
        let tpq = 480u16;
        let t = tpq as u32;

        let mut t0 = Vec::new();
        t0.extend(ev_tempo(0, 500_000));
        t0.extend(ev_time_sig(0, 3, 2)); // 3/4
        t0.extend(ev_eot(0));

        let mut t1 = Vec::new();
        for pitch in [60u8, 62, 64] {
            t1.extend(ev_note_on(0, 0, pitch, 64));
            t1.extend(ev_note_off(t, 0, pitch));
        }
        t1.extend(ev_eot(0));

        let mut midi = midi_header(1, 2, tpq);
        midi.extend(midi_track(t0));
        midi.extend(midi_track(t1));

        let r = SmfParser::parse(&midi).unwrap();
        // 3/4, TPQ=480 → measure_len = 3*480*4/4 = 1440 ticks
        assert_eq!(r.measure_boundaries(), vec![0, 1440]);
    }

    #[test]
    fn test_measure_boundaries_two_measures() {
        // 8 quarter notes → 2 full 4/4 measures
        let tpq = 480u16;
        let t = tpq as u32;

        let mut t0 = Vec::new();
        t0.extend(ev_time_sig(0, 4, 2));
        t0.extend(ev_eot(0));

        let mut t1 = Vec::new();
        for pitch in [60u8, 62, 64, 65, 67, 69, 71, 72] {
            t1.extend(ev_note_on(0, 0, pitch, 64));
            t1.extend(ev_note_off(t, 0, pitch));
        }
        t1.extend(ev_eot(0));

        let mut midi = midi_header(1, 2, tpq);
        midi.extend(midi_track(t0));
        midi.extend(midi_track(t1));

        let r = SmfParser::parse(&midi).unwrap();
        assert_eq!(r.measure_boundaries(), vec![0, 1920, 3840]);
    }

    #[test]
    fn test_note_on_vel_zero_is_note_off() {
        let tpq = 480u16;

        let mut t0 = Vec::new();
        t0.extend(ev_eot(0));

        // NoteOn vel=100, then NoteOn vel=0 (= NoteOff)
        let mut t1 = Vec::new();
        t1.extend(ev_note_on(0, 0, 60, 100));
        // NoteOn ch0, C4, vel=0 acts as NoteOff
        let mut fake_off = var_len(480u32);
        fake_off.extend_from_slice(&[0x90, 60, 0x00]);
        t1.extend(fake_off);
        t1.extend(ev_eot(0));

        let mut midi = midi_header(1, 2, tpq);
        midi.extend(midi_track(t0));
        midi.extend(midi_track(t1));

        let r = SmfParser::parse(&midi).unwrap();
        assert_eq!(r.notes.len(), 1);
        assert_eq!(r.notes[0].velocity, 100); // start velocity preserved
        assert_eq!(r.notes[0].end_tick, 480);
    }

    #[test]
    fn test_chord_simultaneous_notes() {
        let tpq = 480u16;
        let t = tpq as u32;

        let mut t0 = Vec::new();
        t0.extend(ev_eot(0));

        // C-major chord: C4(60), E4(64), G4(67) — all start at tick 0
        let mut t1 = Vec::new();
        t1.extend(ev_note_on(0, 0, 60, 80));
        t1.extend(ev_note_on(0, 0, 64, 80)); // delta=0
        t1.extend(ev_note_on(0, 0, 67, 80)); // delta=0
        t1.extend(ev_note_off(t, 0, 60));
        t1.extend(ev_note_off(0, 0, 64));
        t1.extend(ev_note_off(0, 0, 67));
        t1.extend(ev_eot(0));

        let mut midi = midi_header(1, 2, tpq);
        midi.extend(midi_track(t0));
        midi.extend(midi_track(t1));

        let r = SmfParser::parse(&midi).unwrap();
        assert_eq!(r.notes.len(), 3);
        assert!(r.notes.iter().all(|n| n.start_tick == 0));
        assert!(r.notes.iter().all(|n| n.end_tick == t as u64));
    }

    #[test]
    fn test_tempo_change_mid_piece() {
        let tpq = 480u16;

        let mut t0 = Vec::new();
        t0.extend(ev_tempo(0, 500_000)); // 120 BPM at tick 0
        t0.extend(ev_tempo(1920, 333_333)); // ~180 BPM at measure 2
        t0.extend(ev_eot(0));

        let mut t1 = Vec::new();
        t1.extend(ev_note_on(0, 0, 60, 64));
        t1.extend(ev_note_off(480, 0, 60));
        t1.extend(ev_eot(0));

        let mut midi = midi_header(1, 2, tpq);
        midi.extend(midi_track(t0));
        midi.extend(midi_track(t1));

        let r = SmfParser::parse(&midi).unwrap();
        assert_eq!(r.tempo_changes.len(), 2);
        assert_eq!(r.tempo_at(0), 500_000);
        assert_eq!(r.tempo_at(1919), 500_000); // still 120 BPM
        assert_eq!(r.tempo_at(1920), 333_333); // switched to ~180 BPM
    }

    #[test]
    fn test_key_sig_sharp_keys() {
        let tpq = 480u16;

        let mut t0 = Vec::new();
        t0.extend(ev_key_sig(0, 2, false)); // D major (+2 sharps)
        t0.extend(ev_eot(0));

        let mut t1 = Vec::new();
        t1.extend(ev_note_on(0, 0, 62, 64));
        t1.extend(ev_note_off(480, 0, 62));
        t1.extend(ev_eot(0));

        let mut midi = midi_header(1, 2, tpq);
        midi.extend(midi_track(t0));
        midi.extend(midi_track(t1));

        let r = SmfParser::parse(&midi).unwrap();
        assert_eq!(r.key_sig_at(0), 2);
    }

    #[test]
    fn test_key_sig_flat_keys() {
        let tpq = 480u16;

        let mut t0 = Vec::new();
        t0.extend(ev_key_sig(0, -3, false)); // Eb major (-3 flats)
        t0.extend(ev_eot(0));

        let mut t1 = Vec::new();
        t1.extend(ev_note_on(0, 0, 63, 64));
        t1.extend(ev_note_off(480, 0, 63));
        t1.extend(ev_eot(0));

        let mut midi = midi_header(1, 2, tpq);
        midi.extend(midi_track(t0));
        midi.extend(midi_track(t1));

        let r = SmfParser::parse(&midi).unwrap();
        assert_eq!(r.key_sig_at(0), -3);
    }

    #[test]
    fn test_format0_channel_separation() {
        let tpq = 480u16;

        // ch0: C4(60), ch1: E4(64) — both in single track
        let mut t0 = Vec::new();
        t0.extend(ev_note_on(0, 0, 60, 64)); // ch0
        t0.extend(ev_note_on(0, 1, 64, 64)); // ch1
        t0.extend(ev_note_off(480, 0, 60));
        t0.extend(ev_note_off(0, 1, 64));
        t0.extend(ev_eot(0));

        let mut midi = midi_header(0, 1, tpq);
        midi.extend(midi_track(t0));

        let r = SmfParser::parse(&midi).unwrap();
        assert_eq!(r.format, 0);
        assert_eq!(r.parts.len(), 2);

        let part0_notes: Vec<_> = r.notes.iter().filter(|n| n.part_idx == 0).collect();
        let part1_notes: Vec<_> = r.notes.iter().filter(|n| n.part_idx == 1).collect();
        assert_eq!(part0_notes.len(), 1);
        assert_eq!(part0_notes[0].pitch, 60); // ch0 → part 0
        assert_eq!(part1_notes.len(), 1);
        assert_eq!(part1_notes[0].pitch, 64); // ch1 → part 1
    }

    #[test]
    fn test_drum_channel_flagged() {
        let tpq = 480u16;

        let mut t0 = Vec::new();
        t0.extend(ev_eot(0));

        let mut t1 = Vec::new();
        t1.extend(ev_note_on(0, 9, 38, 80)); // ch9 = drums (snare)
        t1.extend(ev_note_off(480, 9, 38));
        t1.extend(ev_eot(0));

        let mut midi = midi_header(1, 2, tpq);
        midi.extend(midi_track(t0));
        midi.extend(midi_track(t1));

        let r = SmfParser::parse(&midi).unwrap();
        assert!(r.parts[0].is_drum);
    }

    #[test]
    fn test_program_change_captured() {
        let tpq = 480u16;

        let mut t0 = Vec::new();
        t0.extend(ev_eot(0));

        let mut t1 = Vec::new();
        t1.extend(ev_prog_change(0, 0, 40)); // program 40 = Violin
        t1.extend(ev_note_on(0, 0, 64, 64));
        t1.extend(ev_note_off(480, 0, 64));
        t1.extend(ev_eot(0));

        let mut midi = midi_header(1, 2, tpq);
        midi.extend(midi_track(t0));
        midi.extend(midi_track(t1));

        let r = SmfParser::parse(&midi).unwrap();
        assert_eq!(r.parts[0].program, 40);
    }

    #[test]
    fn test_tick_to_seconds_with_tempo_change() {
        // First 1920 ticks at 120 BPM (500000 μs/beat) = 2.0 s
        // Then tempo changes to 60 BPM (1000000 μs/beat)
        // Next 480 ticks at 60 BPM = 1.0 s
        // Total at tick 2400: 3.0 s
        let tpq = 480u16;

        let mut t0 = Vec::new();
        t0.extend(ev_tempo(0, 500_000)); // 120 BPM
        t0.extend(ev_tempo(1920, 1_000_000)); // 60 BPM at measure 2
        t0.extend(ev_eot(0));

        let mut t1 = Vec::new();
        t1.extend(ev_note_on(0, 0, 60, 64));
        t1.extend(ev_note_off(2400, 0, 60)); // note spanning the tempo change
        t1.extend(ev_eot(0));

        let mut midi = midi_header(1, 2, tpq);
        midi.extend(midi_track(t0));
        midi.extend(midi_track(t1));

        let r = SmfParser::parse(&midi).unwrap();

        // tick 1920 = end of first 4/4 bar = 2.0 s
        let s = r.tick_to_seconds(1920);
        assert!((s - 2.0).abs() < 1e-9, "Expected 2.0s, got {s}");

        // tick 2400 = 1920 + 480 ticks at 60 BPM = 2.0 + 1.0 = 3.0 s
        let s = r.tick_to_seconds(2400);
        assert!((s - 3.0).abs() < 1e-9, "Expected 3.0s, got {s}");
    }

    #[test]
    fn test_different_tpq() {
        let bytes = make_f1_4notes(960);
        let r = SmfParser::parse(&bytes).unwrap();

        assert_eq!(r.tpq, 960);
        // quarter note at TPQ=960 → 960 ticks
        assert_eq!(r.notes[0].end_tick, 960);
        assert_eq!(r.notes[3].end_tick, 3840);

        // 960 ticks at 120 BPM, TPQ=960 → 0.5 s
        let s = r.tick_to_seconds(960);
        assert!((s - 0.5).abs() < 1e-9);
    }

    #[test]
    fn test_missing_tempo_uses_default() {
        // No tempo event → default 120 BPM (500000 μs/beat)
        let tpq = 480u16;

        let mut t0 = Vec::new(); // empty tempo track
        t0.extend(ev_eot(0));

        let mut t1 = Vec::new();
        t1.extend(ev_note_on(0, 0, 60, 64));
        t1.extend(ev_note_off(480, 0, 60));
        t1.extend(ev_eot(0));

        let mut midi = midi_header(1, 2, tpq);
        midi.extend(midi_track(t0));
        midi.extend(midi_track(t1));

        let r = SmfParser::parse(&midi).unwrap();
        assert_eq!(r.tempo_at(0), 500_000);
        assert_eq!(r.time_sig_at(0), (4, 4));
        assert_eq!(r.key_sig_at(0), 0);
    }

    #[test]
    fn test_format2_rejected() {
        let mut midi = midi_header(2, 1, 480);
        midi.extend(midi_track(ev_eot(0)));

        let err = SmfParser::parse(&midi).unwrap_err();
        assert!(
            matches!(err, MidiParseError::UnsupportedFormat(2)),
            "Expected UnsupportedFormat(2), got {err:?}"
        );
    }

    #[test]
    fn test_invalid_bytes_rejected() {
        let err = SmfParser::parse(b"not a midi file at all").unwrap_err();
        assert!(
            matches!(err, MidiParseError::MidlyError(_)),
            "Expected MidlyError, got {err:?}"
        );
    }

    #[test]
    fn test_too_short_rejected() {
        let err = SmfParser::parse(b"MThd").unwrap_err();
        assert!(
            matches!(err, MidiParseError::EmptyFile),
            "Expected EmptyFile, got {err:?}"
        );
    }

    #[test]
    fn test_overlapping_same_pitch_fifo() {
        // Tremolo: NoteOn C4 × 2 before any NoteOff — FIFO must match correctly
        let tpq = 480u16;

        let mut t0 = Vec::new();
        t0.extend(ev_eot(0));

        let mut t1 = Vec::new();
        t1.extend(ev_note_on(0, 0, 60, 64)); // first C4 at tick 0
        t1.extend(ev_note_on(240, 0, 60, 80)); // second C4 at tick 240
        t1.extend(ev_note_off(240, 0, 60)); // NoteOff at tick 480 → closes first C4
        t1.extend(ev_note_off(240, 0, 60)); // NoteOff at tick 720 → closes second C4
        t1.extend(ev_eot(0));

        let mut midi = midi_header(1, 2, tpq);
        midi.extend(midi_track(t0));
        midi.extend(midi_track(t1));

        let r = SmfParser::parse(&midi).unwrap();
        assert_eq!(r.notes.len(), 2);

        // FIFO: first NoteOn (tick 0, vel=64) closes at tick 480
        let note0 = r.notes.iter().find(|n| n.start_tick == 0).unwrap();
        assert_eq!(note0.velocity, 64);
        assert_eq!(note0.end_tick, 480);

        // Second NoteOn (tick 240, vel=80) closes at tick 720
        let note1 = r.notes.iter().find(|n| n.start_tick == 240).unwrap();
        assert_eq!(note1.velocity, 80);
        assert_eq!(note1.end_tick, 720);
    }
}
