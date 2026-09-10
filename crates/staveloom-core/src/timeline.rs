use crate::models::{DirectionType, MeasureElement, Score};
use std::collections::HashMap;

pub struct TimelineSolver;

impl TimelineSolver {
    pub fn solve(score: &Score) -> Vec<usize> {
        let mut timeline = Vec::new();
        let measure_count = score.parts.first().map(|p| p.measures.len()).unwrap_or(0);
        if measure_count == 0 {
            return timeline;
        }

        let mut current_idx = 0;
        let mut repeat_starts = Vec::new();
        let mut repeat_counts: HashMap<usize, usize> = HashMap::new();
        let mut coda_target: Option<usize> = None;
        let mut segno_pos: Option<usize> = None;
        let mut has_jumped = false;

        // Pre-scan for Segno, Coda, and Max Ending Numbers per repeat section
        let mut max_endings_for_repeat: HashMap<usize, i32> = HashMap::new();
        let mut current_active_start = 0;
        for i in 0..measure_count {
            if let Some(m) = score.parts.first().and_then(|p| p.measures.get(i)) {
                for el in &m.elements {
                    match el {
                        MeasureElement::Barline(bl) => {
                            if let Some(repeat) = &bl.repeat {
                                if repeat.direction == "forward" {
                                    current_active_start = i;
                                }
                            }
                            if let Some(ending) = &bl.ending {
                                let numbers: Vec<i32> = ending
                                    .number
                                    .split(',')
                                    .map(|s| s.trim().parse::<i32>().unwrap_or(0))
                                    .collect();
                                let max_num = numbers.into_iter().max().unwrap_or(0);
                                let entry = max_endings_for_repeat
                                    .entry(current_active_start)
                                    .or_insert(1);
                                if max_num > *entry {
                                    *entry = max_num;
                                }
                            }
                        }
                        MeasureElement::Direction(dir) => {
                            for dir_type in &dir.types {
                                match dir_type {
                                    DirectionType::Segno => segno_pos = Some(i),
                                    DirectionType::Coda => coda_target = Some(i),
                                    _ => {}
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        while current_idx < measure_count {
            let measure = &score.parts[0].measures[current_idx];

            // 1. Volta Ending Logic
            let mut skip_measure = false;
            for el in &measure.elements {
                if let MeasureElement::Barline(bl) = el {
                    if let Some(ending) = &bl.ending {
                        if ending.ending_type == "start" {
                            let repeat_start = repeat_starts.last().cloned().unwrap_or(0);
                            let current_iter =
                                (*repeat_counts.get(&repeat_start).unwrap_or(&0) + 1) as i32;

                            let numbers: Vec<i32> = ending
                                .number
                                .split(',')
                                .map(|s| s.trim().parse::<i32>().unwrap_or(0))
                                .collect();

                            if !numbers.contains(&current_iter) {
                                skip_measure = true;
                                break;
                            }
                        }
                    }
                }
            }

            if skip_measure {
                let mut found_next = false;
                for j in (current_idx + 1)..measure_count {
                    if let Some(m) = score.parts.first().and_then(|p| p.measures.get(j)) {
                        for el in &m.elements {
                            if let MeasureElement::Barline(bl) = el {
                                if let Some(ending) = &bl.ending {
                                    if ending.ending_type == "stop"
                                        || ending.ending_type == "discontinue"
                                    {
                                        current_idx = j + 1;
                                        found_next = true;
                                        break;
                                    } else if ending.ending_type == "start" {
                                        current_idx = j;
                                        found_next = true;
                                        break;
                                    }
                                }
                            }
                        }
                    }
                    if found_next {
                        break;
                    }
                }
                if !found_next {
                    current_idx += 1;
                }
                continue;
            }

            timeline.push(current_idx);

            let mut next_idx = current_idx + 1;
            let mut jump_taken = false;

            for el in &measure.elements {
                match el {
                    MeasureElement::Barline(bl) => {
                        if let Some(repeat) = &bl.repeat {
                            if repeat.direction == "forward" {
                                if !repeat_starts.contains(&current_idx) {
                                    repeat_starts.push(current_idx);
                                }
                            } else if repeat.direction == "backward" {
                                let start_idx = repeat_starts.last().cloned().unwrap_or(0);
                                let count = repeat_counts.entry(start_idx).or_insert(0);

                                // Determine how many times to repeat
                                // Default is 1 (play 2 times), but if there are volta endings,
                                // it should repeat until all endings are covered.
                                let max_needed =
                                    max_endings_for_repeat.get(&start_idx).cloned().unwrap_or(1)
                                        as usize;

                                if *count < max_needed {
                                    *count += 1;
                                    next_idx = start_idx;
                                    jump_taken = true;
                                }
                            }
                        }
                    }
                    MeasureElement::Direction(dir) => {
                        for dir_type in &dir.types {
                            match dir_type {
                                DirectionType::Coda => {
                                    if has_jumped && Some(current_idx) != coda_target {
                                        if let Some(pos) = coda_target {
                                            next_idx = pos;
                                            jump_taken = true;
                                        }
                                    }
                                }
                                DirectionType::Words(text) => {
                                    let text = text.to_lowercase();
                                    if (text.contains("d.s.") || text.contains("dal segno"))
                                        && !has_jumped
                                    {
                                        if let Some(pos) = segno_pos {
                                            next_idx = pos;
                                            has_jumped = true;
                                            jump_taken = true;
                                        }
                                    } else if (text.contains("d.c.") || text.contains("da capo"))
                                        && !has_jumped
                                    {
                                        next_idx = 0;
                                        has_jumped = true;
                                        jump_taken = true;
                                    } else if text.contains("to coda") && has_jumped {
                                        if let Some(pos) = coda_target {
                                            next_idx = pos;
                                            jump_taken = true;
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }
            }

            if jump_taken {
                current_idx = next_idx;
            } else {
                current_idx += 1;
            }

            if timeline.len() > measure_count * 20 {
                break;
            }
        }

        timeline
    }
}
