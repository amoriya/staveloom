use crate::models::{Beam, BeamValue, MeasureElement, Notation, Score};

fn beam_levels_for_type(note_type: &str) -> usize {
    match note_type {
        "eighth" => 1,
        "16th" => 2,
        "32nd" => 3,
        "64th" => 4,
        "128th" => 5,
        _ => 0,
    }
}

struct NoteSlot {
    chord_idxs: Vec<usize>,
    tick: u64,
    level: usize,
}

fn collect_slots(elements: &[MeasureElement]) -> Vec<NoteSlot> {
    let mut slots = Vec::new();
    let mut tick = 0u64;
    let mut i = 0;
    while i < elements.len() {
        if let MeasureElement::Note(n) = &elements[i] {
            if !n.is_chord {
                let dur = n.duration.max(0) as u64;
                let lvl = if n.rest {
                    0
                } else {
                    n.note_type
                        .as_deref()
                        .map(beam_levels_for_type)
                        .unwrap_or(0)
                };
                let mut chord_idxs = vec![i];
                let mut j = i + 1;
                while j < elements.len() {
                    if let MeasureElement::Note(cn) = &elements[j] {
                        if cn.is_chord {
                            chord_idxs.push(j);
                            j += 1;
                            continue;
                        }
                    }
                    break;
                }
                slots.push(NoteSlot {
                    chord_idxs,
                    tick,
                    level: lvl,
                });
                tick += dur;
                i = j;
                continue;
            }
        }
        i += 1;
    }
    slots
}

fn emit_beam_run(
    group: &[&NoteSlot],
    run_start: usize,
    run_end: usize,
    level: usize,
    assignments: &mut Vec<(usize, i32, BeamValue)>,
) {
    let run_len = run_end - run_start + 1;
    for k in run_start..=run_end {
        let slot = group[k];
        let value = if run_len == 1 {
            if k == group.len() - 1 {
                BeamValue::BackwardHook
            } else {
                BeamValue::ForwardHook
            }
        } else if k == run_start {
            BeamValue::Begin
        } else if k == run_end {
            BeamValue::End
        } else {
            BeamValue::Continue
        };
        for &elem_idx in &slot.chord_idxs {
            assignments.push((elem_idx, level as i32, value.clone()));
        }
    }
}

fn assign_beams(elements: &mut Vec<MeasureElement>, tpq: u32, ts_num: u8, ts_den: u8) {
    let ts_den = ts_den.max(1);
    let beat_unit = (tpq as u64 * 4) / ts_den as u64;
    // Compound time (6/8, 9/8, 12/8...): beam over dotted beats
    let is_compound = ts_num % 3 == 0 && ts_num > 3;
    let beat_group = if is_compound {
        beat_unit * 3
    } else {
        beat_unit
    };
    if beat_group == 0 {
        return;
    }

    let slots = collect_slots(elements);

    // Find beam groups: consecutive beamable slots within the same beat group.
    // A rest slot (level == 0) or a non-beamable note breaks the group.
    let mut beam_groups: Vec<Vec<usize>> = Vec::new(); // indices into `slots`
    let mut current: Vec<usize> = Vec::new();
    let mut current_gid: Option<u64> = None;

    for (i, slot) in slots.iter().enumerate() {
        if slot.level == 0 {
            if current.len() >= 2 {
                beam_groups.push(std::mem::take(&mut current));
            } else {
                current.clear();
            }
            current_gid = None;
            continue;
        }
        let gid = slot.tick / beat_group;
        if current_gid == Some(gid) {
            current.push(i);
        } else {
            if current.len() >= 2 {
                beam_groups.push(std::mem::take(&mut current));
            } else {
                current.clear();
            }
            current = vec![i];
            current_gid = Some(gid);
        }
    }
    if current.len() >= 2 {
        beam_groups.push(current);
    }

    let mut assignments: Vec<(usize, i32, BeamValue)> = Vec::new();

    for group_idxs in &beam_groups {
        let group: Vec<&NoteSlot> = group_idxs.iter().map(|&i| &slots[i]).collect();
        let max_level = group.iter().map(|s| s.level).max().unwrap_or(0);

        for level in 1..=max_level {
            let mut run_start: Option<usize> = None;
            for (k, slot) in group.iter().enumerate() {
                if slot.level >= level {
                    if run_start.is_none() {
                        run_start = Some(k);
                    }
                } else if let Some(rs) = run_start.take() {
                    emit_beam_run(&group, rs, k - 1, level, &mut assignments);
                }
            }
            if let Some(rs) = run_start {
                emit_beam_run(&group, rs, group.len() - 1, level, &mut assignments);
            }
        }
    }

    for (elem_idx, beam_number, value) in assignments {
        if let MeasureElement::Note(n) = &mut elements[elem_idx] {
            n.beams.push(Beam {
                number: beam_number,
                value,
            });
        }
    }
}

/// Apply automatic beaming to all measures in `score` that don't already have beams.
///
/// Safe to call on MusicXML-parsed scores: measures that already contain beam data
/// are left untouched. MIDI-parsed scores (which always have empty beams) are fully beamed.
pub fn auto_beam_score(score: &mut Score) {
    for part in &mut score.parts {
        let mut current_divisions: u32 = 480;
        let mut current_ts_num: u8 = 4;
        let mut current_ts_den: u8 = 4;

        for measure in &mut part.measures {
            // Update divisions and time signature from measure.attributes
            if let Some(attr) = &measure.attributes {
                if let Some(d) = attr.divisions {
                    current_divisions = d.max(1) as u32;
                }
                if let Some(time) = &attr.time {
                    if let Ok(n) = time.beats.parse::<u8>() {
                        current_ts_num = n.max(1);
                    }
                    current_ts_den = (time.beat_type as u8).max(1);
                }
            }

            // Also scan inline Attributes (time-sig / divisions changes mid-piece)
            for el in &measure.elements {
                if let MeasureElement::Attributes(attr) = el {
                    if let Some(d) = attr.divisions {
                        current_divisions = d.max(1) as u32;
                    }
                    if let Some(time) = &attr.time {
                        if let Ok(n) = time.beats.parse::<u8>() {
                            current_ts_num = n.max(1);
                        }
                        current_ts_den = (time.beat_type as u8).max(1);
                    }
                }
            }

            // Skip measures that already carry beam information
            let has_beams = measure
                .elements
                .iter()
                .any(|el| matches!(el, MeasureElement::Note(n) if !n.beams.is_empty()));
            if has_beams {
                continue;
            }

            assign_beams(
                &mut measure.elements,
                current_divisions,
                current_ts_num,
                current_ts_den,
            );
        }
    }
}

/// Replace every double-dotted note (dot_count == 2) with two tied notes:
///   dotted note (dot_count=1, duration × 6/7) + smaller note (dot_count=0, duration / 7).
///
/// Example: double-dotted quarter (840 ticks) → dotted quarter (720) tied to sixteenth (120).
///
/// Ties on the original note are preserved: the first expansion note inherits any
/// incoming tie_stop, and the second inherits any outgoing tie_start.
pub fn expand_double_dotted_notes(score: &mut Score) {
    for part in &mut score.parts {
        for measure in &mut part.measures {
            let old_elements = std::mem::take(&mut measure.elements);
            measure.elements = expand_in_measure(old_elements);
        }
    }
}

fn note_type_two_levels_smaller(note_type: &str) -> &'static str {
    match note_type {
        "breve" => "half",
        "whole" => "quarter",
        "half" => "eighth",
        "quarter" => "16th",
        "eighth" => "32nd",
        "16th" => "64th",
        _ => "64th",
    }
}

fn expand_in_measure(elements: Vec<MeasureElement>) -> Vec<MeasureElement> {
    let mut result: Vec<MeasureElement> = Vec::with_capacity(elements.len());
    let mut i = 0;

    while i < elements.len() {
        let needs_expand = matches!(&elements[i], MeasureElement::Note(n)
            if n.dot_count == 2 && !n.is_chord);

        if !needs_expand {
            result.push(elements[i].clone());
            i += 1;
            continue;
        }

        // Collect the chord group: leader + consecutive is_chord companions
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

        let chord: Vec<_> = (i..j)
            .filter_map(|k| {
                if let MeasureElement::Note(n) = &elements[k] {
                    Some(n)
                } else {
                    None
                }
            })
            .collect();

        let is_rest = chord[0].rest;
        let orig_dur = chord[0].duration;
        let part2_dur = orig_dur / 7;
        let part1_dur = orig_dur - part2_dur;

        let base_type = chord[0].note_type.as_deref().unwrap_or("quarter");
        let small_type = note_type_two_levels_smaller(base_type);

        // --- First chord / rest: dot_count=1 ---
        for (ci, src) in chord.iter().enumerate() {
            let had_tie_stop = src
                .notations
                .iter()
                .any(|no| matches!(no, Notation::Tied { note_type } if note_type == "stop"));

            let mut n1 = (*src).clone();
            n1.duration = part1_dur;
            n1.dot_count = 1;
            n1.is_chord = ci > 0;
            if !is_rest {
                n1.notations
                    .retain(|no| !matches!(no, Notation::Tied { .. }));
                if had_tie_stop {
                    n1.notations.push(Notation::Tied {
                        note_type: "stop".to_string(),
                    });
                }
                n1.notations.push(Notation::Tied {
                    note_type: "start".to_string(),
                });
            }
            result.push(MeasureElement::Note(n1));
        }

        // --- Second chord / rest: dot_count=0, smaller type ---
        for (ci, src) in chord.iter().enumerate() {
            let had_tie_start = src
                .notations
                .iter()
                .any(|no| matches!(no, Notation::Tied { note_type } if note_type == "start"));

            let mut n2 = (*src).clone();
            n2.duration = part2_dur;
            n2.dot_count = 0;
            n2.note_type = Some(small_type.to_string());
            n2.is_chord = ci > 0;
            n2.accidental = None;
            n2.beams = Vec::new();
            if !is_rest {
                n2.notations
                    .retain(|no| !matches!(no, Notation::Tied { .. }));
                n2.notations.push(Notation::Tied {
                    note_type: "stop".to_string(),
                });
                if had_tie_start {
                    n2.notations.push(Notation::Tied {
                        note_type: "start".to_string(),
                    });
                }
            }
            result.push(MeasureElement::Note(n2));
        }

        i = j;
    }

    result
}
