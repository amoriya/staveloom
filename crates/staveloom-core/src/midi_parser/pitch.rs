use super::event::RawNote;
use crate::models::{Clef, Pitch, Unpitched};

/// [semitone index (0=C)][0=prefer-sharp, 1=prefer-flat] → (step, alter)
const SEMITONE_TABLE: [(&str, f32, &str, f32); 12] = [
    ("C", 0.0, "C", 0.0),  // 0
    ("C", 1.0, "D", -1.0), // 1: C#/Db
    ("D", 0.0, "D", 0.0),  // 2
    ("D", 1.0, "E", -1.0), // 3: D#/Eb
    ("E", 0.0, "E", 0.0),  // 4
    ("F", 0.0, "F", 0.0),  // 5
    ("F", 1.0, "G", -1.0), // 6: F#/Gb
    ("G", 0.0, "G", 0.0),  // 7
    ("G", 1.0, "A", -1.0), // 8: G#/Ab
    ("A", 0.0, "A", 0.0),  // 9
    ("A", 1.0, "B", -1.0), // 10: A#/Bb
    ("B", 0.0, "B", 0.0),  // 11
];

/// GM standard drum map: (midi_num, display_step, display_octave)
const DRUM_MAP: &[(u8, &str, i32)] = &[
    (35, "B", 2), // Acoustic Bass Drum
    (36, "B", 2), // Bass Drum 1
    (38, "A", 3), // Acoustic Snare
    (39, "B", 3), // Hand Clap
    (40, "A", 3), // Electric Snare
    (41, "F", 2), // Low Floor Tom
    (42, "G", 4), // Closed Hi-Hat
    (43, "E", 2), // High Floor Tom
    (44, "A", 4), // Pedal Hi-Hat
    (45, "C", 3), // Low Tom
    (46, "G", 4), // Open Hi-Hat
    (47, "D", 3), // Low-Mid Tom
    (48, "E", 3), // Hi-Mid Tom
    (49, "G", 5), // Crash Cymbal 1
    (50, "A", 4), // High Tom
    (51, "A", 5), // Ride Cymbal 1
    (57, "A", 5), // Crash Cymbal 2
    (59, "B", 5), // Ride Cymbal 2
];

/// Convert a MIDI note number to a `Pitch`.
///
/// `key_fifths` is the MusicXML key-signature fifths value (negative = flats, positive = sharps).
/// Sharps are preferred when `key_fifths >= 0`, flats otherwise.
pub fn midi_to_pitch(midi: u8, key_fifths: i8) -> Pitch {
    let semitone = (midi % 12) as usize;
    // MIDI octave: C4 = 60 → octave = (60/12)-1 = 4
    let octave = (midi as i32 / 12) - 1;

    let prefer_sharp = key_fifths >= 0;
    let (sharp_step, sharp_alter, flat_step, flat_alter) = SEMITONE_TABLE[semitone];

    let (step, alter) = if prefer_sharp {
        (sharp_step, sharp_alter)
    } else {
        (flat_step, flat_alter)
    };

    Pitch {
        step: step.to_string(),
        octave,
        alter: if alter == 0.0 { None } else { Some(alter) },
    }
}

/// Return a treble clef.
pub fn treble_clef() -> Clef {
    Clef {
        number: 1,
        sign: "G".to_string(),
        line: Some(2),
        clef_octave_change: None,
    }
}

/// Return a bass clef.
pub fn bass_clef() -> Clef {
    Clef {
        number: 1,
        sign: "F".to_string(),
        line: Some(4),
        clef_octave_change: None,
    }
}

/// Return a percussion clef.
pub fn percussion_clef() -> Clef {
    Clef {
        number: 1,
        sign: "percussion".to_string(),
        line: None,
        clef_octave_change: None,
    }
}

/// Return an alto clef (C clef on 3rd line — used by viola).
pub fn alto_clef() -> Clef {
    Clef {
        number: 1,
        sign: "C".to_string(),
        line: Some(3),
        clef_octave_change: None,
    }
}

/// Return a tenor clef (C clef on 4th line — used by cello/trombone in high register).
pub fn tenor_clef() -> Clef {
    Clef {
        number: 1,
        sign: "C".to_string(),
        line: Some(4),
        clef_octave_change: None,
    }
}

/// Choose a clef based on the GM program number and average pitch.
///
/// - Drum track (`is_drum = true`): percussion clef
/// - Instrument-specific overrides (viola → alto, cello/trombone/tuba/bassoon/… → bass)
/// - Fallback: mean MIDI pitch ≥ 55 (G3) → treble, below → bass
pub fn choose_clef(notes: &[RawNote], is_drum: bool, program: u8) -> Clef {
    if is_drum {
        return percussion_clef();
    }

    let mean_pitch = if notes.is_empty() {
        None
    } else {
        Some((notes.iter().map(|n| n.pitch as u64).sum::<u64>() / notes.len() as u64) as u8)
    };

    // Instrument-specific clef selection (GM program numbers, 0-indexed)
    match program {
        // Viola → Alto clef (가온음자리표, C clef 3rd line)
        41 => return alto_clef(),

        // Cello → Bass clef; switch to Tenor clef when mean pitch is high (C4+)
        42 => {
            return if mean_pitch.unwrap_or(48) >= 60 {
                tenor_clef()
            } else {
                bass_clef()
            };
        }

        // Contrabass → Bass clef
        43 => return bass_clef(),

        // Timpani → Bass clef
        47 => return bass_clef(),

        // Trombone → Bass clef; switch to Tenor clef when mean pitch is high
        57 => {
            return if mean_pitch.unwrap_or(48) >= 58 {
                tenor_clef()
            } else {
                bass_clef()
            };
        }

        // Tuba → Bass clef
        58 => return bass_clef(),

        // Bassoon → Bass clef (낮은음자리표)
        70 => return bass_clef(),

        _ => {}
    }

    // Pitch-based fallback: G3 (MIDI 55) is the treble/bass boundary
    match mean_pitch {
        Some(m) if m >= 55 => treble_clef(),
        Some(_) => bass_clef(),
        None => treble_clef(),
    }
}

/// Return `Some("x")` for cymbals and hi-hats that use an x-notehead in GM drumset notation.
/// Returns `None` for all other drum instruments (normal noteheads).
pub fn drum_notehead(midi: u8) -> Option<&'static str> {
    match midi {
        39       // Hand Clap
        | 42     // Closed Hi-Hat
        | 44     // Pedal Hi-Hat
        | 46     // Open Hi-Hat
        | 49     // Crash Cymbal 1
        | 51     // Ride Cymbal 1
        | 53     // Ride Bell
        | 55     // Splash Cymbal
        | 57     // Crash Cymbal 2
        | 59     // Ride Cymbal 2
        => Some("x"),
        _ => None,
    }
}

/// Map a GM drum MIDI number to an `Unpitched` position.
///
/// Undefined numbers are mapped to the nearest entry in `DRUM_MAP`.
pub fn drum_position(midi: u8) -> Unpitched {
    let entry = DRUM_MAP
        .iter()
        .min_by_key(|&&(n, _, _)| (n as i16 - midi as i16).unsigned_abs())
        .unwrap();
    Unpitched {
        display_step: entry.1.to_string(),
        display_octave: entry.2,
        midi_number: Some(midi),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- midi_to_pitch ----

    #[test]
    fn test_c4_c_major() {
        let p = midi_to_pitch(60, 0);
        assert_eq!(p.step, "C");
        assert_eq!(p.octave, 4);
        assert_eq!(p.alter, None);
    }

    #[test]
    fn test_c4_sharp_in_c_major() {
        // MIDI 61 = C#/Db; C major (fifths=0) → prefer sharp
        let p = midi_to_pitch(61, 0);
        assert_eq!(p.step, "C");
        assert_eq!(p.octave, 4);
        assert_eq!(p.alter, Some(1.0));
    }

    #[test]
    fn test_bb4_in_f_major() {
        // MIDI 70 = A#/Bb; F major (fifths=-1) → prefer flat
        let p = midi_to_pitch(70, -1);
        assert_eq!(p.step, "B");
        assert_eq!(p.octave, 4);
        assert_eq!(p.alter, Some(-1.0));
    }

    #[test]
    fn test_a_sharp_in_c_major() {
        // MIDI 70 = A#/Bb; C major (fifths=0) → prefer sharp → A#
        let p = midi_to_pitch(70, 0);
        assert_eq!(p.step, "A");
        assert_eq!(p.alter, Some(1.0));
    }

    #[test]
    fn test_eb_in_flat_key() {
        // MIDI 63 = D#/Eb; flat key → Eb
        let p = midi_to_pitch(63, -2);
        assert_eq!(p.step, "E");
        assert_eq!(p.alter, Some(-1.0));
    }

    #[test]
    fn test_d_sharp_in_sharp_key() {
        // MIDI 63 = D#/Eb; sharp key → D#
        let p = midi_to_pitch(63, 2);
        assert_eq!(p.step, "D");
        assert_eq!(p.alter, Some(1.0));
    }

    #[test]
    fn test_natural_notes_have_no_alter() {
        // D E F G A B all natural at C major
        for (midi, expected_step) in &[
            (62u8, "D"),
            (64, "E"),
            (65, "F"),
            (67, "G"),
            (69, "A"),
            (71, "B"),
        ] {
            let p = midi_to_pitch(*midi, 0);
            assert_eq!(p.step, *expected_step, "MIDI {}", midi);
            assert_eq!(p.alter, None, "MIDI {} alter should be None", midi);
        }
    }

    #[test]
    fn test_octave_c4_is_4() {
        // C4 = MIDI 60
        assert_eq!(midi_to_pitch(60, 0).octave, 4);
    }

    #[test]
    fn test_octave_c5_is_5() {
        // C5 = MIDI 72
        assert_eq!(midi_to_pitch(72, 0).octave, 5);
    }

    #[test]
    fn test_octave_c_minus1_is_minus1() {
        // C-1 = MIDI 0
        assert_eq!(midi_to_pitch(0, 0).octave, -1);
    }

    #[test]
    fn test_midi_127_no_panic() {
        // G9 = MIDI 127
        let p = midi_to_pitch(127, 0);
        assert_eq!(p.step, "G");
        assert_eq!(p.octave, 9);
    }

    #[test]
    fn test_all_midi_values_valid_step() {
        let valid_steps = ["C", "D", "E", "F", "G", "A", "B"];
        for midi in 0u8..=127 {
            let p = midi_to_pitch(midi, 0);
            assert!(
                valid_steps.contains(&p.step.as_str()),
                "MIDI {} produced invalid step '{}'",
                midi,
                p.step
            );
        }
    }

    // ---- choose_clef ----

    fn note(pitch: u8) -> super::super::event::RawNote {
        super::super::event::RawNote {
            part_idx: 0,
            channel: 0,
            pitch,
            velocity: 64,
            start_tick: 0,
            end_tick: 480,
        }
    }

    #[test]
    fn test_treble_for_high_notes() {
        // A4 = 69 → mean=69 → treble
        let notes = vec![note(69)];
        let clef = choose_clef(&notes, false, 40);
        assert_eq!(clef.sign, "G");
    }

    #[test]
    fn test_bass_for_low_notes() {
        // E2 = 40 → mean=40 → bass
        let notes = vec![note(40)];
        let clef = choose_clef(&notes, false, 40);
        assert_eq!(clef.sign, "F");
    }

    #[test]
    fn test_percussion_for_drum() {
        let notes = vec![note(38)];
        let clef = choose_clef(&notes, true, 118);
        assert_eq!(clef.sign, "percussion");
    }

    #[test]
    fn test_treble_for_empty_notes() {
        let clef = choose_clef(&[], false, 40);
        assert_eq!(clef.sign, "G");
    }

    #[test]
    fn test_boundary_g3_is_treble() {
        // G3 = MIDI 55 → mean=55 → treble
        let notes = vec![note(55)];
        let clef = choose_clef(&notes, false, 40);
        assert_eq!(clef.sign, "G");
    }

    #[test]
    fn test_boundary_fs3_is_bass() {
        // F#3 = MIDI 54 → mean=54 → bass
        let notes = vec![note(54)];
        let clef = choose_clef(&notes, false, 40);
        assert_eq!(clef.sign, "F");
    }

    #[test]
    fn test_viola_always_alto() {
        // Viola (program 41) → alto clef regardless of pitch
        let notes = vec![note(69)]; // A4, would normally be treble
        let clef = choose_clef(&notes, false, 41);
        assert_eq!(clef.sign, "C");
        assert_eq!(clef.line, Some(3));
    }

    #[test]
    fn test_cello_bass_clef() {
        // Cello (program 42), low range → bass clef
        let notes = vec![note(48)]; // C3
        let clef = choose_clef(&notes, false, 42);
        assert_eq!(clef.sign, "F");
    }

    #[test]
    fn test_cello_tenor_clef_for_high_register() {
        // Cello (program 42), high range → tenor clef
        let notes = vec![note(65)]; // F4 → mean=65 ≥ 60
        let clef = choose_clef(&notes, false, 42);
        assert_eq!(clef.sign, "C");
        assert_eq!(clef.line, Some(4));
    }

    #[test]
    fn test_bassoon_bass_clef() {
        let notes = vec![note(55)]; // G3, would be treble by pitch
        let clef = choose_clef(&notes, false, 70);
        assert_eq!(clef.sign, "F");
    }

    // ---- drum_position ----

    #[test]
    fn test_snare_position() {
        // MIDI 38 = Acoustic Snare → A3
        let u = drum_position(38);
        assert_eq!(u.display_step, "A");
        assert_eq!(u.display_octave, 3);
    }

    #[test]
    fn test_bass_drum_position() {
        // MIDI 36 = Bass Drum 1 → B2
        let u = drum_position(36);
        assert_eq!(u.display_step, "B");
        assert_eq!(u.display_octave, 2);
    }

    #[test]
    fn test_unknown_drum_nearest_fallback() {
        // MIDI 37 is between 36 (B2) and 38 (A3); distance from 36=1, from 38=1
        // min_by_key picks first encountered on tie: DRUM_MAP[1]=(36,"B",2)
        let u = drum_position(37);
        assert!(["A", "B"].contains(&u.display_step.as_str()));
    }

    #[test]
    fn test_high_cymbal_position() {
        // MIDI 49 = Crash Cymbal 1 → G5
        let u = drum_position(49);
        assert_eq!(u.display_step, "G");
        assert_eq!(u.display_octave, 5);
    }

    // ---- drum_notehead ----

    #[test]
    fn test_closed_hihat_x_notehead() {
        assert_eq!(drum_notehead(42), Some("x"));
    }

    #[test]
    fn test_open_hihat_x_notehead() {
        assert_eq!(drum_notehead(46), Some("x"));
    }

    #[test]
    fn test_crash_cymbal_x_notehead() {
        assert_eq!(drum_notehead(49), Some("x"));
    }

    #[test]
    fn test_ride_cymbal_x_notehead() {
        assert_eq!(drum_notehead(51), Some("x"));
    }

    #[test]
    fn test_hand_clap_x_notehead() {
        assert_eq!(drum_notehead(39), Some("x"));
    }

    #[test]
    fn test_snare_normal_notehead() {
        // Acoustic snare (38) uses normal notehead
        assert_eq!(drum_notehead(38), None);
    }

    #[test]
    fn test_bass_drum_normal_notehead() {
        assert_eq!(drum_notehead(36), None);
    }

    #[test]
    fn test_floor_tom_normal_notehead() {
        assert_eq!(drum_notehead(41), None);
    }
}
