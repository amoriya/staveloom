use crate::models::{Clef, Harmony, Lyric, Note, Pitch};

impl super::Renderer {
    pub(crate) fn estimate_text_width(&self, text: &str, font_size: f32) -> f32 {
        let mut width = 0.0;
        for ch in text.chars() {
            width += if ch == ' ' {
                font_size * 0.33
            } else if ch.is_ascii() {
                font_size * 0.55
            } else {
                font_size * 0.72
            };
        }
        width.max(0.0)
    }

    pub(crate) fn estimate_part_label_width(&self, text: &str, font_size: f32) -> f32 {
        self.estimate_text_width(text, font_size) + 4.0
    }

    pub(crate) fn estimate_group_label_width(&self, text: &str, font_size: f32) -> f32 {
        self.estimate_text_width(text, font_size) + 4.0
    }

    pub(crate) fn estimate_lyric_width(&self, lyric: &Lyric) -> f32 {
        let text_chars = lyric.text.chars().count() as f32;
        let mut width = text_chars * 7.0;
        if lyric.text.chars().any(|ch| !ch.is_ascii()) {
            width += text_chars * 2.0;
        }
        if lyric.syllabic.as_deref() != Some("single") && !lyric.text.is_empty() {
            width += 4.0;
        }
        if lyric.extend.is_some() {
            width += 10.0;
        }
        width.max(0.0)
    }

    pub(crate) fn estimate_grace_spacing(&self, note: &Note) -> f32 {
        let accidental_width = if note.accidental.is_some()
            || note
                .pitch
                .as_ref()
                .and_then(|pitch| pitch.alter)
                .is_some_and(|alter| alter.abs() > 0.01)
        {
            10.0
        } else {
            0.0
        };
        14.0 + accidental_width + 2.0
    }

    pub(crate) fn estimate_flag_suffix_width(&self, note: &Note) -> f32 {
        if note.beams.is_empty() && note.stem.as_deref() != Some("none") {
            match note.note_type.as_deref() {
                Some("eighth") => 10.0,
                Some("16th") => 12.0,
                Some("32nd") => 14.0,
                Some("64th") => 16.0,
                _ => 0.0,
            }
        } else {
            0.0
        }
    }

    pub(crate) fn format_alter(&self, alter: f32) -> String {
        if (alter - 1.0).abs() < 0.01 {
            "#".to_string()
        } else if (alter - (-1.0)).abs() < 0.01 {
            "\u{266D}".to_string()
        }
        // ♭ (Flat)
        else if (alter - 2.0).abs() < 0.01 {
            "##".to_string()
        } else if (alter - (-2.0)).abs() < 0.01 {
            "\u{266D}\u{266D}".to_string()
        }
        // ♭♭ (Double flat)
        else if alter > 0.0 {
            format!("+{}", alter)
        } else if alter < 0.0 {
            format!("{}", alter)
        } else {
            "".to_string()
        }
    }

    pub(crate) fn estimate_harmony_width(&self, harmony: &Harmony) -> f32 {
        if let Some(numeral) = &harmony.numeral {
            let root_text = numeral
                .root_text
                .clone()
                .unwrap_or_else(|| numeral.root_value.to_string());
            let mut estimated_w = root_text.len() as f32 * 9.5;
            if numeral.root_alter.is_some() {
                estimated_w += 16.0;
            }
            if harmony.inversion.is_some() {
                estimated_w += 12.0;
            }
            return estimated_w.max(28.0);
        }

        let mut text_len = harmony.root_step.len() as f32;
        if harmony.root_alter.unwrap_or(0.0).abs() > 0.01 {
            text_len += 1.2;
        }

        // kind_display estimation
        let kind_len = if harmony.use_symbols {
            match harmony.kind.as_str() {
                "major-seventh" | "minor-seventh" | "augmented" | "diminished"
                | "half-diminished" | "dominant" => 1.0,
                _ => harmony
                    .kind_text
                    .as_deref()
                    .map(|s| s.len() as f32)
                    .unwrap_or(0.0),
            }
        } else {
            match harmony.kind.as_str() {
                "major" => 0.0,
                "minor" => 1.2,
                "augmented" | "diminished" | "dominant" => 3.2,
                "major-seventh" | "minor-seventh" | "diminished-seventh" => 4.5,
                "half-diminished" => 4.5,
                _ => harmony
                    .kind_text
                    .as_deref()
                    .map(|s| s.len() as f32)
                    .unwrap_or(0.0),
            }
        };
        text_len += kind_len;

        if let Some(bass_step) = &harmony.bass_step {
            if harmony.bass_separator.is_some() {
                text_len += 3.5; // " | " + step
            } else {
                text_len += 2.2; // "/" + step
            }
            text_len += (bass_step.len() as f32 - 1.0).max(0.0);
            if harmony.bass_alter.unwrap_or(0.0).abs() > 0.01 {
                text_len += 1.2;
            }
        }

        for _ in &harmony.degrees {
            text_len += 5.5; // "(#11)" etc
        }

        let mut estimated_w = text_len * 9.5;
        if harmony.frame.is_some() {
            estimated_w = estimated_w.max(48.0);
        }
        estimated_w.max(28.0)
    }

    pub(crate) fn pitch_to_y(&self, pitch: &Pitch, clef: &Clef, y_offset: f32) -> f32 {
        let step_val = match pitch.step.as_str() {
            "C" => 0,
            "D" => 1,
            "E" => 2,
            "F" => 3,
            "G" => 4,
            "A" => 5,
            "B" => 6,
            _ => 0,
        };
        let total_steps = pitch.octave * 7 + step_val;
        let (mut ref_steps, ref_line_idx) = match clef.sign.as_str() {
            "G" => (4 * 7 + 4, 3),
            "F" => (3 * 7 + 3, 1),
            "C" => (4 * 7 + 0, 5 - clef.line.unwrap_or(3)),
            "percussion" => (4 * 7 + 4, 2), // Standard percussion reference: B4 on the middle line
            "TAB" => (4 * 7 + 4, 3),
            _ => (4 * 7 + 4, 3),
        };

        if let Some(shift) = clef.clef_octave_change {
            ref_steps += shift * 7;
        }

        // For 1-line percussion staff, the "middle line" index is 2 (offset from y_offset by 2*dist)
        // pitch_to_y already uses ref_line_idx for this.
        y_offset + (ref_line_idx as f32 * self.staff_line_distance)
            - ((total_steps - ref_steps) as f32 * 0.5 * self.staff_line_distance)
    }
}

/// (step, octave, alter × 100, notehead glyph, print-object)
type NoteheadIdentity = (String, i32, i32, String, Option<bool>);

/// Printed identity of a notehead: same staff position, same accidental, same glyph.
/// Two notes sharing it would occupy the exact same spot on the staff.
/// Returns `None` for anything that isn't a normal notehead (rests, unpitched-less notes).
fn notehead_identity(note: &Note) -> Option<NoteheadIdentity> {
    if note.rest {
        return None;
    }
    let (step, octave, alter) = if let Some(p) = &note.pitch {
        (
            p.step.clone(),
            p.octave,
            (p.alter.unwrap_or(0.0) * 100.0).round() as i32,
        )
    } else if let Some(u) = &note.unpitched {
        (u.display_step.clone(), u.display_octave, 0)
    } else {
        return None;
    };
    let head = note
        .notehead
        .as_ref()
        .map(|h| h.value.clone())
        .unwrap_or_default();
    Some((step, octave, alter, head, note.print_object))
}

/// Collapse exact unisons inside a chord to a single notehead.
///
/// A single voice cannot strike the same written pitch twice, but MIDI-derived scores
/// routinely stack duplicate note-ons into one chord. Rendered literally those duplicates
/// come out as extra stemless noteheads displaced beside the real one (the seconds-
/// displacement rule treats a 0-step interval as a cluster), producing notation that
/// cannot exist. Enharmonic pairs (G# / Ab) keep distinct identities and are preserved.
///
/// Where duplicates carry different markings, the one with the most notations wins so
/// ties, slurs and articulations survive the collapse.
pub(crate) fn dedupe_chord_unisons(notes: Vec<&Note>) -> Vec<&Note> {
    if notes.len() < 2 {
        return notes;
    }
    let mut kept: Vec<(Option<NoteheadIdentity>, &Note)> = Vec::new();
    for note in notes {
        let id = notehead_identity(note);
        let dup = id.as_ref().and_then(|id| {
            kept.iter_mut()
                .find(|(kept_id, _)| kept_id.as_ref() == Some(id))
        });
        match dup {
            Some((_, existing)) => {
                if note.notations.len() > existing.notations.len() {
                    *existing = note;
                }
            }
            None => kept.push((id, note)),
        }
    }
    kept.into_iter().map(|(_, note)| note).collect()
}

#[cfg(test)]
mod tests {
    use super::dedupe_chord_unisons;
    use crate::models::{Notation, Note, Pitch};

    fn note(step: &str, octave: i32, alter: Option<f32>, notations: Vec<Notation>) -> Note {
        Note {
            pitch: Some(Pitch {
                step: step.to_string(),
                octave,
                alter,
            }),
            unpitched: None,
            duration: 30,
            voice: Some(1),
            staff: Some(1),
            stem: Some("up".to_string()),
            note_type: Some("16th".to_string()),
            notehead: None,
            rest: false,
            rest_measure: false,
            is_chord: false,
            is_cue: false,
            grace: None,
            dot_count: 0,
            lyrics: Vec::new(),
            beams: Vec::new(),
            notations,
            accidental: None,
            time_modification: None,
            print_object: None,
            print_dot: None,
            harmonies: Vec::new(),
            instrument: None,
        }
    }

    #[test]
    fn collapses_duplicate_unisons() {
        let a = note("G", 1, None, Vec::new());
        let b = note("G", 1, None, Vec::new());
        let c = note("G", 1, None, Vec::new());
        let kept = dedupe_chord_unisons(vec![&a, &b, &c]);
        assert_eq!(kept.len(), 1);
    }

    #[test]
    fn keeps_distinct_pitches_and_enharmonics() {
        let g = note("G", 1, None, Vec::new());
        let g_sharp = note("G", 1, Some(1.0), Vec::new());
        let a_flat = note("A", 1, Some(-1.0), Vec::new());
        let g_other_octave = note("G", 2, None, Vec::new());
        let kept = dedupe_chord_unisons(vec![&g, &g_sharp, &a_flat, &g_other_octave]);
        assert_eq!(kept.len(), 4);
    }

    #[test]
    fn duplicate_carrying_notations_wins() {
        let plain = note("G", 1, None, Vec::new());
        let tied = note(
            "G",
            1,
            None,
            vec![Notation::Tied {
                note_type: "start".to_string(),
            }],
        );
        let kept = dedupe_chord_unisons(vec![&plain, &tied]);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].notations.len(), 1);
    }
}
