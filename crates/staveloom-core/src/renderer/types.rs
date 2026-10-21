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
    /// Staff of the start note — notes in the same staff are recorded as
    /// obstacles the slur curve must clear.
    pub staff: i32,
    /// Extreme points (x, y) of the notes spanned by the slur (noteheads and
    /// stem tips on the slur's side), in the current system's coordinates.
    pub obstacles: Vec<(f32, f32)>,
    /// True when the start point is attached to the start note's stem tip
    /// (rather than its notehead), so it must follow the stem if a beam later
    /// lengthens or shortens it.
    pub stem_anchor: bool,
    /// Set when this entry is not an open slur but a "stop" that appeared in
    /// the file before its matching "start" (common when a slur runs from a
    /// lower staff to an upper one: the upper staff's notes come first in the
    /// measure). Holds the stop note's endpoint for either placement; it is
    /// paired when the start shows up later in the same measure.
    pub pending_stop: Option<PendingSlurStop>,
    /// Set when the slur's end is already known (its stop was read before its
    /// start): the slur stays open only to collect obstacles and is drawn at
    /// the end of the measure as (end x, end y, end staff, end note top,
    /// end note bottom).
    pub known_end: Option<(f32, f32, i32, f32, f32)>,
    /// The start point is the beginning of a system, the slur continuing
    /// from the previous one: it isn't attached to a note, so it may move to
    /// clear this system's first notes.
    pub resumed: bool,
}

/// Endpoint candidates of a slur stop seen before its start.
#[derive(Clone, Copy)]
pub(crate) struct PendingSlurStop {
    pub above: (f32, f32),
    pub below: (f32, f32),
    /// Staff and vertical extent (top, bottom) of the stop note.
    pub staff: i32,
    pub top: f32,
    pub bottom: f32,
}

/// Key offset under which pending slur stops are stored in the active-slur
/// map, so they never collide with an open slur of the same number.
pub(crate) const PENDING_SLUR_STOP_KEY: i32 = 1 << 20;

/// Key offset for slurs whose end is already known (see
/// `SlurStartInfo::known_end`).
pub(crate) const KNOWN_SLUR_END_KEY: i32 = 1 << 21;

/// How a note's voice shares its staff, for placing a slur that starts on it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum VoiceSharing {
    /// No other voice from here to the end of the measure: notehead side.
    Alone,
    /// Another voice sounds with this note: stem side, away from it.
    Now,
    /// Another voice only enters later in the measure (the slur's start note
    /// doesn't show which side it is on): the lowest-numbered voice goes
    /// above, the others below, as MuseScore places them.
    Later { upper: bool },
}

/// Final geometry of one beamed stem, reported by `draw_beam_group` so slurs
/// attached to that stem can be moved to clear the beam.
#[derive(Clone, Copy)]
pub(crate) struct BeamedStemTip {
    /// Stem x as originally drawn with the note (before beam-direction fix-up).
    pub orig_x: f32,
    /// Final stem x.
    pub x: f32,
    /// Final stem tip y (where the stem meets the primary beam).
    pub tip_y: f32,
    pub is_up: bool,
    /// Primary beam thickness; for down stems the beam lies below `tip_y`.
    pub beam_thickness: f32,
    /// Staff of the stem's note.
    pub staff: i32,
    /// The beam joins notes on two staves.
    pub cross_staff: bool,
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
    pub is_above: bool,
    pub number: i32,
    pub level: i32,
    pub bracket: bool,
    pub show_number: bool,
    /// Staff and vertical extent (top, bottom) of the start note.
    pub staff: i32,
    pub top: f32,
    pub bottom: f32,
}

/// A finished tuplet waiting to be drawn at the end of its measure, once
/// every note under it (and its beam) has its final position.
#[derive(Clone)]
pub(crate) struct PendingTuplet {
    pub start: TupletStartInfo,
    pub x2: f32,
    /// Vertical extent (top, bottom) of the stop note.
    pub top2: f32,
    pub bottom2: f32,
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
    /// A note at this onset sits on ledger lines (wider than its notehead).
    pub ledger: bool,
}

pub(crate) struct MeasureSpacing {
    pub time_to_x: HashMap<i32, f32>,
    pub grace_widths: HashMap<i32, f32>,
    pub _total_content_width: f32,
}

/// Per-measure voice layout facts shared by the layout analysis and renderer.
#[derive(Default)]
pub(crate) struct MeasureVoiceInfo {
    /// Time spans (voice, start, end) of every visible note/rest on each
    /// staff, used to tell whether a rest actually shares its beat with
    /// another voice.
    pub voice_spans: HashMap<i32, Vec<(i32, i32, i32)>>,
    /// Horizontal displacement, in notehead widths, for chord groups keyed by
    /// (staff, time position, voice) whose noteheads would overlap another
    /// voice's (unison or second) at the same onset. Equal-duration voices
    /// split around the beat (-0.5 / +0.5) so their stems meet on the beat
    /// line; otherwise the higher-numbered voice moves right by +1.0.
    pub offsets: HashMap<(i32, i32, i32), f32>,
    /// Onsets (time position, explicit stem-down) where a chord within one
    /// voice has noteheads a second apart, so one of them sits on the far side
    /// of the stem: left for stem-down, right for stem-up. `None` (no `<stem>`;
    /// direction decided later from pitch) reserves room on both sides.
    pub chord_seconds: Vec<(i32, Option<bool>)>,
}
