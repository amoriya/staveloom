use crate::models::{Clef, Key, Score, Time};
use std::collections::HashMap;

impl super::Renderer {
    pub(crate) fn analyze_measures(&self, score: &Score) -> MeasureAnalysisResult {
        let max_measures = score
            .parts
            .iter()
            .map(|p| p.measures.len())
            .max()
            .unwrap_or(0);

        let mut result = MeasureAnalysisResult {
            max_measures,
            raw_widths: vec![self.measure_width; max_measures],
            system_start_extra_widths: vec![0.0; max_measures],
            spacings: Vec::with_capacity(max_measures),
            total_durs: vec![0; max_measures],
            theoretical_durs: vec![0; max_measures],
            divisions_per_part: HashMap::new(),
        };

        let mut current_divisions: HashMap<String, i32> = HashMap::new();
        let mut global_clefs: HashMap<String, HashMap<i32, Clef>> = HashMap::new();
        let mut global_key: HashMap<String, Key> = HashMap::new();
        let mut global_time: HashMap<String, Time> = HashMap::new();

        for part in &score.parts {
            current_divisions.insert(part.id.clone(), 1);
            let mut initial_clefs = HashMap::new();
            initial_clefs.insert(
                1,
                Clef {
                    number: 1,
                    sign: "G".to_string(),
                    line: Some(2),
                    ..Default::default()
                },
            );
            global_clefs.insert(part.id.clone(), initial_clefs);
        }

        let mut skip_remaining = 0;

        for m_idx in 0..max_measures {
            let mut is_start_of_multirest = false;
            let mut system_start_attr_w: f32 = 0.0;
            let mut initial_attr_w: f32 = 0.0;
            let mut max_dur = 0;
            let mut max_theoretical_dur = 0;
            let mut onset_needs: std::collections::BTreeMap<
                i32,
                crate::renderer::types::OnsetNeeds,
            > = std::collections::BTreeMap::new();

            for part in &score.parts {
                if m_idx < part.measures.len() {
                    let measure = &part.measures[m_idx];
                    let p_id = &part.id;
                    let staff_dist = self.grand_staff_distance;

                    // Space needed if this measure starts a new system
                    let temp_attr = crate::models::Attributes {
                        clefs: global_clefs
                            .get(p_id)
                            .cloned()
                            .unwrap_or_default()
                            .into_values()
                            .collect(),
                        key: global_key.get(p_id).cloned(),
                        time: global_time.get(p_id).cloned(),
                        ..Default::default()
                    };
                    let (_, consumed_x) = self.draw_attributes(
                        svg::Document::new(),
                        &temp_attr,
                        0.0,
                        0.0,
                        &global_clefs.get(p_id).cloned().unwrap_or_default(),
                        1,
                        staff_dist,
                        &HashMap::new(),
                        false,
                    );
                    system_start_attr_w = system_start_attr_w.max(consumed_x);

                    let mut p_time = 0;
                    let mut p_chord_time = 0;
                    let mut divisions = *current_divisions.get(p_id).unwrap_or(&1);
                    let mut num_staves_sim = 1;
                    let mut current_clefs_sim = global_clefs.get(p_id).cloned().unwrap_or_default();
                    let mut pending_grace_w = 0.0;

                    if let Some(attr) = &measure.attributes {
                        if let Some(count) = attr.multiple_rest {
                            if count > 1 {
                                skip_remaining = count - 1;
                                is_start_of_multirest = true;
                                result.raw_widths[m_idx] = self.measure_width;
                            }
                        }
                        if let Some(d) = attr.divisions {
                            divisions = d;
                            current_divisions.insert(p_id.clone(), d);
                        }
                    }

                    for el in &measure.elements {
                        match el {
                            crate::models::MeasureElement::Harmony(h) => {
                                let entry = onset_needs
                                    .entry(p_time)
                                    .or_insert_with(crate::renderer::types::OnsetNeeds::default);
                                let w = self.estimate_harmony_width(h);
                                let extents = entry.dir_extents.entry((0, false)).or_default();
                                extents.left_width = extents.left_width.max(w / 2.0);
                                extents.right_width = extents.right_width.max(w / 2.0);
                            }
                            crate::models::MeasureElement::Note(n) => {
                                let time = if n.is_chord { p_chord_time } else { p_time };
                                let has_arpeggio = n.notations.iter().any(|not| {
                                    matches!(
                                        not,
                                        crate::models::Notation::Arpeggiate { .. }
                                            | crate::models::Notation::NonArpeggiate { .. }
                                    )
                                });
                                let has_accid_val = n.accidental.is_some()
                                    || n.pitch.as_ref().map_or(false, |p| {
                                        p.alter.is_some() && p.alter.unwrap().abs() > 0.01
                                    });

                                let entry = onset_needs
                                    .entry(time)
                                    .or_insert_with(crate::renderer::types::OnsetNeeds::default);

                                for h in &n.harmonies {
                                    let w = self.estimate_harmony_width(h);
                                    let extents = entry.dir_extents.entry((0, false)).or_default();
                                    extents.left_width = extents.left_width.max(w / 2.0);
                                    extents.right_width = extents.right_width.max(w / 2.0);
                                }

                                if n.grace.is_some() {
                                    pending_grace_w += self.estimate_grace_spacing(n);
                                    entry.grace_prefix = entry.grace_prefix.max(pending_grace_w);
                                } else {
                                    let mut prefix = if has_accid_val {
                                        self.accidental_prefix
                                    } else {
                                        0.0
                                    };
                                    if has_arpeggio {
                                        prefix += self.arpeggio_prefix_bonus;
                                    }
                                    prefix += pending_grace_w;
                                    entry.prefix = entry.prefix.max(prefix);
                                    if !n.is_chord {
                                        entry.grace_prefix =
                                            entry.grace_prefix.max(pending_grace_w);
                                    }

                                    let suffix_w = n.dot_count as f32 * self.dot_suffix_width
                                        + (if n.dot_count > 0 {
                                            self.dot_suffix_flat_bonus
                                        } else {
                                            0.0
                                        })
                                        + self.estimate_flag_suffix_width(n);
                                    entry.suffix = entry.suffix.max(suffix_w);

                                    for lyric in &n.lyrics {
                                        entry.lyric_width =
                                            entry.lyric_width.max(self.estimate_lyric_width(lyric));
                                    }

                                    if !n.is_chord {
                                        pending_grace_w = 0.0;
                                        p_chord_time = p_time;
                                        p_time += (n.duration as f64 * 10080.0 / divisions as f64)
                                            .round()
                                            as i32;
                                    }
                                }
                            }
                            crate::models::MeasureElement::Forward(d) => {
                                onset_needs
                                    .entry(p_time)
                                    .or_insert_with(crate::renderer::types::OnsetNeeds::default);
                                p_time += (*d as f64 * 10080.0 / divisions as f64).round() as i32;
                                p_chord_time = p_time;
                            }
                            crate::models::MeasureElement::Backup(d) => {
                                p_time -= (*d as f64 * 10080.0 / divisions as f64).round() as i32;
                                p_time = p_time.max(0);
                                p_chord_time = p_time;
                            }
                            crate::models::MeasureElement::Attributes(attr) => {
                                if p_time > 0 {
                                    let (_, consumed_x) = self.draw_attributes(
                                        svg::Document::new(),
                                        attr,
                                        0.0,
                                        0.0,
                                        &current_clefs_sim,
                                        num_staves_sim,
                                        staff_dist,
                                        &HashMap::new(),
                                        false,
                                    );
                                    if consumed_x > 0.0 {
                                        let entry = onset_needs.entry(p_time).or_insert_with(
                                            crate::renderer::types::OnsetNeeds::default,
                                        );
                                        entry.prefix = entry
                                            .prefix
                                            .max(consumed_x + self.attribute_prefix_bonus);
                                    }
                                }
                                if let Some(d) = attr.divisions {
                                    divisions = d;
                                    current_divisions.insert(p_id.clone(), d);
                                }
                                if let Some(st) = attr.staves {
                                    num_staves_sim = st;
                                }
                                for c in &attr.clefs {
                                    current_clefs_sim.insert(c.number, c.clone());
                                    global_clefs
                                        .get_mut(p_id)
                                        .unwrap()
                                        .insert(c.number, c.clone());
                                }
                                if let Some(k) = &attr.key {
                                    global_key.insert(p_id.clone(), k.clone());
                                }
                                if let Some(t) = &attr.time {
                                    global_time.insert(p_id.clone(), t.clone());
                                }
                            }
                            crate::models::MeasureElement::Direction(dir) => {
                                let is_below = dir.placement.as_deref() == Some("below");
                                let staff =
                                    dir.staff
                                        .unwrap_or(if is_below { num_staves_sim } else { 1 });
                                let entry = onset_needs
                                    .entry(p_time)
                                    .or_insert_with(crate::renderer::types::OnsetNeeds::default);
                                let extents =
                                    entry.dir_extents.entry((staff, is_below)).or_default();

                                for dtype in &dir.types {
                                    match dtype {
                                        crate::models::DirectionType::Dynamics(dyns) => {
                                            let w = self.estimate_dynamics_width(dyns);
                                            extents.left_width = extents.left_width.max(w / 2.0);
                                            extents.right_width = extents.right_width.max(w / 2.0);
                                        }
                                        crate::models::DirectionType::Words(text_str) => {
                                            let w = text_str.len() as f32 * 7.0 + 5.0;
                                            extents.right_width = extents.right_width.max(w);
                                        }
                                        crate::models::DirectionType::Rehearsal(text_str) => {
                                            let w = text_str.len() as f32 * 7.0 + 8.0;
                                            extents.left_width = extents.left_width.max(w / 2.0);
                                            extents.right_width = extents.right_width.max(w / 2.0);
                                        }
                                        crate::models::DirectionType::Metronome(_) => {
                                            let w = 50.0;
                                            extents.right_width = extents.right_width.max(w);
                                        }
                                        crate::models::DirectionType::Coda
                                        | crate::models::DirectionType::Segno => {
                                            let w = 20.0;
                                            extents.left_width = extents.left_width.max(w / 2.0);
                                            extents.right_width = extents.right_width.max(w / 2.0);
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            _ => {}
                        }
                    }

                    let mut part_measure_dur = p_time;
                    if let Some(attr) = &measure.attributes {
                        let (_, consumed_x) = self.draw_attributes(
                            svg::Document::new(),
                            attr,
                            0.0,
                            0.0,
                            &HashMap::new(),
                            1,
                            staff_dist,
                            &HashMap::new(),
                            false,
                        );
                        initial_attr_w = initial_attr_w.max(consumed_x);
                        if let Some(time) = &attr.time {
                            if let Ok(b) =
                                time.beats.split('+').next().unwrap_or("4").parse::<i32>()
                            {
                                let theoretical_dur =
                                    (b as f64 * (4.0 / time.beat_type as f64) * 10080.0).round()
                                        as i32;
                                max_theoretical_dur = max_theoretical_dur.max(theoretical_dur);
                                if !((m_idx == 0 || measure.implicit)
                                    && part_measure_dur < theoretical_dur)
                                {
                                    part_measure_dur = part_measure_dur.max(theoretical_dur);
                                }
                            }
                        }
                    }
                    max_dur = max_dur.max(part_measure_dur);
                }
            }

            if skip_remaining > 0 && !is_start_of_multirest {
                result.raw_widths[m_idx] = 0.0;
                result
                    .spacings
                    .push(crate::renderer::types::MeasureSpacing {
                        time_to_x: HashMap::new(),
                        grace_widths: HashMap::new(),
                        _total_content_width: 0.0,
                    });
                skip_remaining -= 1;
                continue;
            }

            if is_start_of_multirest {
                result
                    .spacings
                    .push(crate::renderer::types::MeasureSpacing {
                        time_to_x: HashMap::new(),
                        grace_widths: HashMap::new(),
                        _total_content_width: self.measure_width,
                    });
                continue;
            }

            result.total_durs[m_idx] = max_dur;
            result.theoretical_durs[m_idx] = max_theoretical_dur.max(max_dur);

            let mut current_x: f32 = 0.0;
            let mut time_to_x = HashMap::new();
            let mut grace_widths = HashMap::new();
            let onsets: Vec<_> = onset_needs.into_iter().collect();

            let mut last_dir_x: HashMap<(i32, bool), (f32, f32)> = HashMap::new();

            for i in 0..onsets.len() {
                let (t, needs) = &onsets[i];

                let mut required_x: f32 = current_x;
                for (&(staff, is_below), ext) in &needs.dir_extents {
                    if ext.left_width > 0.0 || ext.right_width > 0.0 {
                        if let Some(&(prev_center, prev_right)) = last_dir_x.get(&(staff, is_below))
                        {
                            let min_center = prev_center
                                + prev_right
                                + ext.left_width
                                + self.direction_clearance;
                            let req_start = min_center - needs.prefix;
                            required_x = required_x.max(req_start);
                        }
                    }
                }
                current_x = required_x;

                current_x += needs.prefix;
                let center_x = current_x;
                time_to_x.insert(*t, current_x);
                grace_widths.insert(*t, needs.grace_prefix);

                for (&(staff, is_below), ext) in &needs.dir_extents {
                    if ext.left_width > 0.0 || ext.right_width > 0.0 {
                        last_dir_x.insert((staff, is_below), (center_x, ext.right_width));
                    }
                }

                current_x += self.note_head_width; // Body
                current_x += needs.suffix;

                let dur_to_next = if i + 1 < onsets.len() {
                    (onsets[i + 1].0 - *t) as f32
                } else {
                    (max_dur - *t).max(0) as f32
                };

                let gap = match self.spacing_strategy {
                    crate::renderer::SpacingStrategy::Elastic => {
                        (16.0 * (dur_to_next / 10080.0).powf(0.6)).max(1.5)
                    }
                    crate::renderer::SpacingStrategy::Compact => {
                        if i + 1 < onsets.len() {
                            let next_needs = &onsets[i + 1].1;
                            let lyric_gap = ((needs.lyric_width + next_needs.lyric_width) * 0.5
                                - 12.0)
                                .max(0.0);
                            12.0 + lyric_gap
                        } else {
                            if needs.lyric_width > 0.0 { 4.0 } else { 0.0 }
                        }
                    }
                    crate::renderer::SpacingStrategy::Mobile => {
                        if i + 1 < onsets.len() {
                            let next_needs = &onsets[i + 1].1;
                            let lyric_gap =
                                ((needs.lyric_width + next_needs.lyric_width) * 0.5 - 8.0).max(0.0);
                            self.mobile_onset_gap + lyric_gap
                        } else {
                            if needs.lyric_width > 0.0 { 3.0 } else { 0.0 }
                        }
                    }
                };

                if i + 1 < onsets.len()
                    || self.spacing_strategy == crate::renderer::SpacingStrategy::Elastic
                {
                    current_x += gap;
                }
            }
            let total_content_w = current_x + self.measure_end_padding;
            let mut time_to_ratio = HashMap::new();
            for (t, x_val) in time_to_x {
                time_to_ratio.insert(t, x_val / total_content_w.max(1.0));
            }
            result.raw_widths[m_idx] =
                (self.measure_start_padding + initial_attr_w + total_content_w)
                    .max(self.measure_width);
            result.system_start_extra_widths[m_idx] =
                (system_start_attr_w - initial_attr_w).max(0.0);
            result
                .spacings
                .push(crate::renderer::types::MeasureSpacing {
                    time_to_x: time_to_ratio,
                    grace_widths,
                    _total_content_width: total_content_w,
                });
        }

        result.divisions_per_part = current_divisions;
        result.max_measures = max_measures;
        result
    }
}

pub(crate) struct MeasureAnalysisResult {
    pub max_measures: usize,
    pub raw_widths: Vec<f32>,
    pub system_start_extra_widths: Vec<f32>,
    pub spacings: Vec<crate::renderer::types::MeasureSpacing>,
    pub total_durs: Vec<i32>,
    pub theoretical_durs: Vec<i32>,
    pub divisions_per_part: HashMap<String, i32>,
}
