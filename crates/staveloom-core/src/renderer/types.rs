use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SpacingStrategy {
    Elastic,
    Compact,
    Mobile,
}

#[derive(Clone, Copy)]
pub(crate) struct StemInfo {
    pub x: f32,
    pub y_start: f32,
    pub y_tip: f32,
    pub is_up: bool,
    pub is_cue: bool,
    pub y_top: f32,
    pub y_bottom: f32,
    /// The `<staff>` number this note belongs to within its part (1 = top
    /// staff, e.g. treble clef of a piano grand staff; 2 = next staff down).
    /// Used to detect cross-staff beam groups.
    pub staff: i32,
}

#[derive(Clone)]
pub(crate) struct SlurStartInfo {
    pub x: f32,
    pub y: f32,
    pub is_above: bool,
    pub is_continuation: bool,
}

#[derive(Clone)]
pub(crate) struct TieStartInfo {
    pub x: f32,
    pub y: f32,
    pub is_above: bool,
    pub is_continuation: bool,
}

#[derive(Clone)]
pub(crate) struct TupletStartInfo {
    pub x: f32,
    pub y: f32,
    pub is_above: bool,
    pub number: i32,
    pub level: i32,
    pub bracket: bool,
    pub show_number: bool,
}

#[derive(Clone, Default, Debug)]
pub(crate) struct DirectionExtent {
    pub left_width: f32,
    pub right_width: f32,
}

#[derive(Default)]
pub(crate) struct OnsetNeeds {
    pub prefix: f32,
    pub suffix: f32,
    pub grace_prefix: f32,
    pub lyric_width: f32,
    pub dir_extents: HashMap<(i32, bool), DirectionExtent>, // (staff, is_below) -> extent
}

pub(crate) struct MeasureSpacing {
    pub time_to_x: HashMap<i32, f32>,
    pub grace_widths: HashMap<i32, f32>,
    pub _total_content_width: f32,
}
