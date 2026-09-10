use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct Score {
    pub version: Option<String>,
    pub title: Option<String>,
    pub creator: Option<String>,
    pub concert_score: bool,
    pub part_list: Vec<PartListItem>,
    pub parts: Vec<Part>,
}

impl Score {
    pub fn filter_parts(&mut self, target_ids: &[String]) {
        self.parts.retain(|p| target_ids.contains(&p.id));
        self.part_list.retain(|item| match item {
            PartListItem::Part { id, .. } => target_ids.contains(id),
            PartListItem::Group(_) => true,
        });
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(untagged)]
pub enum PartListItem {
    Part {
        id: String,
        name: Option<String>,
        abbreviation: Option<String>,
        #[serde(skip_serializing_if = "Vec::is_empty", default)]
        instrument_names: Vec<String>,
        #[serde(skip_serializing_if = "Vec::is_empty", default)]
        part_links: Vec<PartLink>,
        #[serde(skip_serializing_if = "Option::is_none")]
        name_display: Option<NameDisplay>,
        #[serde(skip_serializing_if = "Option::is_none")]
        abbreviation_display: Option<NameDisplay>,
        #[serde(skip_serializing_if = "Option::is_none")]
        instrument_sound: Option<String>,
        #[serde(skip_serializing_if = "Vec::is_empty", default)]
        midi_instruments: Vec<MidiInstrument>,
    },
    Group(PartGroup),
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct MidiInstrument {
    pub id: String,
    pub channel: Option<i32>,
    pub program: Option<i32>,
    pub volume: Option<f32>,
    pub pan: Option<f32>,
    pub elevation: Option<f32>,
    pub midi_unpitched: Option<i32>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct NameDisplay {
    pub texts: Vec<NameDisplayText>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum NameDisplayText {
    Display(String),
    Accidental(String),
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct PartLink {
    pub href: Option<String>,
    pub title: Option<String>,
    pub instrument_links: Vec<String>, // list of instrument-link IDs
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct PartGroup {
    pub number: i32,
    pub group_type: String, // "start" or "stop"
    pub name: Option<String>,
    pub abbreviation: Option<String>,
    pub symbol: Option<GroupSymbol>,
    pub barline: Option<String>, // "yes" or "no"
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum GroupSymbol {
    Brace,
    Bracket,
    Line,
    None,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct Part {
    pub id: String,
    pub measures: Vec<Measure>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct Measure {
    pub number: String,
    pub attributes: Option<Attributes>,
    pub elements: Vec<MeasureElement>,
    pub implicit: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum MeasureElement {
    Note(Note),
    Backup(i32),
    Forward(i32),
    Direction(Direction),
    Attributes(Attributes),
    Sound(Sound),
    Barline(Barline),
    Frame(Frame),
    Harmony(Harmony),
    Bookmark(String), // id
    FiguredBass(FiguredBass),
    Grouping(Grouping),
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct Grouping {
    pub grouping_type: String, // "start", "stop"
    pub number: Option<i32>,
    pub features: Vec<GroupingFeature>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct GroupingFeature {
    pub feature_type: String,
    pub text: String,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct FiguredBass {
    pub figures: Vec<Figure>,
    pub default_y: Option<f32>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct Figure {
    pub prefix: Option<String>,
    pub number: Option<String>,
    pub suffix: Option<String>,
    pub extend: Option<String>, // "start", "stop", "continue"
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct Frame {
    pub strings: i32,
    pub frets: i32,
    pub first_fret: Option<i32>,
    pub first_fret_text: Option<String>,
    pub notes: Vec<FrameNote>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct FrameNote {
    pub string: i32,
    pub fret: i32,
    pub fingering: Option<String>,
    pub barre: Option<String>, // "start" or "stop"
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct Barline {
    pub location: String,          // "right", "left", "middle"
    pub bar_style: Option<String>, // "regular", "dotted", "dashed", "heavy", "light-light", etc.
    pub repeat: Option<Repeat>,
    pub ending: Option<Ending>,
    pub fermata: Option<String>, // type: "upright", "inverted", etc.
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct Ending {
    pub number: String,
    pub ending_type: String, // "start", "stop", "discontinue"
    pub text: String,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct Repeat {
    pub direction: String, // "forward" or "backward"
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct Direction {
    pub placement: Option<String>,
    pub types: Vec<DirectionType>,
    pub staff: Option<i32>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum DirectionType {
    Words(String),
    Dynamics(Vec<String>),
    Metronome(MetronomeMark),
    Coda,
    Segno,
    Rehearsal(String),
    Bracket(BracketMark),
    Wedge(WedgeMark),
    OctaveShift(OctaveShiftMark),
    Pedal(PedalMark),
    Dashes(DashesMark),
    Damp,
    DampAll,
    Other(String),
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct DashesMark {
    pub dashes_type: String, // "start", "stop", "continue"
    pub number: Option<i32>,
    pub dash_length: Option<f32>,
    pub space_length: Option<f32>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct PedalMark {
    pub pedal_type: String, // "start", "stop", "change", "continue", "resume", "discontinue"
    pub line: bool,
    pub number: Option<i32>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct OctaveShiftMark {
    pub shift_type: String, // "up", "down", "stop", "continue"
    pub number: Option<i32>,
    pub size: i32, // e.g. 8, 15
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct WedgeMark {
    pub wedge_type: String, // "crescendo", "diminuendo", "stop", "continue"
    pub number: Option<i32>,
    pub spread: Option<f32>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct BracketMark {
    pub bracket_type: String,
    pub number: Option<i32>,
    pub line_end: Option<String>,
    pub line_type: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct MetronomeMark {
    pub beat_unit: String,
    pub beat_unit_dot: i32,
    pub bpm: Option<String>,
    pub to_beat_unit: Option<String>,
    pub to_beat_unit_dot: i32,
    pub parentheses: bool,
    pub tied_unit: Option<Box<MetronomeMark>>,
    pub metronome_notes: Vec<MetronomeNote>,
    pub relation: Option<String>, // "equals", etc.
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct MetronomeNote {
    pub beat_unit: String,
    pub dots: i32,
    pub beams: Vec<Beam>,
    pub tuplet: Option<MetronomeTuplet>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct MetronomeTuplet {
    pub tuplet_type: String,         // "start", "stop"
    pub bracket: Option<String>,     // "yes", "no"
    pub show_number: Option<String>, // "actual", "both", "none"
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Default)]
pub enum PartSymbol {
    Brace,
    Bracket,
    Line,
    #[default]
    None,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct Transpose {
    pub diatonic: Option<i32>,
    pub chromatic: Option<i32>,
    pub octave_change: Option<i32>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct Attributes {
    pub divisions: Option<i32>,
    pub key: Option<Key>,
    pub transpose: Option<Transpose>,
    pub time: Option<Time>,
    pub clefs: Vec<Clef>,
    pub staves: Option<i32>,
    pub part_symbol: Option<PartSymbolMark>,
    pub measure_repeat: Option<MeasureRepeat>,
    pub beat_repeat: Option<BeatRepeat>,
    pub slash: Option<SlashMark>,
    pub multiple_rest: Option<i32>,
    pub staff_details: Vec<StaffDetails>,
    pub capo: Option<i32>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct StaffDetails {
    pub number: i32,
    pub staff_lines: Option<i32>,
    pub staff_tunings: Vec<StaffTuning>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct StaffTuning {
    pub line: i32,
    pub step: String,
    pub alter: Option<f32>,
    pub octave: i32,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct SlashMark {
    pub slash_type: String, // "start", "stop"
    pub use_stems: Option<bool>,
    pub note_type: Option<String>,
    pub dots: i32,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct BeatRepeat {
    pub repeat_type: String, // "start", "stop"
    pub slashes: i32,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct PartSymbolMark {
    pub symbol: PartSymbol,
    pub top_staff: Option<i32>,
    pub bottom_staff: Option<i32>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct MeasureRepeat {
    pub repeat_type: String, // "start", "stop"
    pub count: i32,          // number of measures to repeat (usually 1 or 2)
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct Key {
    pub fifths: i32,
    pub mode: Option<String>,
    pub key_accidentals: Vec<KeyAccidental>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct KeyAccidental {
    pub step: String,
    pub alter: f32,
    pub octaves: Vec<KeyOctave>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct KeyOctave {
    pub number: Option<i32>,
    pub value: i32,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct Time {
    pub beats: String,
    pub beat_type: i32,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct Clef {
    pub number: i32,
    pub sign: String,
    pub line: Option<i32>,
    pub clef_octave_change: Option<i32>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Beam {
    pub number: i32,
    pub value: BeamValue,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub enum BeamValue {
    Begin,
    Continue,
    End,
    ForwardHook,
    BackwardHook,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct TimeModification {
    pub actual_notes: i32,
    pub normal_notes: i32,
    pub normal_type: Option<String>,
    pub normal_dot_count: i32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AccidentalMark {
    pub value: String,
    pub placement: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum Ornament {
    TrillMark,
    Turn,
    DelayedTurn,
    InvertedTurn,
    DelayedInvertedTurn,
    VerticalTurn,
    InvertedVerticalTurn,
    Mordent {
        long: bool,
    },
    InvertedMordent {
        long: bool,
    },
    Haydn,
    Schleifer {
        placement: Option<String>,
    },
    Shake {
        placement: Option<String>,
    },
    Tremolo {
        tremolo_type: String,
        bars: i32,
    },
    WavyLine {
        wavy_type: String,
        number: i32,
        relative_x: Option<f32>,
    },
    AccidentalMark(AccidentalMark),
    Other(String),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Arrow {
    pub direction: String,
    pub style: Option<String>,
    pub placement: Option<String>,
    pub has_arrowhead: bool,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct Harmonic {
    pub is_artificial: bool,
    pub is_natural: bool,
    pub pitch_type: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Notehead {
    pub value: String,
    pub filled: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum TechnicalMark {
    Arrow(Arrow),
    Harmonic(Harmonic),
    BrassBend {
        placement: Option<String>,
    },
    DoubleTongue {
        placement: Option<String>,
    },
    TripleTongue {
        placement: Option<String>,
    },
    DownBow {
        placement: Option<String>,
    },
    UpBow {
        placement: Option<String>,
    },
    Fingering {
        text: String,
        placement: Option<String>,
    },
    Fingernails {
        placement: Option<String>,
    },
    Flip {
        placement: Option<String>,
    },
    Golpe {
        placement: Option<String>,
    },
    HalfMuted {
        placement: Option<String>,
    },
    Handbell {
        value: String,
        placement: Option<String>,
    },
    HarmonMute {
        closed: Option<String>,
        placement: Option<String>,
    },
    Heel {
        placement: Option<String>,
        substitution: Option<bool>,
    },
    Toe {
        placement: Option<String>,
        substitution: Option<bool>,
    },
    Hole {
        content: String,
        placement: Option<String>,
    },
    Open {
        placement: Option<String>,
    },
    OpenString {
        placement: Option<String>,
    },
    Pluck {
        text: String,
        placement: Option<String>,
        default_x: Option<f32>,
        default_y: Option<f32>,
    },
    Smear {
        placement: Option<String>,
    },
    Pizzicato {
        placement: Option<String>,
    },
    SnapPizzicato {
        placement: Option<String>,
    },
    Fret(i32),
    String(i32),
    HammerOn {
        number: i32,
        mark_type: String,
        text: String,
    },
    PullOff {
        number: i32,
        mark_type: String,
        text: String,
    },
    Tap {
        hand: Option<String>,
        placement: Option<String>,
    },
    ThumbPosition {
        placement: Option<String>,
    },
    Stopped {
        placement: Option<String>,
    },
    Bend(Vec<BendMark>),
    OtherTechnical {
        text: String,
        placement: Option<String>,
    },
    Other(String),
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct BendMark {
    pub bend_alter: f32,
    pub pre_bend: bool,
    pub release: bool,
    pub with_bar: Option<WithBar>,
    pub placement: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct WithBar {
    pub value: String, // "dip", "whammy"
    pub placement: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum Notation {
    Slur {
        number: i32,
        note_type: String,
        placement: Option<String>,
    },
    Tied {
        note_type: String,
    },
    Articulation {
        name: String,
        placement: Option<String>,
        default_x: Option<f32>,
        default_y: Option<f32>,
    },
    Tuplet {
        number: Option<i32>,
        note_type: String,
        bracket: Option<String>,
        placement: Option<String>,
        show_number: Option<String>,
        actual_notes: Option<i32>,
        normal_notes: Option<i32>,
    },
    Fermata {
        note_type: Option<String>,
        placement: Option<String>,
    },
    AccidentalMark(AccidentalMark),
    Ornaments(Vec<Ornament>),
    Arpeggiate {
        number: Option<i32>,
        direction: Option<String>,
    },
    NonArpeggiate {
        number: Option<i32>,
        non_arp_type: String,
    },
    Technical(Vec<TechnicalMark>),
    Glissando(GlissandoMark),
    Slide(SlideMark),
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct SlideMark {
    pub slide_type: String, // "start", "stop"
    pub number: i32,
    pub line_type: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct GlissandoMark {
    pub gliss_type: String, // "start", "stop"
    pub number: i32,
    pub line_type: Option<String>, // "wavy", "solid"
    pub text: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct Harmony {
    pub root_step: String,
    pub root_alter: Option<f32>,
    pub kind: String,
    pub kind_text: Option<String>,
    pub use_symbols: bool,
    pub bass_step: Option<String>,
    pub bass_alter: Option<f32>,
    pub bass_separator: Option<String>,
    pub degrees: Vec<Degree>,
    pub frame: Option<Frame>,
    pub numeral: Option<Numeral>,
    pub inversion: Option<i32>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct Numeral {
    pub root_value: i32,
    pub root_text: Option<String>,
    pub root_alter: Option<NumeralAlter>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct NumeralAlter {
    pub value: f32,
    pub location: Option<String>, // "left", "right"
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct Degree {
    pub value: i32,
    pub alter: f32,
    pub degree_type: String, // "add", "alter", "subtract"
    pub type_text: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct Lyric {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub number: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub syllabic: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extend: Option<String>, // "start", "stop", "continue"
    #[serde(skip_serializing_if = "is_false")]
    pub end_line: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct Grace {
    pub slash: Option<String>, // "yes", "no"
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Note {
    pub pitch: Option<Pitch>,
    pub unpitched: Option<Unpitched>,
    pub duration: i32,
    pub voice: Option<i32>,
    pub staff: Option<i32>,
    pub stem: Option<String>,
    pub note_type: Option<String>,
    pub notehead: Option<Notehead>,
    pub rest: bool,
    pub rest_measure: bool,
    pub is_chord: bool,
    pub is_cue: bool,
    pub grace: Option<Grace>,
    pub dot_count: i32,
    pub lyrics: Vec<Lyric>,
    pub beams: Vec<Beam>,
    pub notations: Vec<Notation>,
    pub accidental: Option<String>,
    pub time_modification: Option<TimeModification>,
    pub print_object: Option<bool>,
    /// `print-dot="no"` on the `<note>` element: the note still carries its
    /// augmentation dot for duration purposes, but the dot glyph itself
    /// should not be drawn (common on tablature, where rhythm is shown via
    /// stems/beams above the staff instead).
    pub print_dot: Option<bool>,
    pub harmonies: Vec<Harmony>,
    pub instrument: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Pitch {
    pub step: String,
    pub octave: i32,
    pub alter: Option<f32>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Unpitched {
    pub display_step: String,
    pub display_octave: i32,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub midi_number: Option<u8>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct SystemBoundary {
    pub y_start: f32,
    pub height: f32,
    pub measure_start: usize,
    pub measure_end: usize,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct PlayMetadata {
    pub beats: Vec<BeatEvent>,
    pub system_boundaries: Vec<SystemBoundary>,
}

#[derive(Debug, Clone)]
pub struct TimingEvent {
    pub measure_index: usize,
    pub measure_number: String,
    pub beat_number: f32,   // 1.0, 2.0...
    pub absolute_beat: f32, // accumulated total beats
    pub time_seconds: f32,  // absolute time in seconds
    pub tick_offset: i32,   // ticks from measure start (normalized to 10080)
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct BeatEvent {
    pub x: f32,
    pub y_start: f32,
    pub y_end: f32,
    pub measure_index: usize,
    pub measure_number: String,
    pub beat_number: f32,   // e.g., 1.0, 2.0, 3.0
    pub absolute_beat: f32, // chronological beat counter
    pub time_seconds: f32,
    pub system_index: usize,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct Sound {
    pub tempo: Option<f32>,
    pub dalsegno: Option<String>,
    pub segno: Option<String>,
    pub coda: Option<String>,
    pub tocoda: Option<String>,
}
