use super::event::RawNote;
use crate::models::TimeModification;

/// (numerator × tpq / denominator, note_type, dot_count)
/// Ordered largest to smallest so matching always finds the best fit first.
const NOTE_VALUE_TABLE: &[(u32, u32, &str, u8)] = &[
    (8, 1, "breve", 0),
    (4, 1, "whole", 0),
    (3, 1, "half", 1),
    (2, 1, "half", 0),
    (3, 2, "quarter", 1),
    (1, 1, "quarter", 0),
    (3, 4, "eighth", 1),
    (1, 2, "eighth", 0),
    (3, 8, "16th", 1),
    (1, 4, "16th", 0),
    (3, 16, "32nd", 1),
    (1, 8, "32nd", 0),
    (1, 16, "64th", 0),
];

fn expected_ticks(num: u32, den: u32, tpq: u32) -> u64 {
    tpq as u64 * num as u64 / den as u64
}

fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// Detects the smallest rhythmic grid used in `notes` (in ticks).
/// Returns a value that is a standard subdivision of `tpq`, including triplet grids.
pub fn detect_grid(notes: &[RawNote], tpq: u32) -> u32 {
    let residuals: Vec<u64> = notes
        .iter()
        .flat_map(|n| [n.start_tick % tpq as u64, n.end_tick % tpq as u64])
        .filter(|&r| r > 0)
        .collect();

    if residuals.is_empty() {
        return tpq / 4;
    }

    let g = residuals.iter().fold(tpq as u64, |acc, &r| gcd(acc, r)) as u32;

    // Build candidates: binary subdivisions (tpq/2^k) interleaved with triplet
    // subdivisions (tpq*2/(3*2^k)), then pick the largest that fits within g.
    // Example at tpq=480: 480, 320, 240, 160, 120, 80, 60, 40, 30 …
    // Without triplet candidates, triplet-eighth notes (160 ticks) would be
    // misidentified as 16th notes (120 ticks), destroying the tuplet grouping.
    let mut candidates: Vec<u32> = Vec::new();
    for k in 0u32..6 {
        let pow2 = 1u32 << k;
        let binary = tpq / pow2;
        if binary > 0 {
            candidates.push(binary);
        }
        let t_num = tpq as u64 * 2;
        let t_den = 3u64 * pow2 as u64;
        if t_num % t_den == 0 {
            let triplet = (t_num / t_den) as u32;
            // Don't add triplet candidates finer than the binary minimum (tpq/32).
            // Below that threshold the GCD more likely reflects sloppy timing than
            // a real tuplet grid, and the fallback below handles it correctly.
            if triplet >= tpq / 32 && triplet > 0 {
                candidates.push(triplet);
            }
        }
    }
    candidates.sort_by(|a, b| b.cmp(a));
    candidates.dedup();

    for candidate in candidates {
        if candidate <= g {
            return candidate;
        }
    }
    tpq / 32
}

/// Convert a tick duration to a note value.
///
/// Returns `(note_type, dot_count, time_modification)`.
/// Returns `None` when no single note value matches (caller should use `decompose_duration`).
pub fn ticks_to_note_value(ticks: u64, tpq: u32) -> Option<(String, u8, Option<TimeModification>)> {
    if ticks == 0 {
        return None;
    }

    // Step 1: direct match
    for &(num, den, note_type, dots) in NOTE_VALUE_TABLE {
        if ticks == expected_ticks(num, den, tpq) {
            return Some((note_type.to_string(), dots, None));
        }
    }

    // Step 2: triplet (3:2) — actual×2 = normal_note × 3
    // triplet eighth at tpq=480: ticks=160 → 160*3=480=tpq → quarter, so actual=3 normal=2
    if ticks * 3 % 2 == 0 {
        let triplet_base = ticks * 3 / 2;
        for &(num, den, note_type, dots) in NOTE_VALUE_TABLE {
            if dots == 0 && triplet_base == expected_ticks(num, den, tpq) {
                return Some((
                    note_type.to_string(),
                    0,
                    Some(TimeModification {
                        actual_notes: 3,
                        normal_notes: 2,
                        normal_type: Some(note_type.to_string()),
                        normal_dot_count: 0,
                    }),
                ));
            }
        }
    }

    // Step 3: other tuplet ratios (5:4, 7:4, 7:8, 6:4)
    for &(actual, normal) in &[(5u64, 4u64), (7, 4), (7, 8), (6, 4)] {
        if ticks * actual % normal == 0 {
            let base = ticks * actual / normal;
            for &(num, den, note_type, _) in NOTE_VALUE_TABLE {
                if base == expected_ticks(num, den, tpq) {
                    return Some((
                        note_type.to_string(),
                        0,
                        Some(TimeModification {
                            actual_notes: actual as i32,
                            normal_notes: normal as i32,
                            normal_type: Some(note_type.to_string()),
                            normal_dot_count: 0,
                        }),
                    ));
                }
            }
        }
    }

    None
}

/// Like [`ticks_to_note_value`] but accepts a small timing tolerance.
///
/// Tries exact match first, then probes ±1 … ±`tolerance` ticks.
/// Use `tolerance = tpq / 64` (one 64th note) for notation-software MIDI,
/// or `tpq / 16` for performance MIDI where rounding errors accumulate.
pub fn ticks_to_note_value_tolerant(
    ticks: u64,
    tpq: u32,
    tolerance: u64,
) -> Option<(String, u8, Option<TimeModification>)> {
    if let Some(r) = ticks_to_note_value(ticks, tpq) {
        return Some(r);
    }
    for delta in 1..=tolerance {
        if ticks >= delta {
            if let Some(r) = ticks_to_note_value(ticks - delta, tpq) {
                return Some(r);
            }
        }
        if let Some(r) = ticks_to_note_value(ticks + delta, tpq) {
            return Some(r);
        }
    }
    None
}

/// Greedily decompose a duration into standard note values.
///
/// Each element is `(note_type, dot_count, time_modification)`.
/// When the result has more than one element the caller should emit tied notes.
pub fn decompose_duration(ticks: u64, tpq: u32) -> Vec<(String, u8, Option<TimeModification>)> {
    let mut remaining = ticks;
    let mut parts = Vec::new();

    while remaining > 0 {
        let mut matched = false;
        for &(num, den, note_type, dots) in NOTE_VALUE_TABLE {
            let val = expected_ticks(num, den, tpq);
            if val > 0 && val <= remaining {
                parts.push((note_type.to_string(), dots, None));
                remaining -= val;
                matched = true;
                break;
            }
        }
        if !matched {
            break;
        }
    }

    parts
}

/// Snap a note's start/end ticks to the nearest grid boundary.
///
/// Guarantees end > start by at least one grid unit.
pub fn quantize_note(note: &RawNote, grid: u32) -> RawNote {
    let snap = |tick: u64| -> u64 {
        let g = grid as u64;
        let r = tick % g;
        if r <= g / 2 { tick - r } else { tick + (g - r) }
    };

    let start = snap(note.start_tick);
    let mut end = snap(note.end_tick);
    if end <= start {
        end = start + grid as u64;
    }

    RawNote {
        start_tick: start,
        end_tick: end,
        ..*note
    }
}

/// Returns `true` when the MIDI part is likely a human performance rather than
/// a DAW or notation-software export.
///
/// **Both** signals must fire (AND logic) to avoid over-triggering on DAW
/// files whose individual instrument parts happen to land on an odd tick
/// position while using a small set of velocities:
///
/// - **Tick GCD < a 32nd note** (`tpq/16`): onset positions fall between
///   standard grid lines — impossible for a correctly-quantized DAW export but
///   common when recording a live keyboard performance.
/// - **Unique velocity count > 6**: expressive dynamics from live playing;
///   DAW/notation exports typically use ≤ 6 distinct velocity levels.
pub fn is_human_midi(notes: &[RawNote], tpq: u32) -> bool {
    if notes.is_empty() || tpq == 0 {
        return false;
    }

    // Signal 1: GCD of onset ticks only (not end/offset ticks).
    // Note-off positions reflect how long a key was held, not rhythmic intent;
    // DAW notes held for non-grid durations produce small end-tick GCDs even
    // in perfectly-quantized arrangements.  Onset positions are what quantization
    // actually corrects, so we limit the GCD to those.
    // A 32nd-note threshold (tpq/16).  Any GCD below this means notes land
    // between standard grid positions — characteristic of live performance.
    let g = notes
        .iter()
        .map(|n| n.start_tick)
        .filter(|&t| t > 0)
        .fold(0u64, |acc, t| gcd(acc, t));
    let half_32nd = ((tpq as u64) / 16).max(1);
    let sloppy_grid = g > 0 && g < half_32nd;

    // Signal 2: unique velocity count.
    // Live playing produces many distinct velocity values; DAW exports use a
    // small discrete set.  More than 6 unique values is the threshold.
    let unique_vels = notes
        .iter()
        .map(|n| n.velocity)
        .collect::<std::collections::HashSet<_>>()
        .len();
    let expressive_velocity = unique_vels > 6;

    // AND: require both signals.  Using OR would misclassify DAW instrument
    // parts that have one oddly-placed note (GCD→1) but few velocities.
    sloppy_grid && expressive_velocity
}

/// Decide whether tolerance quantization should be applied.
///
/// Returns `true` when the average residual exceeds 15% of the grid, indicating
/// a human-performance MIDI rather than a notation-software export.
pub fn needs_tolerance_quantization(notes: &[RawNote], tpq: u32) -> bool {
    let grid = detect_grid(notes, tpq);
    if grid == 0 || notes.is_empty() {
        return false;
    }

    let residuals: Vec<u64> = notes
        .iter()
        .flat_map(|n| [n.start_tick % grid as u64, n.end_tick % grid as u64])
        .filter(|&r| r > 0)
        .collect();

    if residuals.is_empty() {
        return false;
    }

    let avg = residuals.iter().sum::<u64>() / residuals.len() as u64;
    avg > grid as u64 * 15 / 100
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::midi_parser::event::RawNote;

    fn note(start: u64, end: u64) -> RawNote {
        RawNote {
            part_idx: 0,
            channel: 0,
            pitch: 60,
            velocity: 64,
            start_tick: start,
            end_tick: end,
        }
    }

    const TPQ: u32 = 480;

    // ---- ticks_to_note_value ----

    #[test]
    fn test_quarter() {
        assert_eq!(
            ticks_to_note_value(480, TPQ),
            Some(("quarter".into(), 0, None))
        );
    }

    #[test]
    fn test_half() {
        assert_eq!(
            ticks_to_note_value(960, TPQ),
            Some(("half".into(), 0, None))
        );
    }

    #[test]
    fn test_whole() {
        assert_eq!(
            ticks_to_note_value(1920, TPQ),
            Some(("whole".into(), 0, None))
        );
    }

    #[test]
    fn test_eighth() {
        assert_eq!(
            ticks_to_note_value(240, TPQ),
            Some(("eighth".into(), 0, None))
        );
    }

    #[test]
    fn test_16th() {
        assert_eq!(
            ticks_to_note_value(120, TPQ),
            Some(("16th".into(), 0, None))
        );
    }

    #[test]
    fn test_32nd() {
        assert_eq!(ticks_to_note_value(60, TPQ), Some(("32nd".into(), 0, None)));
    }

    #[test]
    fn test_64th() {
        assert_eq!(ticks_to_note_value(30, TPQ), Some(("64th".into(), 0, None)));
    }

    #[test]
    fn test_dotted_quarter() {
        assert_eq!(
            ticks_to_note_value(720, TPQ),
            Some(("quarter".into(), 1, None))
        );
    }

    #[test]
    fn test_double_dotted_quarter() {
        // Double-dotted notes are no longer returned as a single value;
        // decompose_duration splits them into (dotted + small) instead.
        assert_eq!(ticks_to_note_value(840, TPQ), None);
        let parts = super::decompose_duration(840, TPQ);
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].0, "quarter");
        assert_eq!(parts[0].1, 1); // dotted quarter
        assert_eq!(parts[1].0, "16th");
        assert_eq!(parts[1].1, 0); // sixteenth
    }

    #[test]
    fn test_dotted_half() {
        assert_eq!(
            ticks_to_note_value(1440, TPQ),
            Some(("half".into(), 1, None))
        );
    }

    #[test]
    fn test_dotted_eighth() {
        assert_eq!(
            ticks_to_note_value(360, TPQ),
            Some(("eighth".into(), 1, None))
        );
    }

    #[test]
    fn test_dotted_16th() {
        assert_eq!(
            ticks_to_note_value(180, TPQ),
            Some(("16th".into(), 1, None))
        );
    }

    #[test]
    fn test_triplet_quarter() {
        // TPQ=480: triplet quarter = 480 * 2/3 = 320
        let result = ticks_to_note_value(320, TPQ).unwrap();
        assert_eq!(result.0, "quarter");
        assert_eq!(result.1, 0);
        let tm = result.2.unwrap();
        assert_eq!(tm.actual_notes, 3);
        assert_eq!(tm.normal_notes, 2);
        assert_eq!(tm.normal_type.as_deref(), Some("quarter"));
    }

    #[test]
    fn test_triplet_eighth() {
        // triplet eighth = 240 * 2/3 = 160
        let result = ticks_to_note_value(160, TPQ).unwrap();
        assert_eq!(result.0, "eighth");
        let tm = result.2.unwrap();
        assert_eq!(tm.actual_notes, 3);
        assert_eq!(tm.normal_notes, 2);
    }

    #[test]
    fn test_triplet_half() {
        // triplet half = 960 * 2/3 = 640
        let result = ticks_to_note_value(640, TPQ).unwrap();
        assert_eq!(result.0, "half");
        let tm = result.2.unwrap();
        assert_eq!(tm.actual_notes, 3);
        assert_eq!(tm.normal_notes, 2);
    }

    #[test]
    fn test_quintuplet_quarter() {
        // quintuplet: 5 in place of 4 quarter notes
        // each = tpq * 4/5 = 384
        let result = ticks_to_note_value(384, TPQ).unwrap();
        assert_eq!(result.0, "quarter");
        let tm = result.2.unwrap();
        assert_eq!(tm.actual_notes, 5);
        assert_eq!(tm.normal_notes, 4);
    }

    #[test]
    fn test_septuplet_quarter() {
        // septuplet: 7 in place of 4 quarter notes → each = tpq * 4/7
        // tpq=480: 480*4/7 = 274 (integer division)
        // 274 * 7 = 1918 ≠ 1920, so this won't match exactly.
        // Use 7:8 instead: each = tpq * 8/7
        // tpq=480: 480*8/7 = 548 (integer division) → not exact either.
        // The table only handles exact integer ticks; just verify no panic.
        let _ = ticks_to_note_value(137, TPQ);
    }

    #[test]
    fn test_no_match_returns_none() {
        // 1 tick — no note value at TPQ=480
        assert_eq!(ticks_to_note_value(1, TPQ), None);
    }

    // ---- ticks_to_note_value_tolerant ----

    #[test]
    fn test_tolerant_exact_match_unchanged() {
        // Exact quarter → should still match
        let r = ticks_to_note_value_tolerant(480, TPQ, 4).unwrap();
        assert_eq!(r.0, "quarter");
        assert_eq!(r.1, 0);
    }

    #[test]
    fn test_tolerant_minus_one_tick() {
        // 479 ≈ quarter (480), tolerance=2
        let r = ticks_to_note_value_tolerant(479, TPQ, 2).unwrap();
        assert_eq!(r.0, "quarter");
    }

    #[test]
    fn test_tolerant_plus_one_tick() {
        // 481 ≈ quarter (480), tolerance=2
        let r = ticks_to_note_value_tolerant(481, TPQ, 2).unwrap();
        assert_eq!(r.0, "quarter");
    }

    #[test]
    fn test_tolerant_outside_tolerance_returns_none() {
        // 475 is 5 away from 480; tolerance=2 → no match from quarter
        // and no other standard value is within 2 of 475 either
        let r = ticks_to_note_value_tolerant(475, TPQ, 2);
        assert!(r.is_none());
    }

    #[test]
    fn test_tolerant_dotted_quarter_with_rounding() {
        // Dotted quarter = 720; human performance might give 718–722
        let r = ticks_to_note_value_tolerant(718, TPQ, 3).unwrap();
        assert_eq!(r.0, "quarter");
        assert_eq!(r.1, 1); // 1 dot
    }

    // ---- decompose_duration ----

    #[test]
    fn test_decompose_quarter_plus_16th() {
        // 600 = 480 (quarter) + 120 (16th)
        let parts = decompose_duration(600, TPQ);
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].0, "quarter");
        assert_eq!(parts[1].0, "16th");
    }

    #[test]
    fn test_decompose_whole_plus_quarter() {
        // 2400 = 1920 (whole) + 480 (quarter)
        let parts = decompose_duration(2400, TPQ);
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].0, "whole");
        assert_eq!(parts[1].0, "quarter");
    }

    #[test]
    fn test_decompose_exact_match_single() {
        // 720 = dotted quarter — should be single element
        let parts = decompose_duration(720, TPQ);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].0, "quarter");
        assert_eq!(parts[0].1, 1);
    }

    #[test]
    fn test_decompose_zero() {
        assert!(decompose_duration(0, TPQ).is_empty());
    }

    // ---- detect_grid ----

    #[test]
    fn test_detect_grid_quarter_aligned() {
        // notes all on quarter-note boundaries → grid = tpq/4 default
        let notes = vec![note(0, 480), note(480, 960), note(960, 1440)];
        let g = detect_grid(&notes, TPQ);
        // All ticks divisible by tpq → residuals empty → default tpq/4 = 120
        assert_eq!(g, 120);
    }

    #[test]
    fn test_detect_grid_eighth_aligned() {
        // notes on eighth-note boundaries (240-tick steps)
        let notes = vec![note(0, 240), note(240, 480), note(480, 720)];
        let g = detect_grid(&notes, TPQ);
        // residuals: 240, 240 (mod 480) → gcd=240 → smallest std sub ≤240 = 240 (tpq/2)
        assert_eq!(g, 240);
    }

    #[test]
    fn test_detect_grid_16th_aligned() {
        // mix of 8th and 16th → grid = 16th = 120
        let notes = vec![note(0, 120), note(120, 240), note(240, 480)];
        let g = detect_grid(&notes, TPQ);
        assert_eq!(g, 120);
    }

    #[test]
    fn test_detect_grid_empty() {
        let g = detect_grid(&[], TPQ);
        assert_eq!(g, TPQ / 4);
    }

    // ---- quantize_note ----

    #[test]
    fn test_quantize_already_aligned() {
        let n = note(0, 480);
        let q = quantize_note(&n, 120);
        assert_eq!(q.start_tick, 0);
        assert_eq!(q.end_tick, 480);
    }

    #[test]
    fn test_quantize_snaps_to_grid() {
        // start=478 (2 below 480), end=962 (2 above 960) → grid=120
        let n = note(478, 962);
        let q = quantize_note(&n, 120);
        assert_eq!(q.start_tick, 480);
        assert_eq!(q.end_tick, 960);
    }

    #[test]
    fn test_quantize_guarantees_min_duration() {
        // start and end both snap to same tick → end += grid
        let n = note(479, 481);
        let q = quantize_note(&n, 480);
        // Both snap to 480; end pushed to 960
        assert_eq!(q.start_tick, 480);
        assert_eq!(q.end_tick, 960);
    }

    #[test]
    fn test_quantize_preserves_other_fields() {
        let n = RawNote {
            part_idx: 2,
            channel: 5,
            pitch: 69,
            velocity: 100,
            start_tick: 0,
            end_tick: 480,
        };
        let q = quantize_note(&n, 120);
        assert_eq!(q.part_idx, 2);
        assert_eq!(q.channel, 5);
        assert_eq!(q.pitch, 69);
        assert_eq!(q.velocity, 100);
    }

    // ---- is_human_midi ----

    // AND logic: both sloppy timing AND many velocities required.

    #[test]
    fn test_is_human_midi_sloppy_and_expressive() {
        // gcd=1 AND 20 unique velocities → human
        let notes: Vec<RawNote> = (0..20u64)
            .map(|i| RawNote {
                part_idx: 0,
                channel: 0,
                pitch: 60,
                velocity: (i * 5 + 20) as u8, // 20 distinct velocities
                start_tick: i * 120 + 1,      // gcd=1 (off-grid)
                end_tick: i * 120 + 121,
            })
            .collect();
        assert!(is_human_midi(&notes, TPQ));
    }

    #[test]
    fn test_is_human_midi_sloppy_but_few_velocities() {
        // gcd=1 but only 1 unique velocity → DAW (AND fails on velocity)
        let notes = vec![note(1, 481), note(483, 963)];
        // note() always uses velocity=64 → unique_vels=1, not > 6
        assert!(!is_human_midi(&notes, TPQ));
    }

    #[test]
    fn test_is_human_midi_many_velocities_but_on_grid() {
        // 20 unique velocities but on-grid (gcd=120) → DAW (AND fails on gcd)
        // gcd=120 = 16th note at tpq=480; threshold = tpq/16 = 30; 120 >= 30 → not sloppy
        let notes: Vec<RawNote> = (0..20u64)
            .map(|i| RawNote {
                part_idx: 0,
                channel: 0,
                pitch: 60,
                velocity: (i * 5 + 20) as u8,
                start_tick: i * 120, // gcd=120, a standard 16th-note grid
                end_tick: i * 120 + 120,
            })
            .collect();
        assert!(!is_human_midi(&notes, TPQ));
    }

    #[test]
    fn test_is_human_midi_sloppy_with_7_velocities() {
        // gcd=5 AND 7 unique velocities → human (covers rock band human MIDI)
        // tpq=120, threshold = 120/16 = 7; gcd=5 < 7 → sloppy; 7 vels > 6 → expressive
        let notes: Vec<RawNote> = (0..7u64)
            .map(|i| RawNote {
                part_idx: 0,
                channel: 0,
                pitch: 60,
                velocity: (i * 10 + 40) as u8, // 7 distinct velocities
                start_tick: i * 120 + 5,       // gcd=5 (off-grid)
                end_tick: i * 120 + 125,
            })
            .collect();
        assert!(is_human_midi(&notes, 120));
    }

    #[test]
    fn test_is_human_midi_empty() {
        assert!(!is_human_midi(&[], TPQ));
    }

    // ---- needs_tolerance_quantization ----

    #[test]
    fn test_no_tolerance_needed_for_exact_grid() {
        let notes = vec![note(0, 480), note(480, 960)];
        assert!(!needs_tolerance_quantization(&notes, TPQ));
    }

    #[test]
    fn test_tolerance_needed_for_sloppy_timing() {
        // Notes with large residuals (>15% of grid)
        // grid = tpq/4 = 120; 15% = 18 ticks; put residuals of ~50
        let notes = vec![note(50, 470), note(530, 950)];
        assert!(needs_tolerance_quantization(&notes, TPQ));
    }
}
