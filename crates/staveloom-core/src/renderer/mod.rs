use crate::models::{
    Attributes, Barline, BeamValue, Clef, Direction, GroupSymbol, Key, MeasureElement, Notation,
    Note, PartGroup, PartListItem, PartSymbol, PlayMetadata, Score, Time,
};
use std::collections::{HashMap, HashSet};
use svg::Document;
use svg::node::Blob;
use svg::node::element::path::Data;
use svg::node::element::{Line, Path, Text};

mod layout;
mod types;
mod utils;

pub use types::SpacingStrategy;
use types::*;

/// Accidentals less than this many diatonic steps apart (a sixth) overlap and
/// need separate columns; shared by the layout and the drawing.
pub(crate) const ACCIDENTAL_COLUMN_STEPS: i32 = 5;

/// How far a tuplet bracket's hooks and number reach past its line (px).
const TUPLET_REACH: f32 = 8.0;

/// Room the layout keeps for a fermata past the staff's outer line (px):
/// process_fermatas draws the 13px glyph from 11px to 24px out, plus a margin.
const FERMATA_REACH: f32 = 35.0;

pub struct Renderer {
    /// 보표에서 인접한 두 줄 사이의 세로 간격(SVG px 단위). 렌더러가 글리프 크기를
    /// 산정하는 기준 단위이기도 해서(예: 노트헤드 폰트 크기는 `staff_line_distance * 4.0`,
    /// 임시표는 `* 3.0`), 이 값을 바꾸면 간격뿐 아니라 악보 전체의 글리프 크기가 함께
    /// 스케일된다. 파트별 보표 간격이 아직 계산되지 않은 곳에서는 기본값(fallback)으로도
    /// 쓰인다.
    pub staff_line_distance: f32,
    /// 페이지 왼쪽 여백(px). 이 값은 하한선(floor)으로만 동작한다 — 특정 시스템(줄)에
    /// 파트/그룹 이름 라벨을 그려야 할 때 실제 왼쪽 여백은
    /// `max(margin_left, label_width + 22.0)`로 계산되므로(`calculate_system_margin`
    /// 참고), 이름 라벨이 없는 시스템/스코어에서만 이 값이 여백을 그대로 결정한다.
    pub margin_left: f32,
    /// 페이지 오른쪽 여백(px). `margin_left`와 달리 라벨 유무와 무관하게 페이지/시스템
    /// 폭을 계산하는 모든 곳(`usable_width`, 캔버스 폭, 보표선 끝 위치)에 그대로
    /// 더해진다 — 라벨 때문에 넓어지는 일이 없다.
    pub margin_right: f32,
    /// 페이지 위쪽 여백(px). 페이지의 첫 번째 시스템이 시작되는 세로 위치(`current_y`)의
    /// 기준값이다.
    pub margin_top: f32,
    /// 같은 시스템 안에서 서로 다른 파트끼리(예: 바이올린과 첼로) 세로로 쌓일 때의
    /// 최소 간격(px). 이 값은 하한선일 뿐이며, 두 파트의 음표 범위·레저선·가사·
    /// 다이내믹 등이 겹칠 것 같으면 실제 간격은 이보다 더 벌어진다.
    pub part_distance: f32,
    /// 하나의 그랜드 스태프 악기(예: 피아노의 오른손/왼손 보표)를 이루는 두 보표
    /// 사이의 최소 세로 간격(px). `part_distance`와 동일하게 하한선 + 충돌 회피
    /// 로직이 적용되지만, 적용 범위가 한 파트 내부의 보표들로 한정된다.
    pub grand_staff_distance: f32,
    /// 마디 하나가 가질 수 있는 최소 폭(px, 하한선). 실제 마디 폭은 안에 들어있는
    /// 음표/쉼표 내용에 따라 정해지며, 이 값은 온쉼표 하나만 있는 마디처럼 내용이
    /// 거의 없어 폭이 지나치게 좁아질 수 있는 경우에만 하한으로 개입한다.
    pub measure_width: f32,
    /// 노트헤드 글리프가 차지한다고 가정하는 가로 폭(px). 온셋(onset) 사이 간격을
    /// 계산하는 스페이싱 수식에도 쓰이고, 스템 위치·임시표-노트헤드 간격·점(dot)
    /// 위치 등 실제 드로잉 오프셋에도 그대로 쓰인다. 따라서 `staff_line_distance`가
    /// 암시하는 실제 글리프 크기와 어긋나면 노트헤드/스템/임시표가 서로 겹치는
    /// 시각적 문제가 생기므로, 두 값은 항상 함께 맞춰야 한다.
    pub note_head_width: f32,
    /// 렌더링된 SVG의 `<style>` 블록에 그대로 삽입되는 CSS `font-family` 목록으로,
    /// 가사·다이내믹·템포/리허설 텍스트·파트 이름 등 일반 텍스트에 적용된다. 노트헤드,
    /// 음자리표, 임시표 같은 실제 음악 기보 글리프에는 영향을 주지 않는데, 이들은
    /// 항상 `estimate_*`/`draw_*` 메서드에 내장된 Bravura/SMuFL 글리프 수치로
    /// 그려지기 때문이다.
    pub font_family: String,
    /// 마디들을 시스템(줄) 단위로 줄바꿈할 때 기준이 되는 목표 폭(px). `None`이면
    /// 스코어 전체를 가로로 길게 스크롤하는 시스템 하나로 렌더링하고, `Some(w)`이면
    /// 마디를 그리디(greedy)하게 채워나가다가 한 시스템이 완성될 때마다 폭 `w`를
    /// 정확히 채우도록 다시 스케일한다.
    pub page_width: Option<f32>,
    /// 값이 설정되어 있으면 `part-list`의 id가 이 목록에 포함된 파트만 렌더링한다.
    /// 목록에 없는 파트는 단순히 숨기는 게 아니라 아예 렌더링 대상에서 제외된다.
    pub filter_parts: Option<Vec<String>>,
    /// true이면 일반적인 기보 외에 추가 디버그 주석(레이아웃 바운딩 박스, 메타데이터
    /// 마커 등)을 SVG 출력에 함께 그린다.
    pub debug_metadata: bool,
    /// 음표 온셋 사이의 가로 간격을 계산하는 방식을 선택한다(`Elastic` = 음표 길이에
    /// 비례, `Compact` = 고정 간격, `Mobile` = 좁은 화면용으로 더 좁힌 고정 간격).
    /// 이 값은 다른 곳의 모바일 전용 동작도 함께 켠다 — 예를 들어
    /// `calculate_system_margin`이나, 첫 시스템 이후 파트/그룹 이름 라벨을 생략하는
    /// 동작은 이 값이 `SpacingStrategy::Mobile`일 때만 작동한다.
    pub spacing_strategy: SpacingStrategy,
    /// 임시표가 붙은 음표 앞에 추가로 확보하는 가로 공간(px).
    pub accidental_prefix: f32,
    /// 아르페지오/논아르페지오 표시가 있는 음표 앞에 추가로 확보하는 가로 공간(px).
    /// 같은 음표에 임시표까지 있으면 `accidental_prefix`에 더해서 적용된다.
    pub arpeggio_prefix_bonus: f32,
    /// 노트헤드 뒤쪽에, 붙임점(augmentation dot) 하나당 확보하는 가로 공간(px)
    /// (음표의 점 개수만큼 곱해짐).
    pub dot_suffix_width: f32,
    /// 붙임점이 하나라도 있는 음표에 개수와 무관하게 한 번만 추가되는 고정 보너스
    /// 가로 공간(px).
    pub dot_suffix_flat_bonus: f32,
    /// 같은 보표/같은 배치(위/아래)에서 연속된 Direction 주석(다이내믹, 지시어,
    /// 리허설 마크 등) 사이에 강제되는 최소 가로 간격(px) — 텍스트끼리 겹치지
    /// 않도록 한다.
    pub direction_clearance: f32,
    /// 마디 중간에 clef/key/time 같은 속성(attributes) 변경이 있은 직후 음표 앞에
    /// 추가로 확보하는 가로 공간(px).
    pub attribute_prefix_bonus: f32,
    /// 모든 마디의 콘텐츠 폭 계산에서, 맨 앞(첫 음표 prefix보다도 앞)에 항상
    /// 더해지는 고정 가로 패딩(px).
    pub measure_start_padding: f32,
    /// 모든 마디의 콘텐츠 폭 계산에서, 맨 뒤(마지막 온셋의 suffix 다음)에 항상
    /// 더해지는 고정 가로 패딩(px).
    pub measure_end_padding: f32,
    /// 연속된 음표 온셋 사이의 기본 가로 간격(px)으로, `spacing_strategy`가
    /// `SpacingStrategy::Mobile`일 때만 사용된다(`Compact`의 고정 12.0 간격에
    /// 대응하는 `Mobile` 전용 값이며, 더 좁게 조정되어 있다).
    pub mobile_onset_gap: f32,
    /// Stem tips of beam groups drawn since the last chord group finished,
    /// consumed by `process_chord_group` to re-anchor slurs on beamed stems.
    beamed_stem_tips: std::cell::RefCell<Vec<BeamedStemTip>>,
    /// Vertical extent (min y, max y) of late-placed elements (slurs, pedal
    /// lines) drawn in the current system. Their positions are only known
    /// while drawing, after the system's height was laid out, so the system's
    /// slice is grown to fit them.
    drawn_extent: std::cell::Cell<Option<(f32, f32)>>,
    /// Dynamics drawn in the current system as (left x, right x, y, below,
    /// staff), so hairpins on the same line can be shortened to avoid them.
    placed_dynamics: std::cell::RefCell<Vec<(f32, f32, f32, bool, i32)>>,
    /// Hairpin segments drawn in the current system as (x1, x2, bottom y,
    /// top y), so pedal lines can be kept clear of them.
    placed_wedges: std::cell::RefCell<Vec<(f32, f32, f32, f32, f32)>>,
    /// Directions drawn in the current system, by staff and side.
    gap_items: std::cell::RefCell<Vec<GapItem>>,
    /// x range of the measure whose directions are being placed.
    measure_span: std::cell::Cell<(f32, f32)>,
    /// Index of that measure in its part.
    current_measure_idx: std::cell::Cell<usize>,
    /// How much the measure being drawn is squeezed (drawn / natural content
    /// width, at most 1), applied to grace-note offsets.
    grace_scale: std::cell::Cell<f32>,
    /// Grace notes drawn, as (measure index, part id, voice, onset, x), so the
    /// play cursor can pass over them while they sound (see `GraceMark`).
    grace_marks: std::cell::RefCell<Vec<GraceMark>>,
    /// Hairpins (part id -> (start measure index, number)) that would sit
    /// between two staves but cross a measure with a cross-staff beam, which
    /// fills that gap: they go outside the part on every system instead.
    outer_wedges: std::cell::RefCell<HashMap<String, HashSet<(usize, i32)>>>,
    current_part_id: std::cell::RefCell<String>,
    /// Notes (by address) starting a trill wavy line that never gets a stop
    /// (before the next start of its number or the end of the part): the
    /// line ends with the note itself.
    open_wavy_notes: std::cell::RefCell<HashSet<usize>>,
    /// Slur curves drawn in the current system as (x1, x2, top y, bottom y,
    /// higher endpoint y, lower endpoint y), so dynamics and hairpins can be
    /// kept clear of them and colliding slurs between staves detected.
    placed_slurs: std::cell::RefCell<Vec<PlacedSlur>>,
    /// Pedal line segments of the current system, drawn at the end of the
    /// system once every hairpin has its final position.
    deferred_pedals: std::cell::RefCell<Vec<DeferredPedal>>,
    /// Every note drawn so far in the current system, so a slur that ends on
    /// another staff than it started on can clear that staff's notes too —
    /// including ones read before the slur started (e.g. the upper staff's
    /// notes in the slur's first measure).
    placed_notes: std::cell::RefCell<Vec<PlacedNote>>,
    /// Tuplets finished in the current measure, drawn at its end.
    pending_tuplets: std::cell::RefCell<Vec<PendingTuplet>>,
    /// Accidentals drawn in the current system as (staff top y, x, y), so a
    /// second voice's accidentals at the same onset take the next column
    /// instead of printing over the first voice's.
    placed_accidentals: std::cell::RefCell<Vec<(f32, f32, f32)>>,
    /// Tuplet brackets drawn in the current system as (part top y, staff,
    /// x1, x2, top, bottom): hairpins and other directions stay clear of them.
    placed_tuplets: std::cell::RefCell<Vec<(f32, i32, f32, f32, f32, f32)>>,
    /// Fingerings drawn in the current system as (part top y, x, centre y):
    /// lines placed later over the notes (barre brackets) clear them.
    placed_fingerings: std::cell::RefCell<Vec<(f32, f32, f32)>>,
    /// Marks on the stem side of a beamed note (fingerings), held until the
    /// beam is drawn and its final stem tip is known.
    pending_beam_marks: std::cell::RefCell<Vec<PendingBeamMark>>,
    /// Direction texts (words, tempo marks, dynamics) placed in the current
    /// system as (x1, x2, top, bottom, below), so later ones can be moved
    /// clear of them.
    placed_text_boxes: std::cell::RefCell<Vec<(f32, f32, f32, f32, bool)>>,
    /// Top y of the part being drawn, tagging `placed_notes` with their part
    /// (staff numbers repeat across parts).
    current_part_y: std::cell::Cell<f32>,
}

/// A drawn note (chord) in the current system: staff, notehead x and
/// vertical extent, and its stem (x, tip y, is up) if any.
#[derive(Clone, Copy)]
struct PlacedNote {
    part_y: f32,
    staff: i32,
    x: f32,
    top: f32,
    bottom: f32,
    stem: Option<(f32, f32, bool)>,
    /// Its stem joins a beam spanning two staves, so its length follows the
    /// staff gap.
    cross_beam: bool,
}

/// A direction drawn in the current system (hairpin, dynamic, words, octave
/// line), with the staff it belongs to and its side, so those sitting between
/// two staves can be checked against the other staff.
#[derive(Clone, Copy)]
struct GapItem {
    part_y: f32,
    staff: i32,
    is_below: bool,
    x1: f32,
    x2: f32,
    top: f32,
    bottom: f32,
}

/// A grace note as drawn (see `Renderer::grace_marks`).
#[derive(Clone)]
struct GraceMark {
    measure_idx: usize,
    part_id: String,
    voice: i32,
    /// Onset it is written at (that of the note it precedes, or the measure end).
    time: i32,
    x: f32,
}

/// A slur curve drawn in the current system (see `Renderer::placed_slurs`):
/// a cubic Bézier from (x1, y1) to (x2, y2) whose two control points share
/// height `cy`, plus its precomputed vertical extent.
#[derive(Clone, Copy)]
struct PlacedSlur {
    x1: f32,
    y1: f32,
    cx1: f32,
    cx2: f32,
    cy1: f32,
    cy2: f32,
    x2: f32,
    y2: f32,
    top: f32,
    bottom: f32,
    /// Ends on a different staff than it starts: one end moves with each
    /// staff, so widening the gap between them only stretches it.
    cross_staff: bool,
    /// Staff of its start note, and the top of its part.
    staff: i32,
    part_y: f32,
}

impl PlacedSlur {
    fn x_range(&self) -> (f32, f32) {
        (self.x1.min(self.x2), self.x1.max(self.x2))
    }

    /// Curve height at `x` (x(t) is monotonic, so solve it by bisection).
    fn y_at(&self, x: f32) -> f32 {
        let (mut lo, mut hi) = (0.0_f32, 1.0_f32);
        let x_of = |t: f32| {
            let u = 1.0 - t;
            u * u * u * self.x1 + 3.0 * u * u * t * self.cx1 + 3.0 * u * t * t * self.cx2
                + t * t * t * self.x2
        };
        let increasing = self.x2 >= self.x1;
        for _ in 0..24 {
            let t = 0.5 * (lo + hi);
            if (x_of(t) < x) == increasing {
                lo = t;
            } else {
                hi = t;
            }
        }
        let t = 0.5 * (lo + hi);
        let u = 1.0 - t;
        u * u * u * self.y1 + 3.0 * u * u * t * self.cy1 + 3.0 * u * t * t * self.cy2
            + t * t * t * self.y2
    }
}

/// A pedal line segment waiting to be drawn (see `Renderer::deferred_pedals`).
#[derive(Clone)]
struct DeferredPedal {
    x1: f32,
    x2: f32,
    y: f32,
    is_initial: bool,
    is_change: bool,
    is_terminal: bool,
    glyph: String,
    is_below: bool,
    /// Top of the part the pedal belongs to.
    part_y: f32,
}

impl Default for Renderer {
    fn default() -> Self {
        Self {
            staff_line_distance: 10.0,  // px, 보표 줄 간격 — 글리프 크기의 기준 단위
            margin_left: 40.0,          // px, 하한선 — 파트/그룹 이름 라벨 폭만큼 더 늘어남
            margin_right: 20.0,         // px, 고정값 — 라벨 때문에 늘어나지 않음
            margin_top: 60.0,           // px, 첫 시스템의 위쪽 시작 오프셋
            part_distance: 80.0,        // px, 서로 다른 파트 간 최소 간격(하한선)
            grand_staff_distance: 80.0, // px, 한 그랜드 스태프 악기 내 보표 간 최소 간격(하한선)
            measure_width: 60.0,        // px, 내용이 거의 없는 마디를 위한 최소 폭(하한선)
            note_head_width: 11.0, // px, 노트헤드가 차지한다고 가정하는 폭(스페이싱+드로잉 겸용)
            font_family: "Bravura, Leland, serif".to_string(), // 일반 텍스트용 CSS 폰트 목록
            page_width: Some(1200.0), // px, 시스템(줄) 목표 폭 (None이면 한 줄로 가로 스크롤)
            filter_parts: None,    // 모든 파트를 렌더링
            debug_metadata: false, // SVG에 디버그 주석을 추가하지 않음
            spacing_strategy: SpacingStrategy::Compact, // 기본값은 고정 간격(Compact) 스페이싱
            accidental_prefix: 12.0, // px, 임시표가 있는 음표 앞 여백
            arpeggio_prefix_bonus: 12.0, // px, 아르페지오 표시가 있는 음표 앞 추가 여백
            dot_suffix_width: 4.0, // px, 붙임점 1개당 추가 폭
            dot_suffix_flat_bonus: 6.0, // px, 붙임점이 하나라도 있으면 붙는 고정 보너스
            direction_clearance: 8.0, // px, 인접한 Direction 라벨 사이 최소 간격
            attribute_prefix_bonus: 6.0, // px, 마디 중간 속성 변경 직후 음표 앞 여백
            measure_start_padding: 12.0, // px, 마디 콘텐츠 맨 앞 고정 패딩
            measure_end_padding: 2.0, // px, 마디 콘텐츠 맨 뒤 고정 패딩
            mobile_onset_gap: 7.0, // px, Mobile 스페이싱 전략 전용 기본 온셋 간격
            beamed_stem_tips: std::cell::RefCell::new(Vec::new()),
            drawn_extent: std::cell::Cell::new(None),
            placed_dynamics: std::cell::RefCell::new(Vec::new()),
            placed_wedges: std::cell::RefCell::new(Vec::new()),
            gap_items: std::cell::RefCell::new(Vec::new()),
            measure_span: std::cell::Cell::new((0.0, 0.0)),
            current_measure_idx: std::cell::Cell::new(0),
            grace_scale: std::cell::Cell::new(1.0),
            grace_marks: std::cell::RefCell::new(Vec::new()),
            outer_wedges: std::cell::RefCell::new(HashMap::new()),
            current_part_id: std::cell::RefCell::new(String::new()),
            open_wavy_notes: std::cell::RefCell::new(HashSet::new()),
            placed_slurs: std::cell::RefCell::new(Vec::new()),
            placed_notes: std::cell::RefCell::new(Vec::new()),
            pending_tuplets: std::cell::RefCell::new(Vec::new()),
            placed_accidentals: std::cell::RefCell::new(Vec::new()),
            placed_tuplets: std::cell::RefCell::new(Vec::new()),
            placed_fingerings: std::cell::RefCell::new(Vec::new()),
            pending_beam_marks: std::cell::RefCell::new(Vec::new()),
            placed_text_boxes: std::cell::RefCell::new(Vec::new()),
            current_part_y: std::cell::Cell::new(0.0),
            deferred_pedals: std::cell::RefCell::new(Vec::new()),
        }
    }
}

impl Renderer {
    /// A preset tuned for narrow (mobile) screens: tighter horizontal note
    /// spacing, smaller margins, and reduced vertical distance between
    /// parts/staves. Glyph scale (`staff_line_distance`) and `note_head_width`
    /// are intentionally left unchanged since they're load-bearing for
    /// collision-free drawing, not just spacing heuristics.
    pub fn mobile() -> Self {
        let mut r = Self::default();
        r.apply_mobile_preset();
        r
    }

    pub fn apply_mobile_preset(&mut self) {
        self.spacing_strategy = SpacingStrategy::Mobile;
        self.margin_left = 2.0;
        self.margin_right = 2.0;
        self.margin_top = 28.0;
        self.part_distance = 72.0;
        self.grand_staff_distance = 72.0;
        self.measure_width = 48.0;
        self.accidental_prefix = 8.0;
        self.arpeggio_prefix_bonus = 8.0;
        self.dot_suffix_width = 3.0;
        self.dot_suffix_flat_bonus = 3.0;
        self.direction_clearance = 5.0;
        self.attribute_prefix_bonus = 4.0;
        self.measure_start_padding = 8.0;
        self.measure_end_padding = 1.0;
        self.mobile_onset_gap = 7.0;
    }
}

#[derive(Clone)]
struct BracketStartInfo {
    x: f32,
    y: f32,
    line_type: Option<String>,
    line_end: Option<String>,
    /// Carried over a system break: `y` is relative to the part's top until
    /// the next system starts.
    is_continuation: bool,
    /// Text leading the line (a barre "IV") and the x it starts at; drawn
    /// with the line, once its height is known.
    label: Option<(String, f32)>,
    is_below: bool,
    staff: i32,
    /// A stop met before its start (an exporter writing one voice's stop
    /// ahead of another voice's start at an earlier beat): its x and line
    /// end, waiting for the start later in the same measure.
    pending_stop: Option<(f32, Option<String>)>,
}

#[derive(Clone)]
struct EndingStartInfo {
    x: f32,
    y: f32,
    text: String,
    is_continuation: bool,
}

enum StackedItem {
    /// type, placement, staff, and for a bracket or dashes line starting
    /// with a text in the same direction (a barre "½C9", "cresc."), that text,
    /// printed inline at the line's start.
    DirectionType(crate::models::DirectionType, Option<String>, Option<i32>, Option<String>),
    Harmony(crate::models::Harmony),
    Lyric(crate::models::Lyric, i32), // lyric, staff
}

struct StackedElement {
    priority: i32,
    time_pos: i32,
    item: StackedItem,
}

#[derive(Clone)]
struct FiguredBassStartInfo {
    x: f32,
    y: f32,
}

#[derive(Clone)]
struct DashesStartInfo {
    x: f32,
    y: f32,
    dash_length: Option<f32>,
    space_length: Option<f32>,
    is_continuation: bool,
}

#[derive(Clone)]
struct LyricStartInfo {
    x: f32,
    y: f32,
    is_continuation: bool,
}

#[derive(Clone)]
struct WedgeStartInfo {
    x: f32,
    y: f32,
    wedge_type: String,
    is_continuation: bool,
    start_spread: f32,
    is_below: bool,
    staff: i32,
    /// Continued from a previous system and not yet placed on this system's
    /// dynamics line (`None`), or the line it has been placed on (`Some`).
    resumed_line: Option<Option<f32>>,
    /// Placed in the gap to the next staff (decided once for the whole
    /// hairpin, see `Renderer::outer_wedges`), so it keeps that side on every
    /// system it crosses.
    in_gap: bool,
}

#[derive(Clone)]
struct GlissandoStartInfo {
    x: f32,
    y: f32,
    line_type: Option<String>,
    text: Option<String>,
    is_continuation: bool,
}

#[derive(Clone)]
struct HammerOnPullOffStartInfo {
    x: f32,
    y: f32,
    text: String,
}

#[derive(Clone)]
struct TremoloStartInfo {
    x: f32,
    y: f32,
    bars: i32,
    is_continuation: bool,
}

#[derive(Clone)]
struct WavyLineStartInfo {
    x: f32,
    y: f32,
    is_continuation: bool,
    _number: i32,
}

#[derive(Clone)]
struct SlideStartInfo {
    x: f32,
    y: f32,
    line_type: Option<String>,
    is_continuation: bool,
}

#[derive(Clone)]
struct GroupingStartInfo {
    x: f32,
    y: f32,
    features: Vec<crate::models::GroupingFeature>,
    is_continuation: bool,
}

#[derive(Clone)]
struct OctaveShiftStartInfo {
    x: f32,
    y: f32,
    size: i32,
    is_continuation: bool,
    shift_type: String, // "up" or "down"
    staff: Option<i32>,
    is_below: bool,
    /// Its stop has been passed in the measure being drawn: later notes are
    /// no longer shifted, though the line is only drawn (and the entry
    /// removed) with the measure's directions.
    ended: bool,
}

#[derive(Clone)]
struct PedalStartInfo {
    x: f32,
    y: f32,
    _line: bool,
    is_initial: bool,
    glyph: String,
    is_continuation: bool,
    is_below: bool,
}

#[derive(Clone)]
struct PartState {
    active_slurs: HashMap<i32, SlurStartInfo>,
    /// Keyed by (step, octave, staff): a part can hold the same pitch on two
    /// staves at once (notation + TAB), each with its own tie.
    active_ties: HashMap<(String, i32, i32), TieStartInfo>,
    active_tuplets: HashMap<i32, TupletStartInfo>,
    active_brackets: HashMap<i32, BracketStartInfo>,
    active_dashes: HashMap<i32, DashesStartInfo>,
    active_ending: Option<EndingStartInfo>,
    active_figured_bass: HashMap<usize, FiguredBassStartInfo>,
    active_lyrics: HashMap<i32, LyricStartInfo>, // verse number -> info
    active_wedges: HashMap<i32, WedgeStartInfo>, // number -> info
    active_glissandi: HashMap<i32, GlissandoStartInfo>, // number -> info
    active_slides: HashMap<i32, SlideStartInfo>, // number -> info
    active_hammer_ons: HashMap<i32, HammerOnPullOffStartInfo>, // number -> info
    active_pull_offs: HashMap<i32, HammerOnPullOffStartInfo>, // number -> info
    active_tremolos: HashMap<i32, TremoloStartInfo>, // Dummy key or index -> info
    active_wavy_lines: HashMap<i32, WavyLineStartInfo>, // number -> info
    active_groupings: HashMap<i32, GroupingStartInfo>, // number -> info
    active_octave_shifts: HashMap<i32, OctaveShiftStartInfo>, // number -> info
    active_pedals: HashMap<i32, PedalStartInfo>, // number -> info
    measure_repeat_active: bool,
    beat_repeat_active: bool,
    beat_repeat_first: bool,
    slash_active: bool,
    slash_use_stems: bool,
    slash_dots: i32,
    slash_note_type: Option<String>,
    beat_repeat_slashes: i32,
    current_beats: i32,
    current_beat_type: i32,
    divisions: i32,
    multiple_rest_remaining: i32,
    multiple_rest_total: i32,
    current_clefs: HashMap<i32, Clef>,
    current_key: crate::models::Key,
    staff_lines: HashMap<i32, i32>,
    num_staves: i32,
    lyric_baseline_y: f32, // Relative to part_start_y
}

impl Default for PartState {
    fn default() -> Self {
        Self {
            active_slurs: HashMap::new(),
            active_ties: HashMap::new(),
            active_tuplets: HashMap::new(),
            active_brackets: HashMap::new(),
            active_dashes: HashMap::new(),
            active_ending: None,
            active_figured_bass: HashMap::new(),
            active_lyrics: HashMap::new(),
            active_wedges: HashMap::new(),
            active_glissandi: HashMap::new(),
            active_slides: HashMap::new(),
            active_hammer_ons: HashMap::new(),
            active_pull_offs: HashMap::new(),
            active_tremolos: HashMap::new(),
            active_wavy_lines: HashMap::new(),
            active_groupings: HashMap::new(),
            active_octave_shifts: HashMap::new(),
            active_pedals: HashMap::new(),
            measure_repeat_active: false,
            beat_repeat_active: false,
            beat_repeat_first: false,
            slash_active: false,
            slash_use_stems: false,
            slash_dots: 0,
            slash_note_type: None,
            beat_repeat_slashes: 1,
            current_beats: 4,
            current_beat_type: 4,
            divisions: 1,
            multiple_rest_remaining: 0,
            multiple_rest_total: 0,
            current_clefs: HashMap::new(),
            current_key: crate::models::Key::default(),
            staff_lines: HashMap::new(),
            num_staves: 1,
            lyric_baseline_y: 0.0,
        }
    }
}

struct SystemLayout {
    measure_indices: Vec<usize>,
    measure_widths: Vec<f32>,
}

/// Extra top clearance (replacing the bare-notehead baseline of 10.0) needed for a
/// note carrying an above-placed slur/ornament/articulation/fermata/technical mark,
/// so `max_top_protrusion` reserves enough room to avoid clipping at the system's
/// top boundary. Constants mirror the chain-adjustment offsets used in
/// process_ornaments/process_fermatas/process_articulations/process_technical.
fn notation_top_clearance(notations: &[Notation]) -> f32 {
    let mut clearance: f32 = 10.0;
    for notation in notations {
        let c = match notation {
            // Constants below are calibrated against actual rendered pixel output
            // (not just the drawing code's internal "chain" spacing offsets, which
            // undershoot real SMuFL glyph extents), so each stays clearly above the
            // ~32px floor that measure-number reservation already provides -- see
            // notation_top_clearance's doc comment above.
            Notation::Slur {
                note_type,
                placement,
                ..
            } if (note_type == "start" || note_type == "stop")
                && placement.as_deref() != Some("below") =>
            {
                55.0
            }
            Notation::Fermata {
                note_type,
                placement,
            } if note_type.as_deref() != Some("inverted")
                && placement.as_deref() != Some("below") =>
            {
                45.0
            }
            Notation::Articulation { placement, .. } if placement.as_deref() != Some("below") => {
                35.0
            }
            Notation::Tuplet { placement, .. } if placement.as_deref() != Some("below") => 40.0,
            Notation::AccidentalMark(_) => 40.0,
            Notation::Technical(marks) => marks
                .iter()
                .map(|m| match m {
                    crate::models::TechnicalMark::Bend(_) => 58.0,
                    crate::models::TechnicalMark::Fret(_)
                    | crate::models::TechnicalMark::String(_)
                    | crate::models::TechnicalMark::HammerOn { .. }
                    | crate::models::TechnicalMark::PullOff { .. }
                    | crate::models::TechnicalMark::Pizzicato { .. }
                    | crate::models::TechnicalMark::Other(_) => 0.0,
                    _ => 48.0,
                })
                .fold(0.0_f32, f32::max),
            Notation::Ornaments(ornaments) => ornaments
                .iter()
                .map(|o| match o {
                    crate::models::Ornament::TrillMark
                    | crate::models::Ornament::WavyLine { .. } => 48.0,
                    crate::models::Ornament::Schleifer { .. } => 70.0,
                    crate::models::Ornament::Shake { .. }
                    | crate::models::Ornament::Turn
                    | crate::models::Ornament::DelayedTurn
                    | crate::models::Ornament::InvertedTurn
                    | crate::models::Ornament::DelayedInvertedTurn
                    | crate::models::Ornament::Haydn
                    | crate::models::Ornament::VerticalTurn
                    | crate::models::Ornament::InvertedVerticalTurn
                    | crate::models::Ornament::Mordent { .. }
                    | crate::models::Ornament::InvertedMordent { .. } => 48.0,
                    crate::models::Ornament::Tremolo { tremolo_type, .. }
                        if tremolo_type == "single" =>
                    {
                        45.0
                    }
                    crate::models::Ornament::AccidentalMark(_) => 40.0,
                    _ => 0.0,
                })
                .fold(0.0_f32, f32::max),
            _ => 0.0,
        };
        clearance = clearance.max(c);
    }
    clearance
}

impl Renderer {
    /// How far left of the staves the score's braces / brackets reach: a
    /// brace (multi-staff part or brace group) about 22px, a bracket's bar 6px.
    fn group_symbol_reach(score: &Score) -> f32 {
        let has_brace_group = score.part_list.iter().any(|item| {
            matches!(item, PartListItem::Group(g) if matches!(g.symbol, Some(GroupSymbol::Brace)))
        });
        let has_multi_staff = score.parts.iter().any(|p| {
            p.measures.iter().any(|m| m.attributes.as_ref().and_then(|a| a.staves).is_some_and(|n| n > 1))
        });
        let has_bracket = score.part_list.iter().any(|item| {
            matches!(item, PartListItem::Group(g) if matches!(g.symbol, Some(GroupSymbol::Bracket)))
        });
        if has_brace_group || has_multi_staff {
            24.0
        } else if has_bracket {
            8.0
        } else {
            0.0
        }
    }

    fn calculate_system_margin(&self, score: &Score, is_first: bool) -> f32 {
        if !is_first && self.spacing_strategy == SpacingStrategy::Mobile {
            // Mobile mode only draws part/group name labels on the first system
            // (see the draw call sites below), so later systems don't need to
            // reserve any label width in their left margin — only room for the
            // braces / brackets drawn left of the staves (see draw_group_symbol).
            return self.margin_left.max(Self::group_symbol_reach(score));
        }
        let mut max_label_width: f32 = 0.0;
        let part_font_size = if is_first { 16.0 } else { 12.0 };
        let group_font_size = if is_first { 14.0 } else { 10.0 };

        for item in &score.part_list {
            match item {
                PartListItem::Part {
                    name, abbreviation, ..
                } => {
                    let display_text = if is_first {
                        name.as_deref().or(abbreviation.as_deref())
                    } else {
                        abbreviation.as_deref().or(name.as_deref())
                    };

                    if let Some(text) = display_text {
                        max_label_width = max_label_width
                            .max(self.estimate_part_label_width(text, part_font_size));
                    }
                }
                PartListItem::Group(group) => {
                    let name = if is_first {
                        group.name.as_deref().or(group.abbreviation.as_deref())
                    } else {
                        group.abbreviation.as_deref().or(group.name.as_deref())
                    };

                    if let Some(name) = name {
                        let display_name = if name.chars().count() > 14 {
                            let mut truncated: String = name.chars().take(12).collect();
                            truncated.push_str("...");
                            truncated
                        } else {
                            name.to_string()
                        };
                        max_label_width = max_label_width
                            .max(self.estimate_group_label_width(&display_name, group_font_size));
                    }
                }
            }
        }

        if max_label_width > 0.0 {
            // Text starts at 6.0px from page edge.
            // margin_left = 6.0 (left padding) + max_label_width + 16.0 (gap to staff)
            (max_label_width + 22.0).max(self.margin_left)
        } else {
            self.margin_left
        }
    }

    pub fn render(&self, score: &Score) -> String {
        self.render_with_metadata(score).0
    }

    pub fn render_with_metadata(&self, score_in: &Score) -> (String, PlayMetadata) {
        #[allow(unused_assignments)]
        let mut score_storage: Option<Score> = None;
        let score = if let Some(target_ids) = &self.filter_parts {
            let mut s = score_in.clone();
            s.filter_parts(target_ids);
            score_storage = Some(s);
            score_storage.as_ref().unwrap()
        } else {
            score_in
        };

        let first_margin = self.calculate_system_margin(score, true);
        let subsequent_margin = self.calculate_system_margin(score, false);

        let analysis = self.analyze_measures(score);
        let max_measures = analysis.max_measures;
        let raw_measure_widths = analysis.raw_widths;
        let fixed_measure_widths = analysis.fixed_widths;
        let system_start_extra_widths = analysis.system_start_extra_widths;
        // Widths of a system's measures stretched (or squeezed) to `usable`:
        // only the note content of each measure takes up the difference, its
        // padding and attributes (and the system-start clef, key and time)
        // keep their size, so every measure's notes are spaced alike.
        let justify = |indices: &[usize], usable: f32, stretch: bool| -> Vec<f32> {
            let fixed = |pos: usize, idx: usize| {
                fixed_measure_widths[idx] + if pos == 0 { system_start_extra_widths[idx] } else { 0.0 }
            };
            let fixed_sum: f32 = indices.iter().enumerate().map(|(p, &i)| fixed(p, i)).sum();
            let content_sum: f32 =
                indices.iter().map(|&i| raw_measure_widths[i] - fixed_measure_widths[i]).sum();
            let scale = if stretch && content_sum > 0.0 {
                ((usable - fixed_sum) / content_sum).max(0.0)
            } else {
                1.0
            };
            indices
                .iter()
                .enumerate()
                .map(|(p, &i)| fixed(p, i) + (raw_measure_widths[i] - fixed_measure_widths[i]) * scale)
                .collect()
        };
        let measure_spacings = analysis.spacings;
        let measure_total_durs = analysis.total_durs;
        let measure_theoretical_durs = analysis.theoretical_durs;
        let current_divisions_per_part = analysis.divisions_per_part;
        let measure_max_reaches: Vec<HashMap<String, f32>> = vec![HashMap::new(); max_measures];
        // 2. Break into systems
        let mut systems = Vec::new();
        if let Some(target_width) = self.page_width {
            let mut current_indices = Vec::new();
            let mut current_sum = 0.0;

            for (m_idx, &mw) in raw_measure_widths.iter().enumerate() {
                let current_margin_left = if systems.is_empty() {
                    first_margin
                } else {
                    subsequent_margin
                };
                let usable_width = target_width - current_margin_left - self.margin_right;
                let candidate_width = if current_indices.is_empty() {
                    mw + system_start_extra_widths[m_idx]
                } else {
                    mw
                };

                if current_sum + candidate_width > usable_width && !current_indices.is_empty() {
                    let scaled_widths = justify(&current_indices, usable_width, true);
                    systems.push(SystemLayout {
                        measure_indices: current_indices.clone(),
                        measure_widths: scaled_widths,
                    });
                    current_indices.clear();
                    current_sum = 0.0;
                }
                current_indices.push(m_idx);
                current_sum += if current_indices.len() == 1 {
                    mw + system_start_extra_widths[m_idx]
                } else {
                    mw
                };
            }
            if !current_indices.is_empty() {
                let current_margin_left = if systems.is_empty() {
                    first_margin
                } else {
                    subsequent_margin
                };
                let usable_width = target_width - current_margin_left - self.margin_right;
                let scaled_widths =
                    justify(&current_indices, usable_width, current_sum > usable_width * 0.5);
                systems.push(SystemLayout {
                    measure_indices: current_indices,
                    measure_widths: scaled_widths,
                });
            }
        } else {
            systems.push(SystemLayout {
                measure_indices: (0..max_measures).collect(),
                measure_widths: raw_measure_widths
                    .iter()
                    .enumerate()
                    .map(|(idx, &width)| {
                        width
                            + if idx == 0 {
                                system_start_extra_widths[idx]
                            } else {
                                0.0
                            }
                    })
                    .collect(),
            });
        }

        // Initialize document
        let final_width = self.page_width.unwrap_or(
            first_margin.max(subsequent_margin)
                + self.margin_right
                + raw_measure_widths.iter().sum::<f32>()
                + system_start_extra_widths.first().copied().unwrap_or(0.0),
        );
        let mut current_y = self.margin_top;
        let mut document = Document::new().set("style", "background-color: white");
        let style = svg::node::element::Style::new(
            "@font-face { font-family: 'Bravura'; src: local('Bravura'); } text { font-family: 'Bravura', serif; }",
        );
        document = document.add(style);

        // Per-system tracking for virtual SVG splitting
        let mut sys_boundaries_vec: Vec<crate::models::SystemBoundary> = Vec::new();
        let mut measure_to_sys: HashMap<usize, usize> = HashMap::new();

        // Persistent state across systems
        *self.open_wavy_notes.borrow_mut() =
            score.parts.iter().flat_map(Self::open_wavy_notes).collect();
        *self.outer_wedges.borrow_mut() = score
            .parts
            .iter()
            .map(|p| (p.id.clone(), Self::outer_wedges(p)))
            .collect();
        let mut part_states: HashMap<String, PartState> = HashMap::new();
        let mut all_measure_render_info = HashMap::new();
        for part in &score.parts {
            let mut state = PartState::default();
            state.divisions = *current_divisions_per_part.get(&part.id).unwrap_or(&1);
            state.current_clefs.insert(
                1,
                Clef {
                    number: 1,
                    sign: "G".to_string(),
                    line: Some(2),
                    ..Default::default()
                },
            );

            // Seed initial state from the very first measure's attributes if they exist
            if let Some(first_m) = part.measures.first() {
                if let Some(attr) = &first_m.attributes {
                    for c in &attr.clefs {
                        state.current_clefs.insert(c.number, c.clone());
                    }
                    for sd in &attr.staff_details {
                        if let Some(lines) = sd.staff_lines {
                            state.staff_lines.insert(sd.number, lines);
                        }
                    }
                    if let Some(k) = &attr.key {
                        state.current_key = k.clone();
                    }
                    if let Some(t) = &attr.time {
                        if let Ok(b) = t.beats.split('+').next().unwrap_or("4").parse::<i32>() {
                            state.current_beats = b;
                        }
                        state.current_beat_type = t.beat_type;
                    }
                    if let Some(n) = attr.staves {
                        state.num_staves = n;
                        if n > 1 && !state.current_clefs.contains_key(&2) {
                            state.current_clefs.insert(
                                2,
                                Clef {
                                    number: 2,
                                    sign: "F".to_string(),
                                    line: Some(4),
                                    ..Default::default()
                                },
                            );
                        }
                    }
                }
            }
            part_states.insert(part.id.clone(), state);
        }

        for (sys_idx, system) in systems.iter().enumerate() {
            // A system may be laid out twice: slurs are only shaped while
            // drawing, so if slurs below one staff and above the next collide,
            // the staff gap is widened and the system redrawn from the saved
            // part state.
            let part_states_at_system_start = part_states.clone();
            let mut extra_staff_gap: HashMap<String, f32> = HashMap::new();
            // Extra space above a part whose upper neighbour's late-drawn
            // elements (pedals, slurs, directions) reach into it.
            let mut extra_part_gap: HashMap<String, f32> = HashMap::new();
            let mut layout_attempt = 0;
            loop {
            let mut sys_doc = Document::new();
            self.beamed_stem_tips.borrow_mut().clear();
            let sys_y_before = current_y;
            self.drawn_extent.set(None);
            self.placed_dynamics.borrow_mut().clear();
            self.placed_text_boxes.borrow_mut().clear();
            self.placed_wedges.borrow_mut().clear();
            self.gap_items.borrow_mut().clear();
            self.placed_slurs.borrow_mut().clear();
            self.placed_notes.borrow_mut().clear();
            self.pending_tuplets.borrow_mut().clear();
            self.placed_accidentals.borrow_mut().clear();
            self.placed_tuplets.borrow_mut().clear();
            self.placed_fingerings.borrow_mut().clear();
            self.pending_beam_marks.borrow_mut().clear();
            self.deferred_pedals.borrow_mut().clear();
            let current_margin_left = if sys_idx == 0 {
                first_margin
            } else {
                subsequent_margin
            };

            // 5. Dynamic vertical layout for THIS system
            let mut part_staff_distances = HashMap::new();
            let mut part_y_offsets = HashMap::new();
            let mut part_bottom_offsets = HashMap::new();
            let mut part_staff_bottom_offsets = HashMap::new();
            let mut total_system_height = 0.0;

            // Add custom tracking maps for precision vertical layout
            let mut part_onset_bounds_list = Vec::new();
            let mut part_onset_bounds_map: HashMap<
                String,
                HashMap<usize, HashMap<i32, (f32, f32)>>,
            > = HashMap::new();

            for item in &score.part_list {
                if let PartListItem::Part { id, .. } = item {
                    if let Some(part) = score.parts.iter().find(|p| p.id == *id) {
                        let state = part_states.get(id).unwrap();

                        // A. Calculate staff distance for this system (Piano/Grand Staff expansion)
                        let mut max_needed_staff_dist = self.grand_staff_distance;
                        let mut current_clefs_temp = state.current_clefs.clone();
                        let mut current_num_staves = state.num_staves;

                        for &m_idx in &system.measure_indices {
                            let measure = &part.measures[m_idx];
                            if let Some(attr) = &measure.attributes {
                                if let Some(n) = attr.staves {
                                    current_num_staves = n;
                                }
                                for clef in &attr.clefs {
                                    current_clefs_temp.insert(clef.number, clef.clone());
                                }
                            }
                            #[derive(Clone, Copy)]
                            struct StaffBounds {
                                head: (f32, f32),
                                total: (f32, f32),
                            }
                            let mut onset_staff_bounds: HashMap<i32, HashMap<i32, StaffBounds>> =
                                HashMap::new();
                            // Note-only envelopes (incl. stems) per onset/staff. Directions are
                            // placed relative to these rather than to `onset_staff_bounds`,
                            // which also holds earlier directions at the same onset; using
                            // that made every extra direction on a beat (mf + hairpin + ...)
                            // stack another 40px further out and wildly overestimate the gap.
                            let mut onset_note_bounds: HashMap<i32, HashMap<i32, (f32, f32)>> =
                                HashMap::new();
                            let mut time_pos = 0;
                            let mut chord_time_pos = 0;
                            let mut divisions = *current_divisions_per_part.get(id).unwrap_or(&1);

                            let cross_beamed = Self::cross_staff_beam_notes(measure);
                            // staff -> (highest head top, lowest head bottom, beam levels)
                            // of its notes in beams joining two staves: the beam
                            // runs between the upper staff's and the lower's.
                            let mut cross_heads: HashMap<i32, (f32, f32, u32)> = HashMap::new();
                            // Extract detailed relative vertical bounds (min_y, max_y) relative to the top line of each staff
                            for el in &measure.elements {
                                match el {
                                    MeasureElement::Note(n) => {
                                        let current_time =
                                            if n.is_chord { chord_time_pos } else { time_pos };
                                        let s_num = n.staff.unwrap_or(1);
                                        let clef = current_clefs_temp
                                            .get(&s_num)
                                            .cloned()
                                            .unwrap_or(Clef {
                                                number: 1,
                                                sign: "G".to_string(),
                                                line: Some(2),
                                                ..Default::default()
                                            });

                                        let mut note_min_total = 0.0;
                                        let mut note_max_total = 40.0;
                                        let mut note_min_head = 0.0;
                                        let mut note_max_head = 40.0;
                                        // Stem direction of this note (None for rests / unpitched),
                                        // used to decide which side of the staff marks land on.
                                        let mut note_stem_up: Option<bool> = None;

                                        // TAB fret numbers sit on the string lines, not at the pitch's
                                        // staff position, and carry no stems; a hidden note draws nothing.
                                        if let Some(pitch) = Self::display_pitch(n).as_ref().filter(|_| clef.sign != "TAB" && n.print_object != Some(false)) {
                                            let y = self.pitch_to_y(pitch, &clef, 0.0);
                                            note_min_head = y - 5.0;
                                            note_max_head = y + 5.0;
                                            note_min_total = y - 5.0;
                                            note_max_total = y + 5.0;

                                            let is_stem_up = if let Some(st) = &n.stem {
                                                st == "up"
                                            } else {
                                                y >= 20.0
                                            };

                                            let beam_extra = if !n.beams.is_empty() {
                                                let max_beam_level = n
                                                    .beams
                                                    .iter()
                                                    .map(|b| b.number)
                                                    .max()
                                                    .unwrap_or(1);
                                                25.0 + (max_beam_level.saturating_sub(1) as f32
                                                    * 7.5)
                                            } else {
                                                0.0
                                            };
                                            // A stem joining the other staff's notes in one
                                            // beam spans the gap at whatever size it gets.
                                            let stem_clearance = if cross_beamed
                                                .contains(&(n as *const Note as usize))
                                            {
                                                let levels = n.beams.iter().map(|b| b.number).max().unwrap_or(1).max(1) as u32;
                                                let e = cross_heads
                                                    .entry(s_num)
                                                    .or_insert((f32::MAX, f32::MIN, 1));
                                                e.0 = e.0.min(y - 5.0);
                                                e.1 = e.1.max(y + 5.0);
                                                e.2 = e.2.max(levels);
                                                0.0
                                            } else {
                                                40.0 + beam_extra
                                            };
                                            note_stem_up = Some(is_stem_up);

                                            if is_stem_up {
                                                note_min_total =
                                                    note_min_total.min(y - stem_clearance);
                                            } else {
                                                note_max_total =
                                                    note_max_total.max(y + stem_clearance);
                                            }
                                        }

                                        if !n.lyrics.is_empty() {
                                            for lyric in &n.lyrics {
                                                let verse = lyric.number.unwrap_or(1) as f32;
                                                let lyric_base_total = 48.0f32.max(
                                                    note_max_total + 8.0 + (verse - 1.0) * 15.0,
                                                );
                                                note_max_total =
                                                    note_max_total.max(lyric_base_total + 15.0);

                                                let lyric_base_head = 48.0f32.max(
                                                    note_max_head + 8.0 + (verse - 1.0) * 15.0,
                                                );
                                                note_max_head =
                                                    note_max_head.max(lyric_base_head + 15.0);
                                            }
                                        }

                                        for h in &n.harmonies {
                                            let harm_span =
                                                if h.frame.is_some() { 90.0 } else { 35.0 };
                                            note_min_total = note_min_total.min(-harm_span);
                                            note_min_head = note_min_head.min(-harm_span);
                                        }

                                        for notation in &n.notations {
                                            match notation {
                                                Notation::Tuplet { placement, .. }
                                                | Notation::Fermata { placement, .. }
                                                | Notation::Articulation { placement, .. } => {
                                                    // Only reserve room on the side the mark is
                                                    // actually drawn on, so a mark above the top
                                                    // staff (or below the bottom one) does not
                                                    // widen the gap between the staves.
                                                    let above = match placement.as_deref() {
                                                        Some("above") => true,
                                                        Some("below") => false,
                                                        _ => match notation {
                                                            // Fermatas sit above unless inverted.
                                                            Notation::Fermata { note_type, .. } => {
                                                                note_type.as_deref() != Some("inverted")
                                                            }
                                                            // Tuplet numbers go on the stem side.
                                                            Notation::Tuplet { .. } => {
                                                                note_stem_up.unwrap_or(true)
                                                            }
                                                            // Articulations go opposite the stem.
                                                            _ => !note_stem_up.unwrap_or(false),
                                                        },
                                                    };
                                                    // A fermata glyph reaches further from the
                                                    // staff than a dot or a tuplet number.
                                                    let (top, bottom) =
                                                        if matches!(notation, Notation::Fermata { .. }) {
                                                            (-FERMATA_REACH, 40.0 + FERMATA_REACH)
                                                        } else {
                                                            (-15.0, 55.0)
                                                        };
                                                    if above {
                                                        note_min_total = note_min_total.min(top);
                                                        note_min_head = note_min_head.min(top);
                                                    } else {
                                                        note_max_total = note_max_total.max(bottom);
                                                        note_max_head = note_max_head.max(bottom);
                                                    }
                                                }
                                                Notation::Ornaments(ornaments) => {
                                                    for orn in ornaments {
                                                        match orn {
                                                            crate::models::Ornament::TrillMark | crate::models::Ornament::Turn | crate::models::Ornament::VerticalTurn => {
                                                                note_min_total = note_min_total.min(-18.0);
                                                                note_min_head = note_min_head.min(-18.0);
                                                            }
                                                            _ => {}
                                                        }
                                                    }
                                                }
                                                _ => {}
                                            }
                                        }

                                        let entry = onset_staff_bounds
                                            .entry(current_time)
                                            .or_insert_with(HashMap::new)
                                            .entry(s_num)
                                            .or_insert(StaffBounds {
                                                head: (0.0, 40.0),
                                                total: (0.0, 40.0),
                                            });
                                        entry.head.0 = entry.head.0.min(note_min_head);
                                        entry.head.1 = entry.head.1.max(note_max_head);
                                        entry.total.0 = entry.total.0.min(note_min_total);
                                        entry.total.1 = entry.total.1.max(note_max_total);
                                        let note_entry = onset_note_bounds
                                            .entry(current_time)
                                            .or_insert_with(HashMap::new)
                                            .entry(s_num)
                                            .or_insert((0.0, 40.0));
                                        note_entry.0 = note_entry.0.min(note_min_total);
                                        note_entry.1 = note_entry.1.max(note_max_total);

                                        if !n.is_chord {
                                            chord_time_pos = time_pos;
                                            time_pos += (n.duration as f64 * 10080.0
                                                / divisions as f64)
                                                .round()
                                                as i32;
                                        }
                                    }
                                    MeasureElement::Forward(d) => {
                                        time_pos +=
                                            (*d as f64 * 10080.0 / divisions as f64).round() as i32;
                                        chord_time_pos = time_pos;
                                    }
                                    MeasureElement::Backup(d) => {
                                        time_pos -=
                                            (*d as f64 * 10080.0 / divisions as f64).round() as i32;
                                        time_pos = time_pos.max(0);
                                        chord_time_pos = time_pos;
                                    }
                                    MeasureElement::Attributes(attr) => {
                                        if let Some(d) = attr.divisions {
                                            divisions = d;
                                        }
                                        if let Some(n) = attr.staves {
                                            current_num_staves = n;
                                        }
                                        for c in &attr.clefs {
                                            current_clefs_temp.insert(c.number, c.clone());
                                        }
                                    }
                                    MeasureElement::Direction(dir) => {
                                        let is_below = dir.placement.as_deref() == Some("below");
                                        let s_num = dir.staff.unwrap_or(if is_below {
                                            current_num_staves
                                        } else {
                                            1
                                        });

                                        let mut dir_y_rel = if is_below {
                                            7.5 * self.staff_line_distance
                                        } else {
                                            -3.5 * self.staff_line_distance
                                        };

                                        if let Some(m_map) = onset_note_bounds.get(&time_pos) {
                                            if let Some(&(n_min, n_max)) = m_map.get(&s_num) {
                                                if n_min < f32::MAX && n_max > f32::MIN {
                                                    if is_below {
                                                        dir_y_rel = dir_y_rel.max(n_max + 40.0);
                                                    } else {
                                                        dir_y_rel = dir_y_rel.min(n_min - 40.0);
                                                    }
                                                }
                                            }
                                        }

                                        let mut element_min = dir_y_rel;
                                        let mut element_max = dir_y_rel;

                                        for dt in &dir.types {
                                            match dt {
                                                crate::models::DirectionType::Dynamics(_) => {
                                                    if is_below {
                                                        element_max =
                                                            element_max.max(dir_y_rel + 20.0);
                                                    } else {
                                                        element_min =
                                                            element_min.min(dir_y_rel - 20.0);
                                                    }
                                                }
                                                crate::models::DirectionType::Words(_) => {
                                                    if is_below {
                                                        element_max =
                                                            element_max.max(dir_y_rel + 15.0);
                                                    } else {
                                                        element_min =
                                                            element_min.min(dir_y_rel - 15.0);
                                                    }
                                                }
                                                crate::models::DirectionType::Rehearsal(_) => {
                                                    let mut reh_y = -4.0 * self.staff_line_distance;
                                                    if let Some(m_map) =
                                                        onset_note_bounds.get(&time_pos)
                                                    {
                                                        if let Some(&(n_min, _)) = m_map.get(&s_num) {
                                                            if n_min < f32::MAX {
                                                                reh_y = reh_y.min(n_min - 30.0);
                                                            }
                                                        }
                                                    }
                                                    element_min = element_min.min(reh_y - 25.0);
                                                }
                                                crate::models::DirectionType::Metronome(_) => {
                                                    let met_y = if is_below {
                                                        8.0 * self.staff_line_distance
                                                    } else {
                                                        -4.5 * self.staff_line_distance
                                                    };
                                                    if is_below {
                                                        element_max = element_max.max(met_y + 25.0);
                                                    } else {
                                                        element_min = element_min.min(met_y - 25.0);
                                                    }
                                                }
                                                _ => {}
                                            }
                                        }

                                        let entry = onset_staff_bounds
                                            .entry(time_pos)
                                            .or_insert_with(HashMap::new)
                                            .entry(s_num)
                                            .or_insert(StaffBounds {
                                                head: (0.0, 40.0),
                                                total: (0.0, 40.0),
                                            });
                                        entry.head.0 = entry.head.0.min(element_min);
                                        entry.head.1 = entry.head.1.max(element_max);
                                        entry.total.0 = entry.total.0.min(element_min);
                                        entry.total.1 = entry.total.1.max(element_max);
                                    }
                                    MeasureElement::Frame(_) => {
                                        let entry = onset_staff_bounds
                                            .entry(time_pos)
                                            .or_insert_with(HashMap::new)
                                            .entry(1)
                                            .or_insert(StaffBounds {
                                                head: (0.0, 40.0),
                                                total: (0.0, 40.0),
                                            });
                                        entry.head.0 = entry.head.0.min(-80.0);
                                        entry.total.0 = entry.total.0.min(-80.0);
                                    }
                                    MeasureElement::Harmony(h) => {
                                        let harm_span =
                                            if h.frame.is_some() { -90.0 } else { -30.0 };
                                        let entry = onset_staff_bounds
                                            .entry(time_pos)
                                            .or_insert_with(HashMap::new)
                                            .entry(1)
                                            .or_insert(StaffBounds {
                                                head: (0.0, 40.0),
                                                total: (0.0, 40.0),
                                            });
                                        entry.head.0 = entry.head.0.min(harm_span);
                                        entry.total.0 = entry.total.0.min(harm_span);
                                    }
                                    _ => {}
                                }
                            }

                            // Calculate staff spacing constraints
                            let spacing = &measure_spacings[m_idx];
                            let m_width_actual = system.measure_widths[system
                                .measure_indices
                                .iter()
                                .position(|&x| x == m_idx)
                                .unwrap_or(0)];
                            let content_w = (m_width_actual - self.measure_start_padding).max(1.0);
                            let margin = 12.0;

                            for s in 1..current_num_staves {
                                // A cross-staff beam needs its band between the upper
                                // staff's lowest and the lower staff's highest heads:
                                // a short stem (12px) each side and the beams.
                                if let (Some(&(_, upper_bottom, l1)), Some(&(lower_top, _, l2))) =
                                    (cross_heads.get(&s), cross_heads.get(&(s + 1)))
                                {
                                    let beams = l1.max(l2) as f32;
                                    let band = 2.0 * 12.0 + 5.0 + (beams - 1.0) * 7.5;
                                    max_needed_staff_dist =
                                        max_needed_staff_dist.max(upper_bottom - lower_top + band);
                                }
                                // Collect per-onset x positions to detect proximity
                                // Protrusion check: only expand if elements would physically
                                // overlap the opposite staff's LINE region [0..40].
                                //   - Staff s elements protruding below: head.1 > staff_dist
                                //     (they enter staff s+1's lines starting at staff_dist)
                                //   - Staff s+1 elements protruding above: head.0 < -staff_dist+40
                                //     (they enter staff s's lines ending at 40)
                                // Because staff_dist is what we are computing, we approximate:
                                //   head.1 > 40 + margin  → needs extra room below staff s lines
                                //   head.0 < -margin      → needs extra room above staff s+1 lines
                                for s_map in onset_staff_bounds.values() {
                                    if let Some(bounds1) = s_map.get(&s) {
                                        // Only expand if staff s elements protrude BELOW its bottom line (y > 40)
                                        if bounds1.head.1 > 40.0 {
                                            max_needed_staff_dist =
                                                max_needed_staff_dist.max(bounds1.head.1 + margin);
                                        }
                                    }
                                    if let Some(bounds2) = s_map.get(&(s + 1)) {
                                        // Only expand if staff s+1 elements protrude ABOVE its top line (y < 0)
                                        if bounds2.head.0 < 0.0 {
                                            max_needed_staff_dist = max_needed_staff_dist
                                                .max(40.0 - bounds2.head.0 + margin);
                                        }
                                    }
                                }

                                // Horizontal proximity check: when both staves have elements
                                // close to each other horizontally, prevent their total envelopes
                                // (including stems) from overlapping.
                                for (&t1, s_map1) in &onset_staff_bounds {
                                    if let Some(bounds1) = s_map1.get(&s) {
                                        let r1 = spacing.time_to_x.get(&t1).cloned().unwrap_or(0.0);
                                        let x1 = r1 * content_w;

                                        for (&t2, s_map2) in &onset_staff_bounds {
                                            if let Some(bounds2) = s_map2.get(&(s + 1)) {
                                                let r2 = spacing
                                                    .time_to_x
                                                    .get(&t2)
                                                    .cloned()
                                                    .unwrap_or(0.0);
                                                let x2 = r2 * content_w;

                                                if (x1 - x2).abs() < 40.0 {
                                                    max_needed_staff_dist = max_needed_staff_dist
                                                        .max(
                                                            bounds1.total.1 - bounds2.total.0
                                                                + margin,
                                                        );
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        let max_needed_staff_dist = max_needed_staff_dist
                            + extra_staff_gap.get(id).copied().unwrap_or(0.0);
                        part_staff_distances.insert(id.clone(), max_needed_staff_dist);

                        // B. Calculate part vertical footprint for this system
                        let staff_dist = max_needed_staff_dist;
                        let mut current_staff_lines_temp = state.staff_lines.clone();
                        let mut current_num_staves_footprint = state.num_staves;
                        // Probe for staff line changes in this system
                        for &m_idx in &system.measure_indices {
                            if let Some(m) = part.measures.get(m_idx) {
                                if let Some(attr) = &m.attributes {
                                    if let Some(n) = attr.staves {
                                        current_num_staves_footprint = n;
                                    }
                                    for sd in &attr.staff_details {
                                        if let Some(lines) = sd.staff_lines {
                                            current_staff_lines_temp.insert(sd.number, lines);
                                        }
                                    }
                                }
                            }
                        }

                        let mut part_bottom_max: f32 = 0.0;
                        for s in 0..current_num_staves_footprint {
                            let lines = *current_staff_lines_temp.get(&(s + 1)).unwrap_or(&5);
                            let start_i = (5 - lines) / 2;
                            let staff_top = (s as f32 * staff_dist)
                                + (start_i as f32 * self.staff_line_distance);
                            let staff_bottom =
                                staff_top + ((lines - 1) as f32 * self.staff_line_distance);
                            part_bottom_max = part_bottom_max.max(staff_bottom);
                        }

                        // Collect precise onset occupancy bounds
                        let mut onset_bounds: HashMap<usize, HashMap<i32, (f32, f32)>> =
                            HashMap::new();
                        let mut min_rel: f32 = 0.0;
                        let mut max_rel: f32 = 40.0;
                        let mut clefs_footprint = state.current_clefs.clone();

                        let mut current_num_staves_occ = state.num_staves;
                        for &m_idx in &system.measure_indices {
                            let m = &part.measures[m_idx];
                            if let Some(attr) = &m.attributes {
                                if let Some(n) = attr.staves {
                                    current_num_staves_occ = n;
                                }
                                for c in &attr.clefs {
                                    clefs_footprint.insert(c.number, c.clone());
                                }
                            }

                            let mut time_pos = 0;
                            let mut divisions = *current_divisions_per_part.get(id).unwrap_or(&1);
                            if let Some(attr) = &m.attributes {
                                if let Some(n) = attr.staves {
                                    current_num_staves_occ = n;
                                }
                                if let Some(d) = attr.divisions {
                                    divisions = d;
                                }
                            }

                            let mut chord_time_pos = 0;
                            let cross_beamed = Self::cross_staff_beam_notes(m);
                            for el in &m.elements {
                                match el {
                                    MeasureElement::Attributes(attr) => {
                                        if let Some(n) = attr.staves {
                                            current_num_staves_occ = n;
                                        }
                                        if let Some(d) = attr.divisions {
                                            divisions = d;
                                        }
                                        for c in &attr.clefs {
                                            clefs_footprint.insert(c.number, c.clone());
                                        }
                                    }
                                    MeasureElement::Note(n) => {
                                        let current_time =
                                            if n.is_chord { chord_time_pos } else { time_pos };
                                        let s_num = n.staff.unwrap_or(1);
                                        let s_offset = (s_num - 1) as f32 * staff_dist;
                                        let clef =
                                            clefs_footprint.get(&s_num).cloned().unwrap_or(Clef {
                                                number: 1,
                                                sign: "G".to_string(),
                                                line: Some(2),
                                                ..Default::default()
                                            });

                                        let mut note_min = s_offset;
                                        let mut note_max = s_offset + 40.0;

                                        // TAB fret numbers sit on the string lines, not at the pitch's
                                        // staff position, and carry no stems; a hidden note draws nothing.
                                        if let Some(pitch) = Self::display_pitch(n).as_ref().filter(|_| clef.sign != "TAB" && n.print_object != Some(false)) {
                                            let y = self.pitch_to_y(pitch, &clef, 0.0);
                                            note_min = y - 5.0 + s_offset;
                                            note_max = y + 5.0 + s_offset;

                                            let is_stem_up = if let Some(st) = &n.stem {
                                                st == "up"
                                            } else {
                                                y >= 20.0
                                            };

                                            // Base stem clearance: 35px stem + 5px buffer.
                                            // For beamed notes: add 25px for beam slope shift
                                            // (draw_beam_group can push by up to min_stem_len=25px)
                                            // + up to 22px for extra beam levels (3 levels * 7.5px).
                                            let beam_extra = if !n.beams.is_empty() {
                                                let max_beam_level = n
                                                    .beams
                                                    .iter()
                                                    .map(|b| b.number)
                                                    .max()
                                                    .unwrap_or(1);
                                                25.0 + (max_beam_level.saturating_sub(1) as f32
                                                    * 7.5)
                                            } else {
                                                0.0
                                            };
                                            // (Not a stem of a beam spanning both staves.)
                                            let stem_clearance = if cross_beamed
                                                .contains(&(n as *const Note as usize))
                                            {
                                                0.0
                                            } else {
                                                40.0 + beam_extra
                                            };

                                            if is_stem_up {
                                                note_min =
                                                    note_min.min(y - stem_clearance + s_offset);
                                            } else {
                                                note_max =
                                                    note_max.max(y + stem_clearance + s_offset);
                                            }
                                        }

                                        // Precision lyric baseline: push below note bottom per verse
                                        if !n.lyrics.is_empty() {
                                            for lyric in &n.lyrics {
                                                let verse = lyric.number.unwrap_or(1) as f32;
                                                let lyric_base =
                                                    (s_offset + (s_num as f32) * staff_dist + 8.0)
                                                        .max(note_max + 8.0 + (verse - 1.0) * 15.0);
                                                note_max = note_max.max(lyric_base + 15.0);
                                            }
                                        }

                                        for h in &n.harmonies {
                                            let harm_span =
                                                if h.frame.is_some() { 90.0 } else { 35.0 };
                                            note_min = note_min.min(s_offset - harm_span);
                                        }

                                        for notation in &n.notations {
                                            match notation {
                                                Notation::Fermata { note_type, .. } => {
                                                    if note_type.as_deref() == Some("inverted") {
                                                        note_max = note_max
                                                            .max(s_offset + 40.0 + FERMATA_REACH);
                                                    } else {
                                                        note_min =
                                                            note_min.min(s_offset - FERMATA_REACH);
                                                    }
                                                }
                                                Notation::Tuplet { .. }
                                                | Notation::Articulation { .. } => {
                                                    note_min = note_min.min(s_offset - 15.0);
                                                    note_max = note_max.max(s_offset + 55.0);
                                                }
                                                Notation::Ornaments(ornaments) => {
                                                    for orn in ornaments {
                                                        match orn {
                                                            crate::models::Ornament::TrillMark | crate::models::Ornament::Turn | crate::models::Ornament::VerticalTurn => {
                                                                note_min = note_min.min(s_offset - 18.0);
                                                            }
                                                            _ => {}
                                                        }
                                                    }
                                                }
                                                _ => {}
                                            }
                                        }

                                        let bounds_map =
                                            onset_bounds.entry(m_idx).or_insert_with(HashMap::new);
                                        let entry = bounds_map
                                            .entry(current_time)
                                            .or_insert((f32::MAX, f32::MIN));
                                        entry.0 = entry.0.min(note_min);
                                        entry.1 = entry.1.max(note_max);

                                        min_rel = min_rel.min(note_min);
                                        max_rel = max_rel.max(note_max);

                                        if !n.is_chord {
                                            chord_time_pos = time_pos;
                                            time_pos += (n.duration as f64 * 10080.0
                                                / divisions as f64)
                                                .round()
                                                as i32;
                                        }
                                    }
                                    MeasureElement::Forward(d) => {
                                        time_pos +=
                                            (*d as f64 * 10080.0 / divisions as f64).round() as i32;
                                        chord_time_pos = time_pos;
                                    }
                                    MeasureElement::Backup(d) => {
                                        time_pos -=
                                            (*d as f64 * 10080.0 / divisions as f64).round() as i32;
                                        time_pos = time_pos.max(0);
                                        chord_time_pos = time_pos;
                                    }
                                    MeasureElement::Direction(dir) => {
                                        let is_below = dir.placement.as_deref() == Some("below");
                                        let s_num = dir.staff.unwrap_or(if is_below {
                                            current_num_staves_occ
                                        } else {
                                            1
                                        });
                                        let s_offset = (s_num - 1) as f32 * staff_dist;

                                        // Mirror draw_direction placement: below starts at 7.5 staff lines down
                                        let mut dir_y_rel = if is_below {
                                            s_offset + 7.5 * self.staff_line_distance
                                        } else {
                                            s_offset - 3.5 * self.staff_line_distance
                                        };

                                        // Collision-aware push: check existing note bounds at same onset
                                        if let Some(m_map) = onset_bounds.get(&m_idx) {
                                            if let Some(&(n_min, n_max)) = m_map.get(&time_pos) {
                                                if n_min < f32::MAX && n_max > f32::MIN {
                                                    if is_below {
                                                        dir_y_rel = dir_y_rel.max(n_max + 40.0);
                                                    } else {
                                                        dir_y_rel = dir_y_rel.min(n_min - 40.0);
                                                    }
                                                }
                                            }
                                        }

                                        let mut element_min = dir_y_rel;
                                        let mut element_max = dir_y_rel;

                                        for dt in &dir.types {
                                            match dt {
                                                crate::models::DirectionType::Dynamics(_) => {
                                                    if is_below {
                                                        element_max =
                                                            element_max.max(dir_y_rel + 20.0);
                                                    } else {
                                                        element_min =
                                                            element_min.min(dir_y_rel - 20.0);
                                                    }
                                                }
                                                crate::models::DirectionType::Words(_) => {
                                                    if is_below {
                                                        element_max =
                                                            element_max.max(dir_y_rel + 15.0);
                                                    } else {
                                                        element_min =
                                                            element_min.min(dir_y_rel - 15.0);
                                                    }
                                                }
                                                crate::models::DirectionType::Rehearsal(_) => {
                                                    let mut reh_y =
                                                        s_offset - 4.0 * self.staff_line_distance;
                                                    if let Some(m_map) = onset_bounds.get(&m_idx) {
                                                        if let Some(&(n_min, _)) =
                                                            m_map.get(&time_pos)
                                                        {
                                                            if n_min < f32::MAX {
                                                                reh_y = reh_y.min(n_min - 30.0);
                                                            }
                                                        }
                                                    }
                                                    element_min = element_min.min(reh_y - 25.0);
                                                }
                                                crate::models::DirectionType::Metronome(_) => {
                                                    let met_y = if is_below {
                                                        s_offset + 8.0 * self.staff_line_distance
                                                    } else {
                                                        s_offset - 4.5 * self.staff_line_distance
                                                    };
                                                    if is_below {
                                                        element_max = element_max.max(met_y + 25.0);
                                                    } else {
                                                        element_min = element_min.min(met_y - 25.0);
                                                    }
                                                }
                                                _ => {}
                                            }
                                        }

                                        let bounds_map =
                                            onset_bounds.entry(m_idx).or_insert_with(HashMap::new);
                                        let entry = bounds_map
                                            .entry(time_pos)
                                            .or_insert((f32::MAX, f32::MIN));
                                        entry.0 = entry.0.min(element_min);
                                        entry.1 = entry.1.max(element_max);

                                        min_rel = min_rel.min(element_min);
                                        max_rel = max_rel.max(element_max);
                                    }
                                    MeasureElement::Frame(_) => {
                                        min_rel = min_rel.min(-80.0);
                                        let bounds_map =
                                            onset_bounds.entry(m_idx).or_insert_with(HashMap::new);
                                        let entry = bounds_map
                                            .entry(time_pos)
                                            .or_insert((f32::MAX, f32::MIN));
                                        entry.0 = entry.0.min(-80.0);
                                    }
                                    MeasureElement::Harmony(h) => {
                                        let harm_span =
                                            if h.frame.is_some() { -90.0 } else { -30.0 };
                                        min_rel = min_rel.min(harm_span);
                                        let bounds_map =
                                            onset_bounds.entry(m_idx).or_insert_with(HashMap::new);
                                        let entry = bounds_map
                                            .entry(time_pos)
                                            .or_insert((f32::MAX, f32::MIN));
                                        entry.0 = entry.0.min(harm_span);
                                    }
                                    _ => {}
                                }
                            }
                        }

                        part_onset_bounds_list.push((
                            id.clone(),
                            onset_bounds,
                            part_bottom_max,
                            min_rel,
                            max_rel,
                        ));
                    }
                }
            }

            // Pass 2: Calculate Y-offsets for each part ensuring no vertical overlaps at overlapping X positions
            let mut prev_id: Option<String> = None;
            let mut prev_max_rel: Option<f32> = None;
            // Bottom line of the previous part's lowest staff, relative to its top.
            let mut prev_staff_bottom: Option<f32> = None;
            for (id, onset_bounds, part_bottom_max, min_rel, max_rel) in part_onset_bounds_list {
                let resolved_y;

                if let Some(ref p_id) = prev_id {
                    let prev_y = *part_y_offsets.get(p_id).unwrap();
                    let prev_bounds = part_onset_bounds_map.get(p_id).unwrap();

                    let mut required_push = self.part_distance;
                    let margin = 10.0; // Keep parts compact while preserving clearance

                    if let Some(prev_max) = prev_max_rel {
                        let full_height_push = prev_max + margin - min_rel;
                        required_push = required_push.max(full_height_push);
                    }
                    // Below a part of several staves, the same gap between staves
                    // as below a one-staff part (`part_distance` counts one staff).
                    if let Some(bottom) = prev_staff_bottom {
                        required_push = required_push
                            .max(bottom + self.part_distance - 4.0 * self.staff_line_distance);
                    }

                    for &m_idx in &system.measure_indices {
                        if let (Some(pb_measure), Some(cb_measure)) =
                            (prev_bounds.get(&m_idx), onset_bounds.get(&m_idx))
                        {
                            let spacing = &measure_spacings[m_idx];
                            // Estimate absolute content width for calculations
                            let m_width_actual = system.measure_widths[system
                                .measure_indices
                                .iter()
                                .position(|&x| x == m_idx)
                                .unwrap_or(0)];
                            let content_w = (m_width_actual - self.measure_start_padding).max(1.0);

                            for (&prev_t, &(_p_min, p_max)) in pb_measure {
                                let p_ratio =
                                    spacing.time_to_x.get(&prev_t).cloned().unwrap_or(0.0);
                                let p_x = p_ratio * content_w;

                                for (&curr_t, &(c_min, _c_max)) in cb_measure {
                                    let c_ratio =
                                        spacing.time_to_x.get(&curr_t).cloned().unwrap_or(0.0);
                                    let c_x = c_ratio * content_w;

                                    // If elements are horizontally close (within 25px)
                                    if (p_x - c_x).abs() < 40.0 {
                                        // Condition: resolved_y + c_min >= prev_y + p_max + margin
                                        let push = prev_y + p_max + margin - c_min - prev_y;
                                        required_push = required_push.max(push);
                                    }
                                }
                            }
                        }
                    }
                    resolved_y =
                        prev_y + required_push + extra_part_gap.get(&id).copied().unwrap_or(0.0);
                } else {
                    // First part starts at total_system_height
                    resolved_y = total_system_height + min_rel.min(0.0).abs();
                }

                part_y_offsets.insert(id.clone(), resolved_y);
                let visual_bottom = part_bottom_max.max(max_rel);
                part_bottom_offsets.insert(id.clone(), resolved_y + visual_bottom);
                part_staff_bottom_offsets.insert(id.clone(), resolved_y + part_bottom_max);
                part_onset_bounds_map.insert(id.clone(), onset_bounds);

                total_system_height = resolved_y + visual_bottom;
                prev_id = Some(id.clone());
                prev_max_rel = Some(max_rel);
                prev_staff_bottom = Some(part_bottom_max);
            }

            // Pre-calculate system-wide top protrusion to shift the whole system down
            let mut max_top_protrusion: f32 = 0.0;
            for item in &score.part_list {
                if let PartListItem::Part { id, .. } = item {
                    if let Some(part) = score.parts.iter().find(|p| p.id == *id) {
                        let staff_dist = *part_staff_distances.get(id).unwrap();
                        let state = part_states.get(id).unwrap();
                        let mut current_clefs_temp = state.current_clefs.clone();
                        let mut current_key_temp = state.current_key.clone();

                        for &m_idx in &system.measure_indices {
                            let m = &part.measures[m_idx];
                            if let Some(attr) = &m.attributes {
                                for c in &attr.clefs {
                                    current_clefs_temp.insert(c.number, c.clone());
                                }
                                if let Some(k) = &attr.key {
                                    current_key_temp = k.clone();
                                }
                            }
                            let part_y_offset_rel = *part_y_offsets.get(id).unwrap();

                            // Account for key signature symbols that extend above the staff top.
                            // Treble clef sharps use offsets [0.0, 1.5, -0.5, ...]: the 3rd sharp
                            // sits at -0.5 × sld above the top line. Without this, sparse scores
                            // with no high notes leave insufficient top margin, clipping the symbol.
                            {
                                let treble_sharps: [f32; 7] = [0.0, 1.5, -0.5, 1.0, 2.5, 0.5, 2.0];
                                let treble_flats: [f32; 7] = [2.0, 0.5, 2.5, 1.0, 3.0, 1.5, 3.5];
                                let bass_sharps: [f32; 7] = [1.0, 2.5, 0.5, 2.0, 3.5, 1.5, 3.0];
                                let bass_flats: [f32; 7] = [3.0, 1.5, 3.5, 2.0, 4.0, 2.5, 4.5];
                                let fifths = current_key_temp.fifths;
                                let n = fifths.unsigned_abs() as usize;
                                let is_sharp = fifths > 0;
                                let n_staves =
                                    current_clefs_temp.keys().max().copied().unwrap_or(1);
                                for s_num in 1..=n_staves {
                                    let s_offset = (s_num - 1) as f32 * staff_dist;
                                    let clef =
                                        current_clefs_temp.get(&s_num).cloned().unwrap_or(Clef {
                                            number: 1,
                                            sign: "G".to_string(),
                                            line: Some(2),
                                            ..Default::default()
                                        });
                                    let offsets: &[f32] = match clef.sign.as_str() {
                                        "F" => {
                                            if is_sharp {
                                                &bass_sharps
                                            } else {
                                                &bass_flats
                                            }
                                        }
                                        _ => {
                                            if is_sharp {
                                                &treble_sharps
                                            } else {
                                                &treble_flats
                                            }
                                        }
                                    };
                                    let min_offset = offsets[..n.min(offsets.len())]
                                        .iter()
                                        .cloned()
                                        .fold(f32::INFINITY, f32::min);
                                    if min_offset.is_finite() {
                                        // Glyph center is at min_offset * sld from the staff top.
                                        // dominant-baseline:central, font-size = 4*sld:
                                        //   sharps (♯): roughly symmetric → top ≈ center − 1.0×sld
                                        //   flats  (♭): long upward stem → top ≈ center − 2.0×sld
                                        // Even a flat at offset +0.5 can reach −1.5×sld (15 px)
                                        // above the staff top and be clipped when no high notes exist.
                                        let glyph_top_extend = if is_sharp {
                                            self.staff_line_distance
                                        } else {
                                            2.0 * self.staff_line_distance
                                        };
                                        let glyph_top = min_offset * self.staff_line_distance
                                            - glyph_top_extend;
                                        let protrusion =
                                            -(glyph_top - 5.0 + s_offset + part_y_offset_rel);
                                        if protrusion > 0.0 {
                                            max_top_protrusion = max_top_protrusion.max(protrusion);
                                        }
                                    }
                                }
                            }
                            for el in &m.elements {
                                match el {
                                    MeasureElement::Note(n) => {
                                        let s_num = n.staff.unwrap_or(1);
                                        let s_offset = (s_num - 1) as f32 * staff_dist;
                                        let clef = current_clefs_temp
                                            .get(&s_num)
                                            .cloned()
                                            .unwrap_or(Clef {
                                                number: 1,
                                                sign: "G".to_string(),
                                                line: Some(2),
                                                ..Default::default()
                                            });
                                        // TAB fret numbers sit on the string lines, not at the pitch's
                                        // staff position, and carry no stems; a hidden note draws nothing.
                                        if let Some(pitch) = Self::display_pitch(n).as_ref().filter(|_| clef.sign != "TAB" && n.print_object != Some(false)) {
                                            let y = self.pitch_to_y(pitch, &clef, 0.0);
                                            let clearance = notation_top_clearance(&n.notations);
                                            max_top_protrusion = max_top_protrusion.max(
                                                -(y - clearance + s_offset + part_y_offset_rel),
                                            );
                                        }
                                    }
                                    MeasureElement::Frame(_) => {
                                        max_top_protrusion =
                                            max_top_protrusion.max(-(part_y_offset_rel - 110.0));
                                    }
                                    MeasureElement::Harmony(h) => {
                                        if h.frame.is_some() {
                                            max_top_protrusion = max_top_protrusion
                                                .max(-(part_y_offset_rel - 120.0));
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                }
            }

            // Measure numbers are drawn at (part_start_y - 20.0) with font-size 12.
            // Each per-system SVG has viewBox starting at sys_y_before = current_y.
            // The text ascent is ~10–12 px, so the top of the glyph sits at
            //   part_start_y - 20 - 12  =  current_y + max_top_protrusion - 32
            // For this to be ≥ viewBox top (current_y) we need max_top_protrusion ≥ 32.
            {
                let first_part_offset = score
                    .part_list
                    .iter()
                    .find_map(|i| {
                        if let PartListItem::Part { id, .. } = i {
                            part_y_offsets.get(id).copied()
                        } else {
                            None
                        }
                    })
                    .unwrap_or(0.0);
                let needed = 32.0_f32 - first_part_offset;
                if needed > max_top_protrusion {
                    max_top_protrusion = needed;
                }
            }

            let system_render_y = current_y + max_top_protrusion;
            // The vertical-layout pass above draws (throwaway) beams relative to
            // y = 0; their stem tips must not re-anchor this system's slurs.
            self.beamed_stem_tips.borrow_mut().clear();
            self.placed_notes.borrow_mut().clear();
            self.pending_tuplets.borrow_mut().clear();
            self.placed_accidentals.borrow_mut().clear();
            self.placed_tuplets.borrow_mut().clear();
            self.placed_fingerings.borrow_mut().clear();
            self.pending_beam_marks.borrow_mut().clear();
            let mut measure_coords_in_system = Vec::new();
            let mut system_metadata_y = (0.0, 0.0);

            // Draw group symbols and calculate system group ranges for barlines
            let mut open_groups: HashMap<i32, (PartGroup, f32)> = HashMap::new();
            let mut current_system_group_ranges = Vec::new();
            for item in &score.part_list {
                match item {
                    PartListItem::Group(group) => {
                        if group.group_type == "start" {
                            open_groups.insert(group.number, (group.clone(), -1.0));
                        } else if let Some((start_g, start_y_rel)) =
                            open_groups.remove(&group.number)
                        {
                            let mut last_bottom = 0.0;
                            let mut in_group = false;
                            for it in &score.part_list {
                                match it {
                                    PartListItem::Group(g)
                                        if g.number == start_g.number
                                            && g.group_type == "start" =>
                                    {
                                        in_group = true
                                    }
                                    PartListItem::Group(g)
                                        if g.number == start_g.number && g.group_type == "stop" =>
                                    {
                                        break;
                                    }
                                    PartListItem::Part { id, .. } if in_group => {
                                        if let Some(&b) = part_staff_bottom_offsets.get(id) {
                                            last_bottom = b;
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            if start_y_rel >= 0.0 {
                                sys_doc = self.draw_group_symbol(
                                    sys_doc,
                                    &start_g,
                                    system_render_y + start_y_rel,
                                    system_render_y + last_bottom,
                                    current_margin_left,
                                );
                                if let Some(name) = &start_g.name {
                                    if sys_idx == 0
                                        || self.spacing_strategy != SpacingStrategy::Mobile
                                    {
                                        sys_doc = self.draw_group_name(
                                            sys_doc,
                                            name,
                                            system_render_y + start_y_rel,
                                            system_render_y + last_bottom,
                                            current_margin_left,
                                            sys_idx == 0,
                                        );
                                    }
                                }
                                current_system_group_ranges.push((
                                    start_g,
                                    system_render_y + start_y_rel,
                                    system_render_y + last_bottom,
                                ));
                            }
                        }
                    }
                    PartListItem::Part { id, .. } => {
                        if let Some(&y_rel) = part_y_offsets.get(id) {
                            for (_, start_y_rel) in open_groups.values_mut() {
                                if *start_y_rel < 0.0 {
                                    *start_y_rel = y_rel;
                                }
                            }
                        }
                    }
                }
            }

            // Draw system-start vertical line connecting all staves across all parts
            let first_part_id = match score
                .part_list
                .iter()
                .find(|i| matches!(i, PartListItem::Part { .. }))
            {
                Some(PartListItem::Part { id, .. }) => Some(id),
                _ => None,
            };

            if let Some(fid) = first_part_id {
                let first_y_offset = *part_y_offsets.get(fid).unwrap();
                let system_top = system_render_y + first_y_offset;

                // Calculate the actual bottom of the last part's staff
                let mut last_y_bottom: f32 = 0.0;
                for item in &score.part_list {
                    if let PartListItem::Part { id, .. } = item {
                        if let Some(&bottom_rel) = part_staff_bottom_offsets.get(id) {
                            last_y_bottom = last_y_bottom.max(bottom_rel);
                        }
                    }
                }
                let system_bottom = system_render_y + last_y_bottom;
                system_metadata_y = (system_top, system_bottom);
                sys_doc = sys_doc.add(
                    Line::new()
                        .set("x1", current_margin_left)
                        .set("y1", system_top)
                        .set("x2", current_margin_left)
                        .set("y2", system_bottom)
                        .set("stroke", "black")
                        .set("stroke-width", 1),
                );
            }
            let system_actual_width: f32 = system.measure_widths.iter().sum();
            let system_right_edge = current_margin_left + system_actual_width;

            for item in &score.part_list {
                if let PartListItem::Part {
                    id,
                    name,
                    abbreviation,
                    part_links: _,
                    name_display: _,
                    abbreviation_display: _,
                    ..
                } = item
                {
                    let part = score.parts.iter().find(|p| p.id == *id).unwrap();
                    let state = part_states.get_mut(id).unwrap();
                    let part_start_y = system_render_y + *part_y_offsets.get(id).unwrap();
                    self.current_part_y.set(part_start_y);
                    *self.current_part_id.borrow_mut() = id.clone();
                    let staff_dist = part_staff_distances
                        .get(id)
                        .cloned()
                        .unwrap_or(self.grand_staff_distance);

                    // Calculate system-wide lyric baseline
                    let mut system_max_reach: f32 = 40.0;
                    for &m_idx in &system.measure_indices {
                        if let Some(reach) = measure_max_reaches[m_idx].get(id) {
                            system_max_reach = system_max_reach.max(*reach);
                        }
                    }
                    state.lyric_baseline_y = system_max_reach;

                    let mut num_staves = state.num_staves;

                    let display_name = if sys_idx == 0 {
                        name.as_ref().or(abbreviation.as_ref())
                    } else {
                        abbreviation.as_ref().or(name.as_ref())
                    };
                    if let Some(n) = display_name {
                        if sys_idx == 0 || self.spacing_strategy != SpacingStrategy::Mobile {
                            let end_y =
                                part_start_y + (num_staves as f32 - 1.0) * staff_dist + 40.0;
                            sys_doc = self.draw_part_name(
                                sys_doc,
                                n,
                                part_start_y,
                                end_y,
                                current_margin_left,
                                sys_idx == 0,
                            );
                        }
                    }
                    for s in 0..num_staves {
                        let lines = *state.staff_lines.get(&(s + 1)).unwrap_or(&5);
                        sys_doc = self.draw_staff_lines(
                            sys_doc,
                            part_start_y + (s as f32 * staff_dist),
                            system_right_edge + self.margin_right,
                            lines,
                            current_margin_left,
                        );
                    }

                    let mut part_symbol = None;
                    if let Some(first) = part.measures.first() {
                        if let Some(attr) = &first.attributes {
                            part_symbol = attr.part_symbol.clone();
                        }
                    }
                    if part_symbol.is_none() && num_staves > 1 {
                        part_symbol = Some(crate::models::PartSymbolMark {
                            symbol: PartSymbol::Brace,
                            top_staff: None,
                            bottom_staff: None,
                        });
                    }

                    // (Relative Y of continued spanners is converted to absolute
                    // once, in the per-type loops below — converting here too
                    // added part_start_y twice.)

                    if let Some(ps) = part_symbol {
                        if ps.symbol != PartSymbol::None {
                            let top_s = ps.top_staff.unwrap_or(1);
                            let bottom_s = ps.bottom_staff.unwrap_or(num_staves);
                            let start_y = part_start_y + (top_s as f32 - 1.0) * staff_dist;
                            let end_y = part_start_y + (bottom_s as f32 - 1.0) * staff_dist + 40.0;

                            let mut temp_group = PartGroup::default();
                            temp_group.symbol = match ps.symbol {
                                PartSymbol::Brace => Some(GroupSymbol::Brace),
                                PartSymbol::Bracket => Some(GroupSymbol::Bracket),
                                _ => Some(GroupSymbol::Brace),
                            };
                            sys_doc = self.draw_group_symbol(
                                sys_doc,
                                &temp_group,
                                start_y,
                                end_y,
                                current_margin_left,
                            );
                        }
                    }

                    let mut measure_x = current_margin_left;
                    let mut measure_render_right_edge = measure_x;

                    // Resume spanners at system start
                    if let Some(e) = state.active_ending.as_mut() {
                        if e.is_continuation {
                            e.y = part_start_y + e.y;
                            e.x = current_margin_left;
                            sys_doc = self.draw_volta_bracket(
                                sys_doc,
                                e.x,
                                e.y,
                                e.x + 20.0,
                                e.y,
                                "",
                                false,
                                false,
                            );
                        }
                    }
                    for s in state.active_slurs.values_mut() {
                        if s.is_continuation {
                            // Stored relative to its own staff's top line, so it
                            // follows that staff when the staff gap changes.
                            s.y = part_start_y + (s.staff - 1).max(0) as f32 * staff_dist + s.y;
                            s.x = current_margin_left;
                            s.is_continuation = false;
                            s.resumed = true;
                        }
                    }
                    for t in state.active_ties.values_mut() {
                        if t.is_continuation {
                            t.y = part_start_y + t.y;
                            t.x = current_margin_left;
                            t.is_continuation = false;
                        }
                    }
                    for d in state.active_dashes.values_mut() {
                        if d.is_continuation {
                            d.y = part_start_y + d.y;
                            d.x = current_margin_left;
                            d.is_continuation = false;
                        }
                    }
                    for b in state.active_brackets.values_mut() {
                        if b.is_continuation {
                            b.y = part_start_y + b.y;
                            b.x = current_margin_left;
                            b.is_continuation = false;
                        }
                    }
                    for l in state.active_lyrics.values_mut() {
                        if l.is_continuation {
                            l.y = part_start_y + l.y;
                            l.x = current_margin_left;
                            l.is_continuation = false;
                        }
                    }
                    for o in state.active_octave_shifts.values_mut() {
                        if o.is_continuation {
                            // Stored relative to its own staff (the gap to the
                            // staff above may differ on this system).
                            let staff_top = part_start_y
                                + (o.staff.unwrap_or(1) - 1) as f32 * staff_dist;
                            o.y = staff_top + o.y;
                            o.x = current_margin_left;
                            o.is_continuation = false;
                        }
                    }
                    for p in state.active_pedals.values_mut() {
                        if p.is_continuation {
                            // Restart from this system's own pedal line: the
                            // previous system's height (and staff gap) don't
                            // apply here; each measure pushes it clear again.
                            p.y = if p.is_below {
                                part_start_y
                                    + (state.num_staves as f32 - 1.0) * staff_dist
                                    + 10.0 * self.staff_line_distance
                            } else {
                                part_start_y - 4.0 * self.staff_line_distance
                            };
                            p.x = current_margin_left;
                            p.is_continuation = false;
                        } else {
                            p.x = current_margin_left;
                        }
                    }
                    for w in state.active_wedges.values_mut() {
                        if w.is_continuation {
                            w.y = part_start_y + w.y;
                            w.x = current_margin_left;
                            w.is_continuation = false;
                        }
                    }
                    for w in state.active_wavy_lines.values_mut() {
                        if w.is_continuation {
                            w.y = part_start_y + w.y;
                            w.x = current_margin_left;
                            w.is_continuation = false;
                        }
                    }
                    for t in state.active_tremolos.values_mut() {
                        if t.is_continuation {
                            t.y = part_start_y + t.y;
                            t.x = current_margin_left;
                            t.is_continuation = false;
                        }
                    }
                    for g in state.active_glissandi.values_mut() {
                        if g.is_continuation {
                            g.y = part_start_y + g.y;
                            g.x = current_margin_left;
                            g.is_continuation = false;
                        }
                    }
                    for s in state.active_slides.values_mut() {
                        if s.is_continuation {
                            s.y = part_start_y + s.y;
                            s.x = current_margin_left;
                            s.is_continuation = false;
                        }
                    }

                    // Lyrics are drawn once the whole system is laid out, each verse
                    // on one line: below the lowest point any measure needs, not
                    // dipping under each low note.
                    let mut pending_lyrics: Vec<(crate::models::Lyric, f32, i32)> = Vec::new();
                    let mut system_lyric_baselines: HashMap<i32, f32> = HashMap::new();

                    for (sys_m_idx, (&m_idx, &m_width)) in system
                        .measure_indices
                        .iter()
                        .zip(system.measure_widths.iter())
                        .enumerate()
                    {
                        if m_width <= 0.0 {
                            // If this was part of a multi-rest, decrement count even if we skip rendering
                            if state.multiple_rest_remaining > 0 {
                                state.multiple_rest_remaining -= 1;
                            }
                            continue;
                        }
                        let measure = &part.measures[m_idx];
                        // Show measure numbers only at the beginning of each system.
                        if m_width > 0.0 && id == &score.parts[0].id && sys_m_idx == 0 {
                            let text = Text::new((m_idx + 1).to_string())
                                .set("x", measure_x)
                                .set("y", part_start_y - 20.0)
                                .set("font-size", 12)
                                .set("font-family", "serif")
                                .set("font-weight", "bold")
                                .set("fill", "black");
                            sys_doc = sys_doc.add(text);
                        }
                        if m_width > 0.0 && (m_idx == 0 || m_idx == system.measure_indices[0]) {
                            for (group, g_start_y, g_end_y) in &current_system_group_ranges {
                                if group.barline.as_deref() == Some("yes")
                                    && (part_start_y - *g_start_y).abs() < 1.0
                                {
                                    sys_doc = sys_doc.add(
                                        Line::new()
                                            .set("x1", measure_x)
                                            .set("y1", *g_start_y)
                                            .set("x2", measure_x)
                                            .set("y2", *g_end_y)
                                            .set("stroke", "black")
                                            .set("stroke-width", 1),
                                    );
                                }
                            }
                        }

                        let mut has_right_barline = false;
                        let mut stop_beat_repeat_after_this = false;

                        // A multi-measure rest starts here: set it up before choosing
                        // how to draw the measure, so this measure is drawn as the
                        // rest and exactly `count` measures are consumed by it.
                        if state.multiple_rest_remaining == 0 {
                            let count = measure
                                .attributes
                                .iter()
                                .chain(measure.elements.iter().filter_map(|el| match el {
                                    MeasureElement::Attributes(a) => Some(a),
                                    _ => None,
                                }))
                                .find_map(|a| a.multiple_rest);
                            if let Some(count) = count.filter(|&c| c > 0) {
                                state.multiple_rest_remaining = count;
                                state.multiple_rest_total = count;
                            }
                        }

                        if state.measure_repeat_active {
                            let center_x = measure_x + m_width * 0.5;
                            let center_y = part_start_y + 2.0 * self.staff_line_distance;
                            sys_doc = sys_doc.add(
                                Text::new("\u{E500}")
                                    .set("x", center_x)
                                    .set("y", center_y)
                                    .set("font-size", self.staff_line_distance * 4.0)
                                    .set("text-anchor", "middle")
                                    .set("dominant-baseline", "central")
                                    .set("font-family", self.font_family.as_str()),
                            );
                        } else if state.multiple_rest_remaining > 0 {
                            if state.multiple_rest_remaining == state.multiple_rest_total
                                && id == &score.parts[0].id
                            {
                                // Let the playback cursor move across the rest: each
                                // measure it stands for gets an equal share of it,
                                // with its beats spread evenly.
                                let total = state.multiple_rest_total.max(1);
                                let share = m_width / total as f32;
                                let beats = state.current_beats.max(1);
                                let beat_units = 10080 * 4 / state.current_beat_type.max(1);
                                let time_map: HashMap<i32, f32> = (0..beats)
                                    .map(|b| (b * beat_units, b as f32 / beats as f32))
                                    .collect();
                                for k in 0..total {
                                    let mi = m_idx + k as usize;
                                    if mi >= part.measures.len() {
                                        break;
                                    }
                                    measure_coords_in_system.push((
                                        mi,
                                        measure_x + share * k as f32,
                                        share,
                                        state.current_beats,
                                        state.current_beat_type,
                                        state.divisions,
                                        time_map.clone(),
                                    ));
                                }
                            }
                            if state.multiple_rest_remaining == state.multiple_rest_total {
                              // On every staff of the part; the count only above the first.
                              for staff_i in 0..state.num_staves.max(1) {
                                let hbar_width = 40.0;
                                let center_x = measure_x + m_width * 0.5;
                                let center_y = part_start_y
                                    + staff_i as f32 * staff_dist
                                    + 2.0 * self.staff_line_distance;

                                // Draw robust H-bar using Path for better scaling
                                let x1 = center_x - hbar_width * 0.5;
                                let x2 = center_x + hbar_width * 0.5;
                                let hbar_data = Data::new()
                                    .move_to((x1, center_y - 5.0))
                                    .line_to((x1, center_y + 5.0)) // left vertical
                                    .move_to((x1, center_y))
                                    .line_to((x2, center_y)) // horizontal
                                    .move_to((x2, center_y - 5.0))
                                    .line_to((x2, center_y + 5.0)); // right vertical

                                sys_doc = sys_doc.add(
                                    Path::new()
                                        .set("fill", "none")
                                        .set("stroke", "black")
                                        .set("stroke-width", 2.5)
                                        .set("d", hbar_data),
                                );

                                if staff_i == 0 {
                                    sys_doc = sys_doc.add(
                                        Text::new(state.multiple_rest_total.to_string())
                                            .set("x", center_x)
                                            .set("y", center_y - 2.5 * self.staff_line_distance)
                                            .set("font-size", 14)
                                            .set("text-anchor", "middle")
                                            .set("font-family", "serif")
                                            .set("font-weight", "bold"),
                                    );
                                }
                              }
                            }
                            state.multiple_rest_remaining -= 1;
                        } else if m_width > 0.0 {
                            let measure_total_dur = measure_total_durs[m_idx];
                            let measure_theoretical_dur = measure_theoretical_durs[m_idx];
                            let measure_time_map = &measure_spacings[m_idx].time_to_x;
                            let is_first_in_system = sys_m_idx == 0;

                            let prev_key = state.current_key.clone();
                            // Update state with explicit measure attributes if they exist
                            if let Some(attr) = &measure.attributes {
                                for c in &attr.clefs {
                                    state.current_clefs.insert(c.number, c.clone());
                                }
                                if let Some(k) = &attr.key {
                                    state.current_key = k.clone();
                                }
                                if let Some(t) = &attr.time {
                                    if let Ok(b) =
                                        t.beats.split('+').next().unwrap_or("4").parse::<i32>()
                                    {
                                        state.current_beats = b;
                                    }
                                    state.current_beat_type = t.beat_type;
                                }
                                if let Some(d) = attr.divisions {
                                    state.divisions = d;
                                }
                            }
                            let measure_staff_voices =
                                Self::analyze_measure_voices(measure, state.divisions);

                            // Force attributes at system start OR use explicit measure attributes
                            let mut attr_to_draw = measure
                                .attributes
                                .as_ref()
                                .map(|a| Self::with_key_cancel(a, Some(&prev_key)));
                            if is_first_in_system {
                                let mut attr = attr_to_draw.unwrap_or_default();
                                if attr.clefs.is_empty() {
                                    attr.clefs = state.current_clefs.values().cloned().collect();
                                }
                                if attr.key.is_none() {
                                    attr.key = Some(state.current_key.clone());
                                }
                                if attr.time.is_none() {
                                    attr.time = Some(crate::models::Time {
                                        beats: state.current_beats.to_string(),
                                        beat_type: state.current_beat_type,
                                    });
                                }
                                attr_to_draw = Some(attr);
                            }

                            let has_left_forward_repeat = measure.elements.iter().any(|el| {
                                if let MeasureElement::Barline(bl) = el {
                                    bl.location == "left"
                                        && bl
                                            .repeat
                                            .as_ref()
                                            .map(|r| r.direction == "forward")
                                            .unwrap_or(false)
                                } else {
                                    false
                                }
                            });

                            let (new_doc, initial_attr_width_shared) =
                                if let Some(attr) = &attr_to_draw {
                                    self.draw_attributes(
                                        sys_doc,
                                        attr,
                                        part_start_y,
                                        measure_x,
                                        &state.current_clefs,
                                        num_staves,
                                        staff_dist,
                                        &state.staff_lines,
                                        has_left_forward_repeat,
                                    )
                                } else if has_left_forward_repeat {
                                    // No attributes, but the repeat dots still need clearance
                                    // (same 12px draw_attributes adds for them).
                                    (sys_doc, 12.0)
                                } else {
                                    (sys_doc, 0.0)
                                };
                            sys_doc = new_doc;

                            // Must mirror analyze_measures, which sizes each measure as
                            // measure_start_padding + attribute width + note content.
                            let content_start_x =
                                measure_x + initial_attr_width_shared + self.measure_start_padding;
                            let content_width =
                                m_width - initial_attr_width_shared - self.measure_start_padding;
                            // A measure squeezed below its natural width moves its
                            // notes closer; grace notes (spaced in fixed px before
                            // their onset) must close up the same way. Only mildly:
                            // a large ratio comes from width bookkeeping (a first
                            // measure's system-start attributes), not real squeezing.
                            let natural_w = measure_spacings[m_idx]._total_content_width;
                            self.current_measure_idx.set(m_idx);
                            self.grace_scale.set(if natural_w > 0.0 {
                                (content_width / natural_w).clamp(0.85, 1.0)
                            } else {
                                1.0
                            });

                            if id == &score.parts[0].id {
                                measure_coords_in_system.push((
                                    m_idx,
                                    content_start_x,
                                    content_width,
                                    state.current_beats,
                                    state.current_beat_type,
                                    state.divisions,
                                    measure_spacings[m_idx].time_to_x.clone(),
                                ));
                            }

                            let mut measure_occupancy = HashMap::new();
                            // The measuring pass below also records notes and
                            // finished tuplets; only the real pass's may stay.
                            let placed_notes_len = self.placed_notes.borrow().len();
                            let pending_tuplets_len = self.pending_tuplets.borrow().len();
                            let pending_beam_marks_len = self.pending_beam_marks.borrow().len();
                            let placed_accidentals_len = self.placed_accidentals.borrow().len();
                            let placed_tuplets_len = self.placed_tuplets.borrow().len();
                            let placed_fingerings_len = self.placed_fingerings.borrow().len();
                            let mut temp_time_pos = 0;
                            let mut temp_chord_group = Vec::new();
                            let mut temp_group_time_pos = 0;
                            // Keyed by (voice, beam-level number) — see active_beams below for why.
                            let mut temp_active_beams: HashMap<
                                (i32, i32),
                                Vec<(StemInfo, Vec<crate::models::Beam>)>,
                            > = HashMap::new();
                            let mut temp_active_slurs = state.active_slurs.clone();
                            let mut temp_active_ties = state.active_ties.clone();
                            let mut temp_active_tuplets = state.active_tuplets.clone();
                            let mut temp_active_brackets = state.active_brackets.clone();
                            let mut temp_active_dashes = state.active_dashes.clone();
                            let mut temp_active_wedges = state.active_wedges.clone();
                            let mut temp_active_glissandi = state.active_glissandi.clone();
                            let mut temp_slash_active = state.slash_active;
                            let mut temp_slash_use_stems = state.slash_use_stems;
                            let mut temp_slash_dots = state.slash_dots;
                            let mut temp_slash_note_type = state.slash_note_type.clone();
                            let mut temp_beat_repeat_active = state.beat_repeat_active;
                            let mut _temp_beat_repeat_first = state.beat_repeat_first;
                            let mut temp_beat_repeat_slashes = state.beat_repeat_slashes;
                            let mut temp_active_octave_shifts = state.active_octave_shifts.clone();
                            let mut temp_active_pedals = state.active_pedals.clone();
                            let mut temp_beats = state.current_beats;
                            let mut temp_divisions = state.divisions;
                            let mut temp_staff_lines = state.staff_lines.clone();
                            let mut used_grace_widths_temp: HashMap<i32, f32> = HashMap::new();

                            let elements = &measure.elements;
                            for (el_idx, element) in elements.iter().enumerate() {
                                match element {
                                    MeasureElement::Sound(_) => {}
                                    MeasureElement::Note(note) => {
                                        if note.is_chord {
                                            temp_chord_group.push(note);
                                        } else {
                                            if !temp_chord_group.is_empty() {
                                                let grace_offset =
                                                    if temp_chord_group[0].grace.is_some() {
                                                        let total_grace_w = *measure_spacings
                                                            [m_idx]
                                                            .grace_widths
                                                            .get(&temp_group_time_pos)
                                                            .unwrap_or(&0.0);
                                                        let used = used_grace_widths_temp
                                                            .entry(temp_group_time_pos)
                                                            .or_insert(0.0);
                                                        let current_note_w = temp_chord_group
                                                            .iter()
                                                            .map(|n| self.estimate_grace_spacing(n))
                                                            .fold(0.0, f32::max);
                                                        let offset = -(total_grace_w - *used);
                                                        *used += current_note_w;
                                                        offset
                                                    } else {
                                                        0.0
                                                    };
                                                let (_, min, max, _) = self.process_chord_group(
                                                    Document::new(),
                                                    &temp_chord_group,
                                                    &state.current_clefs,
                                                    part_start_y,
                                                    0.0,
                                                    0.0,
                                                    measure_time_map,
                                                    measure_total_dur,
                                                    measure_theoretical_dur,
                                                    temp_group_time_pos,
                                                    &mut &mut temp_active_beams,
                                                    &mut temp_active_slurs,
                                                    &mut temp_active_ties,
                                                    &mut temp_active_tuplets,
                                                    &mut state.active_lyrics.clone(),
                                                    &mut state.active_glissandi.clone(),
                                                    &mut state.active_slides.clone(),
                                                    &mut state.active_hammer_ons.clone(),
                                                    &mut state.active_pull_offs.clone(),
                                                    &mut state.active_tremolos.clone(),
                                                    &mut state.active_wavy_lines.clone(),
                                                    &temp_active_octave_shifts,
                                                    &mut Vec::new(),
                                                    staff_dist,
                                                    temp_slash_active,
                                                    temp_slash_use_stems,
                                                    temp_slash_dots,
                                                    temp_slash_note_type.as_deref(),
                                                    temp_beat_repeat_active,
                                                    temp_beat_repeat_slashes,
                                                    temp_beats,
                                                    temp_divisions,
                                                    0.0,
                                                    &temp_staff_lines,
                                                    0.0,
                                                    grace_offset,
                                                    &measure_staff_voices,
                                                );
                                                let entry = measure_occupancy
                                                    .entry(temp_group_time_pos)
                                                    .or_insert((f32::MAX, f32::MIN));
                                                entry.0 = entry.0.min(min - part_start_y);
                                                entry.1 = entry.1.max(max - part_start_y);
                                                if temp_beat_repeat_active {
                                                    _temp_beat_repeat_first = false;
                                                }
                                            }
                                            temp_chord_group.clear();
                                            temp_chord_group.push(note);
                                            temp_group_time_pos = temp_time_pos;
                                            temp_time_pos += (note.duration as f64 * 10080.0
                                                / temp_divisions as f64)
                                                .round()
                                                as i32;
                                        }
                                    }
                                    MeasureElement::Backup(d) => {
                                        if !temp_chord_group.is_empty() {
                                            let grace_offset =
                                                if temp_chord_group[0].grace.is_some() {
                                                    let total_grace_w = *measure_spacings[m_idx]
                                                        .grace_widths
                                                        .get(&temp_group_time_pos)
                                                        .unwrap_or(&0.0);
                                                    let used = used_grace_widths_temp
                                                        .entry(temp_group_time_pos)
                                                        .or_insert(0.0);
                                                    let current_note_w = temp_chord_group
                                                        .iter()
                                                        .map(|n| self.estimate_grace_spacing(n))
                                                        .fold(0.0, f32::max);
                                                    let offset = -(total_grace_w - *used);
                                                    *used += current_note_w;
                                                    offset
                                                } else {
                                                    0.0
                                                };
                                            let (_, min, max, _) = self.process_chord_group(
                                                Document::new(),
                                                &temp_chord_group,
                                                &state.current_clefs,
                                                part_start_y,
                                                0.0,
                                                0.0,
                                                measure_time_map,
                                                measure_total_dur,
                                                measure_theoretical_dur,
                                                temp_group_time_pos,
                                                &mut &mut temp_active_beams,
                                                &mut temp_active_slurs,
                                                &mut temp_active_ties,
                                                &mut temp_active_tuplets,
                                                &mut state.active_lyrics.clone(),
                                                &mut state.active_glissandi.clone(),
                                                &mut state.active_slides.clone(),
                                                &mut state.active_hammer_ons.clone(),
                                                &mut state.active_pull_offs.clone(),
                                                &mut state.active_tremolos.clone(),
                                                &mut state.active_wavy_lines.clone(),
                                                &temp_active_octave_shifts,
                                                &mut Vec::new(),
                                                staff_dist,
                                                temp_slash_active,
                                                temp_slash_use_stems,
                                                temp_slash_dots,
                                                temp_slash_note_type.as_deref(),
                                                temp_beat_repeat_active,
                                                temp_beat_repeat_slashes,
                                                temp_beats,
                                                temp_divisions,
                                                0.0,
                                                &temp_staff_lines,
                                                0.0,
                                                grace_offset,
                                                &measure_staff_voices,
                                            );
                                            let entry = measure_occupancy
                                                .entry(temp_group_time_pos)
                                                .or_insert((f32::MAX, f32::MIN));
                                            entry.0 = entry.0.min(min - part_start_y);
                                            entry.1 = entry.1.max(max - part_start_y);
                                            if temp_beat_repeat_active {
                                                _temp_beat_repeat_first = false;
                                            }
                                            temp_chord_group.clear();
                                        }
                                        temp_time_pos -=
                                            (*d as f64 * 10080.0 / temp_divisions as f64).round()
                                                as i32;
                                    }
                                    MeasureElement::Forward(d) => {
                                        if !temp_chord_group.is_empty() {
                                            let grace_offset =
                                                if temp_chord_group[0].grace.is_some() {
                                                    let total_grace_w = *measure_spacings[m_idx]
                                                        .grace_widths
                                                        .get(&temp_group_time_pos)
                                                        .unwrap_or(&0.0);
                                                    let used = used_grace_widths_temp
                                                        .entry(temp_group_time_pos)
                                                        .or_insert(0.0);
                                                    let current_note_w = temp_chord_group
                                                        .iter()
                                                        .map(|n| self.estimate_grace_spacing(n))
                                                        .fold(0.0, f32::max);
                                                    let offset = -(total_grace_w - *used);
                                                    *used += current_note_w;
                                                    offset
                                                } else {
                                                    0.0
                                                };
                                            let (_, min, max, _) = self.process_chord_group(
                                                Document::new(),
                                                &temp_chord_group,
                                                &state.current_clefs,
                                                part_start_y,
                                                0.0,
                                                0.0,
                                                measure_time_map,
                                                measure_total_dur,
                                                measure_theoretical_dur,
                                                temp_group_time_pos,
                                                &mut &mut temp_active_beams,
                                                &mut temp_active_slurs,
                                                &mut temp_active_ties,
                                                &mut temp_active_tuplets,
                                                &mut state.active_lyrics.clone(),
                                                &mut state.active_glissandi.clone(),
                                                &mut state.active_slides.clone(),
                                                &mut state.active_hammer_ons.clone(),
                                                &mut state.active_pull_offs.clone(),
                                                &mut state.active_tremolos.clone(),
                                                &mut state.active_wavy_lines.clone(),
                                                &temp_active_octave_shifts,
                                                &mut Vec::new(),
                                                staff_dist,
                                                temp_slash_active,
                                                temp_slash_use_stems,
                                                temp_slash_dots,
                                                temp_slash_note_type.as_deref(),
                                                temp_beat_repeat_active,
                                                temp_beat_repeat_slashes,
                                                temp_beats,
                                                temp_divisions,
                                                0.0,
                                                &temp_staff_lines,
                                                0.0,
                                                grace_offset,
                                                &measure_staff_voices,
                                            );
                                            let entry = measure_occupancy
                                                .entry(temp_group_time_pos)
                                                .or_insert((f32::MAX, f32::MIN));
                                            entry.0 = entry.0.min(min - part_start_y);
                                            entry.1 = entry.1.max(max - part_start_y);
                                            if temp_beat_repeat_active {
                                                _temp_beat_repeat_first = false;
                                            }
                                            temp_chord_group.clear();
                                        }
                                        temp_time_pos +=
                                            (*d as f64 * 10080.0 / temp_divisions as f64).round()
                                                as i32;
                                    }
                                    MeasureElement::Attributes(attr) => {
                                        let mut is_mid = false;
                                        for i in (el_idx + 1)..elements.len() {
                                            if let MeasureElement::Note(n) = &elements[i] {
                                                if n.is_chord {
                                                    is_mid = true;
                                                }
                                                break;
                                            }
                                            if let MeasureElement::Forward(_)
                                            | MeasureElement::Backup(_) = &elements[i]
                                            {
                                                break;
                                            }
                                        }
                                        if !is_mid && !temp_chord_group.is_empty() {
                                            let grace_offset =
                                                if temp_chord_group[0].grace.is_some() {
                                                    let total_grace_w = *measure_spacings[m_idx]
                                                        .grace_widths
                                                        .get(&temp_group_time_pos)
                                                        .unwrap_or(&0.0);
                                                    let used = used_grace_widths_temp
                                                        .entry(temp_group_time_pos)
                                                        .or_insert(0.0);
                                                    let current_note_w = temp_chord_group
                                                        .iter()
                                                        .map(|n| self.estimate_grace_spacing(n))
                                                        .fold(0.0, f32::max);
                                                    let offset = -(total_grace_w - *used);
                                                    *used += current_note_w;
                                                    offset
                                                } else {
                                                    0.0
                                                };
                                            let (_, min, max, _) = self.process_chord_group(
                                                Document::new(),
                                                &temp_chord_group,
                                                &state.current_clefs,
                                                part_start_y,
                                                0.0,
                                                0.0,
                                                measure_time_map,
                                                measure_total_dur,
                                                measure_theoretical_dur,
                                                temp_group_time_pos,
                                                &mut &mut temp_active_beams,
                                                &mut temp_active_slurs,
                                                &mut temp_active_ties,
                                                &mut temp_active_tuplets,
                                                &mut state.active_lyrics.clone(),
                                                &mut state.active_glissandi.clone(),
                                                &mut state.active_slides.clone(),
                                                &mut state.active_hammer_ons.clone(),
                                                &mut state.active_pull_offs.clone(),
                                                &mut state.active_tremolos.clone(),
                                                &mut state.active_wavy_lines.clone(),
                                                &temp_active_octave_shifts,
                                                &mut Vec::new(),
                                                staff_dist,
                                                temp_slash_active,
                                                temp_slash_use_stems,
                                                temp_slash_dots,
                                                temp_slash_note_type.as_deref(),
                                                temp_beat_repeat_active,
                                                temp_beat_repeat_slashes,
                                                temp_beats,
                                                temp_divisions,
                                                0.0,
                                                &temp_staff_lines,
                                                0.0,
                                                grace_offset,
                                                &measure_staff_voices,
                                            );
                                            // Merge: other staves/voices share this onset.
                                            let entry = measure_occupancy
                                                .entry(temp_group_time_pos)
                                                .or_insert((f32::MAX, f32::MIN));
                                            entry.0 = entry.0.min(min - part_start_y);
                                            entry.1 = entry.1.max(max - part_start_y);
                                            if temp_beat_repeat_active {
                                                _temp_beat_repeat_first = false;
                                            }
                                            temp_chord_group.clear();
                                        }
                                        if let Some(n) = attr.staves {
                                            num_staves = n;
                                            state.num_staves = n;
                                        }
                                        if let Some(d) = attr.divisions {
                                            temp_divisions = d;
                                        }
                                        for sd in &attr.staff_details {
                                            if let Some(lines) = sd.staff_lines {
                                                temp_staff_lines.insert(sd.number, lines);
                                            }
                                        }
                                        for clef in &attr.clefs {
                                            state.current_clefs.insert(clef.number, clef.clone());
                                        }
                                        if let Some(d) = attr.divisions {
                                            temp_divisions = d;
                                        }
                                        if let Some(sl) = &attr.slash {
                                            if sl.slash_type == "start" {
                                                temp_slash_active = true;
                                                temp_slash_use_stems =
                                                    sl.use_stems.unwrap_or(false);
                                                temp_slash_dots = sl.dots;
                                                temp_slash_note_type = sl.note_type.clone();
                                            } else {
                                                temp_slash_active = false;
                                            }
                                        }
                                        if let Some(br) = &attr.beat_repeat {
                                            if br.repeat_type == "start" {
                                                temp_beat_repeat_active = true;
                                                temp_beat_repeat_slashes = br.slashes;
                                                _temp_beat_repeat_first = true;
                                            } else {
                                                temp_beat_repeat_active = false;
                                                _temp_beat_repeat_first = false;
                                            }
                                        }
                                        if let Some(time) = &attr.time {
                                            if let Ok(b) = time
                                                .beats
                                                .split('+')
                                                .next()
                                                .unwrap_or("4")
                                                .parse::<i32>()
                                            {
                                                temp_beats = b;
                                            }
                                        }
                                    }
                                    MeasureElement::Direction(dir) => {
                                        let mut is_mid = false;
                                        for i in (el_idx + 1)..elements.len() {
                                            if let MeasureElement::Note(n) = &elements[i] {
                                                if n.is_chord {
                                                    is_mid = true;
                                                }
                                                break;
                                            }
                                            if let MeasureElement::Forward(_)
                                            | MeasureElement::Backup(_) = &elements[i]
                                            {
                                                break;
                                            }
                                        }
                                        if !is_mid && !temp_chord_group.is_empty() {
                                            let grace_offset =
                                                if temp_chord_group[0].grace.is_some() {
                                                    let total_grace_w = *measure_spacings[m_idx]
                                                        .grace_widths
                                                        .get(&temp_group_time_pos)
                                                        .unwrap_or(&0.0);
                                                    let used = used_grace_widths_temp
                                                        .entry(temp_group_time_pos)
                                                        .or_insert(0.0);
                                                    let current_note_w = temp_chord_group
                                                        .iter()
                                                        .map(|n| self.estimate_grace_spacing(n))
                                                        .fold(0.0, f32::max);
                                                    let offset = -(total_grace_w - *used);
                                                    *used += current_note_w;
                                                    offset
                                                } else {
                                                    0.0
                                                };
                                            let (_, min, max, _) = self.process_chord_group(
                                                Document::new(),
                                                &temp_chord_group,
                                                &state.current_clefs,
                                                part_start_y,
                                                0.0,
                                                0.0,
                                                measure_time_map,
                                                measure_total_dur,
                                                measure_theoretical_dur,
                                                temp_group_time_pos,
                                                &mut &mut temp_active_beams,
                                                &mut temp_active_slurs,
                                                &mut temp_active_ties,
                                                &mut temp_active_tuplets,
                                                &mut state.active_lyrics.clone(),
                                                &mut state.active_glissandi.clone(),
                                                &mut state.active_slides.clone(),
                                                &mut state.active_hammer_ons.clone(),
                                                &mut state.active_pull_offs.clone(),
                                                &mut state.active_tremolos.clone(),
                                                &mut state.active_wavy_lines.clone(),
                                                &temp_active_octave_shifts,
                                                &mut Vec::new(),
                                                staff_dist,
                                                temp_slash_active,
                                                temp_slash_use_stems,
                                                temp_slash_dots,
                                                temp_slash_note_type.as_deref(),
                                                temp_beat_repeat_active,
                                                temp_beat_repeat_slashes,
                                                temp_beats,
                                                temp_divisions,
                                                0.0,
                                                &temp_staff_lines,
                                                0.0,
                                                grace_offset,
                                                &measure_staff_voices,
                                            );
                                            // Merge: other staves/voices share this onset.
                                            let entry = measure_occupancy
                                                .entry(temp_group_time_pos)
                                                .or_insert((f32::MAX, f32::MIN));
                                            entry.0 = entry.0.min(min - part_start_y);
                                            entry.1 = entry.1.max(max - part_start_y);
                                            if temp_beat_repeat_active {
                                                _temp_beat_repeat_first = false;
                                            }
                                            temp_chord_group.clear();
                                        }
                                        // Measurement-only pass (throwaway doc, x = 0):
                                        // discard anything it records for the real
                                        // drawing (queued pedals, placed dynamics/hairpins).
                                        let recorded = (
                                            self.deferred_pedals.borrow().len(),
                                            self.placed_dynamics.borrow().len(),
                                            self.placed_wedges.borrow().len(),
                                            self.placed_text_boxes.borrow().len(),
                                            self.gap_items.borrow().len(),
                                        );
                                        self.draw_direction(
                                            Document::new(),
                                            dir,
                                            part_start_y,
                                            0.0,
                                            num_staves,
                                            None,
                                            staff_dist,
                                            &mut temp_active_brackets,
                                            &mut temp_active_dashes,
                                            &mut temp_active_wedges,
                                            &mut temp_active_glissandi,
                                            &mut temp_active_octave_shifts,
                                            &mut temp_active_pedals,
                                            None,
                                        );
                                        self.deferred_pedals.borrow_mut().truncate(recorded.0);
                                        self.placed_dynamics.borrow_mut().truncate(recorded.1);
                                        self.placed_wedges.borrow_mut().truncate(recorded.2);
                                        self.placed_text_boxes.borrow_mut().truncate(recorded.3);
                                        self.gap_items.borrow_mut().truncate(recorded.4);
                                    }
                                    MeasureElement::FiguredBass(_)
                                    | MeasureElement::Harmony(_)
                                    | MeasureElement::Bookmark(_)
                                    | MeasureElement::Grouping(_) => {}
                                    _ => {}
                                }
                            }
                            if !temp_chord_group.is_empty() {
                                let grace_offset = if temp_chord_group[0].grace.is_some() {
                                    let total_grace_w = *measure_spacings[m_idx]
                                        .grace_widths
                                        .get(&temp_group_time_pos)
                                        .unwrap_or(&0.0);
                                    let used = used_grace_widths_temp
                                        .entry(temp_group_time_pos)
                                        .or_insert(0.0);
                                    let current_note_w = temp_chord_group
                                        .iter()
                                        .map(|n| self.estimate_grace_spacing(n))
                                        .fold(0.0, f32::max);
                                    let offset = -(total_grace_w - *used);
                                    *used += current_note_w;
                                    offset
                                } else {
                                    0.0
                                };
                                let (_, min, max, _) = self.process_chord_group(
                                    Document::new(),
                                    &temp_chord_group,
                                    &state.current_clefs,
                                    part_start_y,
                                    0.0,
                                    0.0,
                                    measure_time_map,
                                    measure_total_dur,
                                    measure_theoretical_dur,
                                    temp_group_time_pos,
                                    &mut &mut temp_active_beams,
                                    &mut temp_active_slurs,
                                    &mut temp_active_ties,
                                    &mut temp_active_tuplets,
                                    &mut state.active_lyrics.clone(),
                                    &mut state.active_glissandi.clone(),
                                    &mut state.active_slides.clone(),
                                    &mut state.active_hammer_ons.clone(),
                                    &mut state.active_pull_offs.clone(),
                                    &mut state.active_tremolos.clone(),
                                    &mut state.active_wavy_lines.clone(),
                                    &temp_active_octave_shifts,
                                    &mut Vec::new(),
                                    staff_dist,
                                    temp_slash_active,
                                    temp_slash_use_stems,
                                    temp_slash_dots,
                                    temp_slash_note_type.as_deref(),
                                    temp_beat_repeat_active,
                                    temp_beat_repeat_slashes,
                                    temp_beats,
                                    temp_divisions,
                                    0.0,
                                    &temp_staff_lines,
                                    0.0,
                                    grace_offset,
                                    &measure_staff_voices,
                                );
                                // Merge: other staves/voices share this onset.
                                let entry = measure_occupancy
                                    .entry(temp_group_time_pos)
                                    .or_insert((f32::MAX, f32::MIN));
                                entry.0 = entry.0.min(min - part_start_y);
                                entry.1 = entry.1.max(max - part_start_y);
                                if temp_beat_repeat_active {
                                    _temp_beat_repeat_first = false;
                                }
                            }

                            self.placed_notes.borrow_mut().truncate(placed_notes_len);
                            self.pending_tuplets.borrow_mut().truncate(pending_tuplets_len);
                            self.pending_beam_marks.borrow_mut().truncate(pending_beam_marks_len);
                            self.placed_accidentals.borrow_mut().truncate(placed_accidentals_len);
                            self.placed_tuplets.borrow_mut().truncate(placed_tuplets_len);
                            self.placed_fingerings.borrow_mut().truncate(placed_fingerings_len);
                            // Update active octave shifts based on the pre-calculated measure occupancy
                            // to ensure they clear all notes in this measure.
                            // (Not one between two staves: the part-wide occupancy
                            // includes the other staff, and would pull it over that
                            // staff's notes. It clears its own staff when drawn.)
                            let num_staves_here = state.num_staves;
                            for (&_num, start_info) in state.active_octave_shifts.iter_mut() {
                                let staff = start_info.staff.unwrap_or(1);
                                let between_staves = if start_info.is_below {
                                    staff < num_staves_here
                                } else {
                                    staff > 1
                                };
                                if between_staves {
                                    continue;
                                }
                                for (&_time, &(m_min, m_max)) in &measure_occupancy {
                                    let abs_min = m_min + part_start_y;
                                    let abs_max = m_max + part_start_y;
                                    if start_info.is_below {
                                        start_info.y = start_info.y.max(abs_max + 10.0);
                                    } else {
                                        start_info.y = start_info.y.min(abs_min - 15.0);
                                    }
                                }
                            }

                            let mut time_pos = 0;
                            // Keyed by (voice, beam-level number): two voices sharing a
                            // staff can each have an independent beam-level-1 group open
                            // at once (e.g. a malformed/missing <backup>, or an exporter
                            // that interleaves voices by beat instead of emitting one
                            // voice fully before the next). Keying by level number alone
                            // let their notes collide into a single group and inherit
                            // whichever voice's note happened to be pushed first.
                            let mut active_beams: HashMap<
                                (i32, i32),
                                Vec<(StemInfo, Vec<crate::models::Beam>)>,
                            > = HashMap::new();
                            let mut chord_group = Vec::new();
                            let mut group_time_pos = 0;
                            let elements = &measure.elements;
                            let mut priority_elements: Vec<StackedElement> = Vec::new();
                            let mut used_grace_widths: HashMap<i32, f32> = HashMap::new();

                            let current_time_x_offset = 0.0;
                            for (el_idx, element) in elements.iter().enumerate() {
                                let _el_time = match element {
                                    MeasureElement::Note(n) if n.is_chord => group_time_pos,
                                    MeasureElement::Note(_) => time_pos,
                                    _ => time_pos,
                                };

                                match element {
                                    MeasureElement::Sound(_) => {}
                                    MeasureElement::Note(note) => {
                                        if note.is_chord {
                                            chord_group.push(note);
                                        } else {
                                            if !chord_group.is_empty() {
                                                if state.beat_repeat_active
                                                    && !state.beat_repeat_first
                                                {
                                                    let x_ratio = measure_time_map
                                                        .get(&group_time_pos)
                                                        .cloned()
                                                        .unwrap_or(0.0);
                                                    let slash_x = content_start_x
                                                        + x_ratio * content_width
                                                        + current_time_x_offset;
                                                    let slash_y = part_start_y
                                                        + (chord_group[0].staff.unwrap_or(1)
                                                            as f32
                                                            - 1.0)
                                                            * staff_dist
                                                        + 2.0 * self.staff_line_distance;
                                                    let sym = self.get_beat_repeat_symbol(
                                                        state.beat_repeat_slashes,
                                                    );
                                                    sys_doc = sys_doc.add(
                                                        Text::new(sym)
                                                            .set("x", slash_x)
                                                            .set("y", slash_y)
                                                            .set(
                                                                "font-size",
                                                                self.staff_line_distance * 4.0,
                                                            )
                                                            .set("text-anchor", "middle")
                                                            .set("dominant-baseline", "central")
                                                            .set(
                                                                "font-family",
                                                                self.font_family.as_str(),
                                                            ),
                                                    );
                                                } else {
                                                    let grace_offset =
                                                        if chord_group[0].grace.is_some() {
                                                            let total_grace_w = *measure_spacings
                                                                [m_idx]
                                                                .grace_widths
                                                                .get(&group_time_pos)
                                                                .unwrap_or(&0.0);
                                                            let used = used_grace_widths
                                                                .entry(group_time_pos)
                                                                .or_insert(0.0);
                                                            let current_note_w = chord_group
                                                                .iter()
                                                                .map(|n| self.estimate_grace_spacing(n))
                                                                .fold(0.0, f32::max);
                                                            let offset = -(total_grace_w - *used);
                                                            *used += current_note_w;
                                                            offset
                                                        } else {
                                                            0.0
                                                        };
                                                    let (new_doc, _, _, group_right_edge) = self
                                                        .process_chord_group(
                                                            sys_doc,
                                                            &chord_group,
                                                            &state.current_clefs,
                                                            part_start_y,
                                                            content_start_x + current_time_x_offset,
                                                            content_width,
                                                            measure_time_map,
                                                            measure_total_dur,
                                                            measure_theoretical_dur,
                                                            group_time_pos,
                                                            &mut active_beams,
                                                            &mut state.active_slurs,
                                                            &mut state.active_ties,
                                                            &mut state.active_tuplets,
                                                            &mut state.active_lyrics,
                                                            &mut state.active_glissandi,
                                                            &mut state.active_slides,
                                                            &mut state.active_hammer_ons,
                                                            &mut state.active_pull_offs,
                                                            &mut state.active_tremolos,
                                                            &mut state.active_wavy_lines,
                                                            &state.active_octave_shifts,
                                                            &mut priority_elements,
                                                            staff_dist,
                                                            state.slash_active,
                                                            state.slash_use_stems,
                                                            state.slash_dots,
                                                            state.slash_note_type.as_deref(),
                                                            state.beat_repeat_active,
                                                            state.beat_repeat_slashes,
                                                            state.current_beats,
                                                            state.divisions,
                                                            current_time_x_offset,
                                                            &state.staff_lines,
                                                            state.lyric_baseline_y,
                                                            grace_offset,
                                                            &measure_staff_voices,
                                                        );
                                                    sys_doc = new_doc;
                                                    measure_render_right_edge =
                                                        measure_render_right_edge
                                                            .max(group_right_edge);
                                                    if state.beat_repeat_active {
                                                        state.beat_repeat_first = false;
                                                    }
                                                }
                                            }
                                            chord_group.clear();
                                            chord_group.push(note);
                                            group_time_pos = time_pos;

                                            // Handle rhythmic slash for measure rest immediately
                                            if state.slash_active && note.rest_measure {
                                                let (new_doc, _, _, group_right_edge) = self
                                                    .process_chord_group(
                                                        sys_doc,
                                                        &chord_group,
                                                        &state.current_clefs,
                                                        part_start_y,
                                                        content_start_x + current_time_x_offset,
                                                        content_width,
                                                        measure_time_map,
                                                        measure_total_dur,
                                                        measure_theoretical_dur,
                                                        group_time_pos,
                                                        &mut active_beams,
                                                        &mut state.active_slurs,
                                                        &mut state.active_ties,
                                                        &mut state.active_tuplets,
                                                        &mut state.active_lyrics,
                                                        &mut state.active_glissandi,
                                                        &mut state.active_slides,
                                                        &mut state.active_hammer_ons,
                                                        &mut state.active_pull_offs,
                                                        &mut state.active_tremolos,
                                                        &mut state.active_wavy_lines,
                                                        &state.active_octave_shifts,
                                                        &mut priority_elements,
                                                        staff_dist,
                                                        state.slash_active,
                                                        state.slash_use_stems,
                                                        state.slash_dots,
                                                        state.slash_note_type.as_deref(),
                                                        state.beat_repeat_active,
                                                        state.beat_repeat_slashes,
                                                        state.current_beats,
                                                        state.divisions,
                                                        current_time_x_offset,
                                                        &state.staff_lines,
                                                        state.lyric_baseline_y,
                                                        0.0,
                                                        &measure_staff_voices,
                                                    );
                                                sys_doc = new_doc;
                                                measure_render_right_edge =
                                                    measure_render_right_edge.max(group_right_edge);
                                                chord_group.clear();
                                            }

                                            time_pos += (note.duration as f64 * 10080.0
                                                / state.divisions as f64)
                                                .round()
                                                as i32;
                                        }
                                    }
                                    MeasureElement::Backup(d) => {
                                        if !chord_group.is_empty() {
                                            if state.beat_repeat_active && !state.beat_repeat_first
                                            {
                                                let x_ratio = measure_time_map
                                                    .get(&group_time_pos)
                                                    .cloned()
                                                    .unwrap_or(0.0);
                                                let slash_x = content_start_x
                                                    + x_ratio * content_width
                                                    + current_time_x_offset;
                                                let slash_y = part_start_y
                                                    + (chord_group[0].staff.unwrap_or(1) as f32
                                                        - 1.0)
                                                        * staff_dist
                                                    + 2.0 * self.staff_line_distance;
                                                let sym = self.get_beat_repeat_symbol(
                                                    state.beat_repeat_slashes,
                                                );
                                                sys_doc = sys_doc.add(
                                                    Text::new(sym)
                                                        .set("x", slash_x)
                                                        .set("y", slash_y)
                                                        .set(
                                                            "font-size",
                                                            self.staff_line_distance * 4.0,
                                                        )
                                                        .set("text-anchor", "middle")
                                                        .set("dominant-baseline", "central")
                                                        .set(
                                                            "font-family",
                                                            self.font_family.as_str(),
                                                        ),
                                                );
                                            } else {
                                                let grace_offset = if chord_group[0].grace.is_some()
                                                {
                                                    let total_grace_w = *measure_spacings[m_idx]
                                                        .grace_widths
                                                        .get(&group_time_pos)
                                                        .unwrap_or(&0.0);
                                                    let used = used_grace_widths
                                                        .entry(group_time_pos)
                                                        .or_insert(0.0);
                                                    let current_note_w = chord_group
                                                        .iter()
                                                        .map(|n| self.estimate_grace_spacing(n))
                                                        .fold(0.0, f32::max);
                                                    let offset = -(total_grace_w - *used);
                                                    *used += current_note_w;
                                                    offset
                                                } else {
                                                    0.0
                                                };
                                                let (new_doc, _, _, group_right_edge) = self
                                                    .process_chord_group(
                                                        sys_doc,
                                                        &chord_group,
                                                        &state.current_clefs,
                                                        part_start_y,
                                                        content_start_x + current_time_x_offset,
                                                        content_width,
                                                        measure_time_map,
                                                        measure_total_dur,
                                                        measure_theoretical_dur,
                                                        group_time_pos,
                                                        &mut active_beams,
                                                        &mut state.active_slurs,
                                                        &mut state.active_ties,
                                                        &mut state.active_tuplets,
                                                        &mut state.active_lyrics,
                                                        &mut state.active_glissandi,
                                                        &mut state.active_slides,
                                                        &mut state.active_hammer_ons,
                                                        &mut state.active_pull_offs,
                                                        &mut state.active_tremolos,
                                                        &mut state.active_wavy_lines,
                                                        &state.active_octave_shifts,
                                                        &mut priority_elements,
                                                        staff_dist,
                                                        state.slash_active,
                                                        state.slash_use_stems,
                                                        state.slash_dots,
                                                        state.slash_note_type.as_deref(),
                                                        state.beat_repeat_active,
                                                        state.beat_repeat_slashes,
                                                        state.current_beats,
                                                        state.divisions,
                                                        current_time_x_offset,
                                                        &state.staff_lines,
                                                        state.lyric_baseline_y,
                                                        grace_offset,
                                                        &measure_staff_voices,
                                                    );
                                                sys_doc = new_doc;
                                                measure_render_right_edge =
                                                    measure_render_right_edge.max(group_right_edge);
                                                if state.beat_repeat_active {
                                                    state.beat_repeat_first = false;
                                                }
                                            }
                                            chord_group.clear();
                                        }
                                        time_pos -= (*d as f64 * 10080.0 / state.divisions as f64)
                                            .round()
                                            as i32;
                                    }
                                    MeasureElement::Forward(d) => {
                                        if !chord_group.is_empty() {
                                            if state.beat_repeat_active && !state.beat_repeat_first
                                            {
                                                let x_ratio = measure_time_map
                                                    .get(&group_time_pos)
                                                    .cloned()
                                                    .unwrap_or(0.0);
                                                let slash_x = content_start_x
                                                    + x_ratio * content_width
                                                    + current_time_x_offset;
                                                let slash_y = part_start_y
                                                    + (chord_group[0].staff.unwrap_or(1) as f32
                                                        - 1.0)
                                                        * staff_dist
                                                    + 2.0 * self.staff_line_distance;
                                                let sym = self.get_beat_repeat_symbol(
                                                    state.beat_repeat_slashes,
                                                );
                                                sys_doc = sys_doc.add(
                                                    Text::new(sym)
                                                        .set("x", slash_x)
                                                        .set("y", slash_y)
                                                        .set(
                                                            "font-size",
                                                            self.staff_line_distance * 4.0,
                                                        )
                                                        .set("text-anchor", "middle")
                                                        .set("dominant-baseline", "central")
                                                        .set(
                                                            "font-family",
                                                            self.font_family.as_str(),
                                                        ),
                                                );
                                            } else {
                                                let grace_offset = if chord_group[0].grace.is_some()
                                                {
                                                    let total_grace_w = *measure_spacings[m_idx]
                                                        .grace_widths
                                                        .get(&group_time_pos)
                                                        .unwrap_or(&0.0);
                                                    let used = used_grace_widths
                                                        .entry(group_time_pos)
                                                        .or_insert(0.0);
                                                    let current_note_w = chord_group
                                                        .iter()
                                                        .map(|n| self.estimate_grace_spacing(n))
                                                        .fold(0.0, f32::max);
                                                    let offset = -(total_grace_w - *used);
                                                    *used += current_note_w;
                                                    offset
                                                } else {
                                                    0.0
                                                };
                                                let (new_doc, _, _, group_right_edge) = self
                                                    .process_chord_group(
                                                        sys_doc,
                                                        &chord_group,
                                                        &state.current_clefs,
                                                        part_start_y,
                                                        content_start_x + current_time_x_offset,
                                                        content_width,
                                                        measure_time_map,
                                                        measure_total_dur,
                                                        measure_theoretical_dur,
                                                        group_time_pos,
                                                        &mut active_beams,
                                                        &mut state.active_slurs,
                                                        &mut state.active_ties,
                                                        &mut state.active_tuplets,
                                                        &mut state.active_lyrics,
                                                        &mut state.active_glissandi,
                                                        &mut state.active_slides,
                                                        &mut state.active_hammer_ons,
                                                        &mut state.active_pull_offs,
                                                        &mut state.active_tremolos,
                                                        &mut state.active_wavy_lines,
                                                        &state.active_octave_shifts,
                                                        &mut priority_elements,
                                                        staff_dist,
                                                        state.slash_active,
                                                        state.slash_use_stems,
                                                        state.slash_dots,
                                                        state.slash_note_type.as_deref(),
                                                        state.beat_repeat_active,
                                                        state.beat_repeat_slashes,
                                                        state.current_beats,
                                                        state.divisions,
                                                        current_time_x_offset,
                                                        &state.staff_lines,
                                                        state.lyric_baseline_y,
                                                        grace_offset,
                                                        &measure_staff_voices,
                                                    );
                                                sys_doc = new_doc;
                                                measure_render_right_edge =
                                                    measure_render_right_edge.max(group_right_edge);
                                                if state.beat_repeat_active {
                                                    state.beat_repeat_first = false;
                                                }
                                            }
                                            chord_group.clear();
                                        }
                                        time_pos += (*d as f64 * 10080.0 / state.divisions as f64)
                                            .round()
                                            as i32;
                                    }
                                    MeasureElement::Attributes(attr) => {
                                        let mut is_mid = false;
                                        for i in (el_idx + 1)..elements.len() {
                                            if let MeasureElement::Note(n) = &elements[i] {
                                                if n.is_chord {
                                                    is_mid = true;
                                                }
                                                break;
                                            }
                                            if let MeasureElement::Forward(_)
                                            | MeasureElement::Backup(_) = &elements[i]
                                            {
                                                break;
                                            }
                                        }
                                        if !is_mid && !chord_group.is_empty() {
                                            if state.beat_repeat_active && !state.beat_repeat_first
                                            {
                                                let x_ratio = measure_time_map
                                                    .get(&group_time_pos)
                                                    .cloned()
                                                    .unwrap_or(0.0);
                                                let slash_x = content_start_x
                                                    + x_ratio * content_width
                                                    + current_time_x_offset;
                                                let slash_y = part_start_y
                                                    + (chord_group[0].staff.unwrap_or(1) as f32
                                                        - 1.0)
                                                        * staff_dist
                                                    + 2.0 * self.staff_line_distance;
                                                let sym = self.get_beat_repeat_symbol(
                                                    state.beat_repeat_slashes,
                                                );
                                                sys_doc = sys_doc.add(
                                                    Text::new(sym)
                                                        .set("x", slash_x)
                                                        .set("y", slash_y)
                                                        .set(
                                                            "font-size",
                                                            self.staff_line_distance * 4.0,
                                                        )
                                                        .set("text-anchor", "middle")
                                                        .set("dominant-baseline", "central")
                                                        .set(
                                                            "font-family",
                                                            self.font_family.as_str(),
                                                        ),
                                                );
                                            } else {
                                                let grace_offset = if chord_group[0].grace.is_some()
                                                {
                                                    let total_grace_w = *measure_spacings[m_idx]
                                                        .grace_widths
                                                        .get(&group_time_pos)
                                                        .unwrap_or(&0.0);
                                                    let used = used_grace_widths
                                                        .entry(group_time_pos)
                                                        .or_insert(0.0);
                                                    let current_note_w = chord_group
                                                        .iter()
                                                        .map(|n| self.estimate_grace_spacing(n))
                                                        .fold(0.0, f32::max);
                                                    let offset = -(total_grace_w - *used);
                                                    *used += current_note_w;
                                                    offset
                                                } else {
                                                    0.0
                                                };
                                                let (new_doc, _, _, group_right_edge) = self
                                                    .process_chord_group(
                                                        sys_doc,
                                                        &chord_group,
                                                        &state.current_clefs,
                                                        part_start_y,
                                                        content_start_x + current_time_x_offset,
                                                        content_width,
                                                        measure_time_map,
                                                        measure_total_dur,
                                                        measure_theoretical_dur,
                                                        group_time_pos,
                                                        &mut active_beams,
                                                        &mut state.active_slurs,
                                                        &mut state.active_ties,
                                                        &mut state.active_tuplets,
                                                        &mut state.active_lyrics,
                                                        &mut state.active_glissandi,
                                                        &mut state.active_slides,
                                                        &mut state.active_hammer_ons,
                                                        &mut state.active_pull_offs,
                                                        &mut state.active_tremolos,
                                                        &mut state.active_wavy_lines,
                                                        &state.active_octave_shifts,
                                                        &mut priority_elements,
                                                        staff_dist,
                                                        state.slash_active,
                                                        state.slash_use_stems,
                                                        state.slash_dots,
                                                        state.slash_note_type.as_deref(),
                                                        state.beat_repeat_active,
                                                        state.beat_repeat_slashes,
                                                        state.current_beats,
                                                        state.divisions,
                                                        current_time_x_offset,
                                                        &state.staff_lines,
                                                        state.lyric_baseline_y,
                                                        grace_offset,
                                                        &measure_staff_voices,
                                                    );
                                                sys_doc = new_doc;
                                                measure_render_right_edge =
                                                    measure_render_right_edge.max(group_right_edge);
                                                if state.beat_repeat_active {
                                                    state.beat_repeat_first = false;
                                                }
                                            }
                                            chord_group.clear();
                                        }
                                        if let Some(n) = attr.staves {
                                            num_staves = n;
                                            state.num_staves = n;
                                        }
                                        for sd in &attr.staff_details {
                                            if let Some(lines) = sd.staff_lines {
                                                state.staff_lines.insert(sd.number, lines);
                                            }
                                        }
                                        for clef in &attr.clefs {
                                            state.current_clefs.insert(clef.number, clef.clone());
                                        }
                                        if let Some(k) = &attr.key {
                                            state.current_key = k.clone();
                                        }
                                        if let Some(t) = &attr.time {
                                            if let Ok(b) = t
                                                .beats
                                                .split('+')
                                                .next()
                                                .unwrap_or("4")
                                                .parse::<i32>()
                                            {
                                                state.current_beats = b;
                                            }
                                            state.current_beat_type = t.beat_type;
                                        }
                                        if let Some(d) = attr.divisions {
                                            state.divisions = d;
                                        }
                                        if let Some(mr) = &attr.measure_repeat {
                                            if mr.repeat_type == "start" {
                                                state.measure_repeat_active = true;
                                            } else if mr.repeat_type == "stop" {
                                                state.measure_repeat_active = false;
                                            }
                                        }
                                        if let Some(br) = &attr.beat_repeat {
                                            if br.repeat_type == "start" {
                                                state.beat_repeat_active = true;
                                                state.beat_repeat_slashes = br.slashes;
                                                state.beat_repeat_first = true;
                                            } else if br.repeat_type == "stop" {
                                                stop_beat_repeat_after_this = true;
                                            }
                                        }
                                        if let Some(c) = attr.capo {
                                            let x_ratio = measure_time_map
                                                .get(&time_pos)
                                                .cloned()
                                                .unwrap_or(0.0);
                                            let capo_x = content_start_x
                                                + x_ratio * content_width
                                                + current_time_x_offset;
                                            let capo_y = part_start_y - 20.0;
                                            sys_doc = sys_doc.add(
                                                Text::new(format!("Capo {}", c))
                                                    .set("x", capo_x)
                                                    .set("y", capo_y)
                                                    .set("font-size", 12)
                                                    .set("font-family", "serif")
                                                    .set("font-style", "italic")
                                                    .set("text-anchor", "start"),
                                            );
                                        }
                                        if let Some(sl) = &attr.slash {
                                            if sl.slash_type == "start" {
                                                state.slash_active = true;
                                                state.slash_use_stems =
                                                    sl.use_stems.unwrap_or(false);
                                                state.slash_dots = sl.dots;
                                                state.slash_note_type = sl.note_type.clone();
                                            } else if sl.slash_type == "stop" {
                                                state.slash_active = false;
                                            }
                                        }
                                        if let Some(time) = &attr.time {
                                            if let Ok(b) = time
                                                .beats
                                                .split('+')
                                                .next()
                                                .unwrap_or("4")
                                                .parse::<i32>()
                                            {
                                                state.current_beats = b;
                                            }
                                            state.current_beat_type = time.beat_type;
                                        }

                                        if time_pos > 0 {
                                            // Mid-measure attributes follow time map
                                            let x_ratio = measure_time_map
                                                .get(&time_pos)
                                                .cloned()
                                                .unwrap_or(0.0);
                                            // Pre-calculate consumed width to shift it left, avoiding overlap with notes at same time
                                            let (_, w) = self.draw_attributes(
                                                Document::new(),
                                                attr,
                                                0.0,
                                                0.0,
                                                &state.current_clefs,
                                                num_staves,
                                                staff_dist,
                                                &state.staff_lines,
                                                false,
                                            );
                                            let (new_doc, _consumed_x) = self.draw_attributes(
                                                sys_doc,
                                                attr,
                                                part_start_y,
                                                content_start_x
                                                    + x_ratio * content_width
                                                    + current_time_x_offset
                                                    - w
                                                    - 10.0,
                                                &state.current_clefs,
                                                num_staves,
                                                staff_dist,
                                                &state.staff_lines,
                                                false,
                                            );
                                            sys_doc = new_doc;
                                        }
                                    }
                                    MeasureElement::Direction(dir) => {
                                        if !chord_group.is_empty() {
                                            let grace_offset = if chord_group[0].grace.is_some() {
                                                let total_grace_w = *measure_spacings[m_idx]
                                                    .grace_widths
                                                    .get(&group_time_pos)
                                                    .unwrap_or(&0.0);
                                                let used = used_grace_widths
                                                    .entry(group_time_pos)
                                                    .or_insert(0.0);
                                                let current_note_w = chord_group
                                                    .iter()
                                                    .map(|n| self.estimate_grace_spacing(n))
                                                    .fold(0.0, f32::max);
                                                let offset = -(total_grace_w - *used);
                                                *used += current_note_w;
                                                offset
                                            } else {
                                                0.0
                                            };
                                            let (new_doc, _, _, group_right_edge) = self
                                                .process_chord_group(
                                                    sys_doc,
                                                    &chord_group,
                                                    &state.current_clefs,
                                                    part_start_y,
                                                    content_start_x + current_time_x_offset,
                                                    content_width,
                                                    measure_time_map,
                                                    measure_total_dur,
                                                    measure_theoretical_dur,
                                                    group_time_pos,
                                                    &mut active_beams,
                                                    &mut state.active_slurs,
                                                    &mut state.active_ties,
                                                    &mut state.active_tuplets,
                                                    &mut state.active_lyrics,
                                                    &mut state.active_glissandi,
                                                    &mut state.active_slides,
                                                    &mut state.active_hammer_ons,
                                                    &mut state.active_pull_offs,
                                                    &mut state.active_tremolos,
                                                    &mut state.active_wavy_lines,
                                                    &state.active_octave_shifts,
                                                    &mut priority_elements,
                                                    staff_dist,
                                                    state.slash_active,
                                                    state.slash_use_stems,
                                                    state.slash_dots,
                                                    state.slash_note_type.as_deref(),
                                                    state.beat_repeat_active,
                                                    state.beat_repeat_slashes,
                                                    state.current_beats,
                                                    state.divisions,
                                                    current_time_x_offset,
                                                    &state.staff_lines,
                                                    state.lyric_baseline_y,
                                                    grace_offset,
                                                    &measure_staff_voices,
                                                );
                                            sys_doc = new_doc;
                                            measure_render_right_edge =
                                                measure_render_right_edge.max(group_right_edge);
                                            if state.beat_repeat_active {
                                                state.beat_repeat_first = false;
                                            }
                                            chord_group.clear();
                                        }

                                        let x_ratio =
                                            measure_time_map.get(&time_pos).cloned().unwrap_or(0.0);
                                        let dir_x = content_start_x
                                            + x_ratio * content_width
                                            + current_time_x_offset;

                                        // Words leading a bracket or dashes line started in
                                        // the same direction are its label, drawn with it.
                                        let starts_line = |t: &crate::models::DirectionType| {
                                            matches!(t, crate::models::DirectionType::Bracket(b) if b.bracket_type == "start")
                                                || matches!(t, crate::models::DirectionType::Dashes(d) if d.dashes_type == "start")
                                        };
                                        let line_label: Option<String> =
                                            if dir.types.iter().any(starts_line) {
                                                let text: String = dir
                                                    .types
                                                    .iter()
                                                    .filter_map(|t| match t {
                                                        crate::models::DirectionType::Words(w) => {
                                                            Some(w.as_str())
                                                        }
                                                        _ => None,
                                                    })
                                                    .collect::<Vec<_>>()
                                                    .join(" ");
                                                (!text.trim().is_empty()).then_some(text)
                                            } else {
                                                None
                                            };
                                        for dtype in &dir.types {
                                            if line_label.is_some()
                                                && matches!(dtype, crate::models::DirectionType::Words(_))
                                            {
                                                continue;
                                            }
                                            // Immediately update state-affecting directions for correct note positioning
                                            if let crate::models::DirectionType::OctaveShift(
                                                shift,
                                            ) = dtype
                                            {
                                                let num = shift.number.unwrap_or(1);
                                                if shift.shift_type == "up"
                                                    || shift.shift_type == "down"
                                                {
                                                    let is_below = Self::octave_shift_below(
                                                        dir.placement.as_deref(),
                                                        &shift.shift_type,
                                                    );
                                                    let y_offset = part_start_y
                                                        - 4.0 * self.staff_line_distance;
                                                    state.active_octave_shifts.insert(
                                                        num,
                                                        OctaveShiftStartInfo {
                                                            x: dir_x,
                                                            y: y_offset,
                                                            size: shift.size,
                                                            is_continuation: false,
                                                            shift_type: shift.shift_type.clone(),
                                                            staff: dir.staff,
                                                            is_below,
                                                            ended: false,
                                                        },
                                                    );
                                                } else if shift.shift_type == "stop" {
                                                    // Notes from here on are unshifted; the line
                                                    // itself is drawn in the priority pass.
                                                    if let Some(active) = state
                                                        .active_octave_shifts
                                                        .get_mut(&shift.number.unwrap_or(1))
                                                    {
                                                        active.ended = true;
                                                    }
                                                }
                                            }
                                            priority_elements.push(StackedElement {
                                                priority: self.get_priority(dtype),
                                                time_pos,
                                                item: StackedItem::DirectionType(
                                                    dtype.clone(),
                                                    dir.placement.clone(),
                                                    dir.staff,
                                                    line_label.clone().filter(|_| starts_line(dtype)),
                                                ),
                                            });
                                        }
                                    }
                                    MeasureElement::Harmony(harmony) => {
                                        if !chord_group.is_empty() {
                                            let grace_offset = if chord_group[0].grace.is_some() {
                                                let total_grace_w = *measure_spacings[m_idx]
                                                    .grace_widths
                                                    .get(&group_time_pos)
                                                    .unwrap_or(&0.0);
                                                let used = used_grace_widths
                                                    .entry(group_time_pos)
                                                    .or_insert(0.0);
                                                let current_note_w = chord_group
                                                    .iter()
                                                    .map(|n| self.estimate_grace_spacing(n))
                                                    .fold(0.0, f32::max);
                                                let offset = -(total_grace_w - *used);
                                                *used += current_note_w;
                                                offset
                                            } else {
                                                0.0
                                            };
                                            let (new_doc, _, _, group_right_edge) = self
                                                .process_chord_group(
                                                    sys_doc,
                                                    &chord_group,
                                                    &state.current_clefs,
                                                    part_start_y,
                                                    content_start_x + current_time_x_offset,
                                                    content_width,
                                                    measure_time_map,
                                                    measure_total_dur,
                                                    measure_theoretical_dur,
                                                    group_time_pos,
                                                    &mut active_beams,
                                                    &mut state.active_slurs,
                                                    &mut state.active_ties,
                                                    &mut state.active_tuplets,
                                                    &mut state.active_lyrics,
                                                    &mut state.active_glissandi,
                                                    &mut state.active_slides,
                                                    &mut state.active_hammer_ons,
                                                    &mut state.active_pull_offs,
                                                    &mut state.active_tremolos,
                                                    &mut state.active_wavy_lines,
                                                    &state.active_octave_shifts,
                                                    &mut priority_elements,
                                                    staff_dist,
                                                    state.slash_active,
                                                    state.slash_use_stems,
                                                    state.slash_dots,
                                                    state.slash_note_type.as_deref(),
                                                    state.beat_repeat_active,
                                                    state.beat_repeat_slashes,
                                                    state.current_beats,
                                                    state.divisions,
                                                    current_time_x_offset,
                                                    &state.staff_lines,
                                                    state.lyric_baseline_y,
                                                    grace_offset,
                                                    &measure_staff_voices,
                                                );
                                            sys_doc = new_doc;
                                            measure_render_right_edge =
                                                measure_render_right_edge.max(group_right_edge);
                                            if state.beat_repeat_active {
                                                state.beat_repeat_first = false;
                                            }
                                            chord_group.clear();
                                        }
                                        priority_elements.push(StackedElement {
                                            priority: 8,
                                            time_pos,
                                            item: StackedItem::Harmony(harmony.clone()),
                                        });
                                    }
                                    MeasureElement::Bookmark(id) => {
                                        if !chord_group.is_empty() {
                                            if state.beat_repeat_active && !state.beat_repeat_first
                                            {
                                                let x_ratio = measure_time_map
                                                    .get(&group_time_pos)
                                                    .cloned()
                                                    .unwrap_or(0.0);
                                                let slash_x = content_start_x
                                                    + x_ratio * content_width
                                                    + current_time_x_offset;
                                                let slash_y = part_start_y
                                                    + (chord_group[0].staff.unwrap_or(1) as f32
                                                        - 1.0)
                                                        * staff_dist
                                                    + 2.0 * self.staff_line_distance;
                                                let sym = self.get_beat_repeat_symbol(
                                                    state.beat_repeat_slashes,
                                                );
                                                sys_doc = sys_doc.add(
                                                    Text::new(sym)
                                                        .set("x", slash_x)
                                                        .set("y", slash_y)
                                                        .set(
                                                            "font-size",
                                                            self.staff_line_distance * 4.0,
                                                        )
                                                        .set("text-anchor", "middle")
                                                        .set("dominant-baseline", "central")
                                                        .set(
                                                            "font-family",
                                                            self.font_family.as_str(),
                                                        ),
                                                );
                                            } else {
                                                let grace_offset = if chord_group[0].grace.is_some()
                                                {
                                                    let total_grace_w = *measure_spacings[m_idx]
                                                        .grace_widths
                                                        .get(&group_time_pos)
                                                        .unwrap_or(&0.0);
                                                    let used = used_grace_widths
                                                        .entry(group_time_pos)
                                                        .or_insert(0.0);
                                                    let current_note_w = chord_group
                                                        .iter()
                                                        .map(|n| self.estimate_grace_spacing(n))
                                                        .fold(0.0, f32::max);
                                                    let offset = -(total_grace_w - *used);
                                                    *used += current_note_w;
                                                    offset
                                                } else {
                                                    0.0
                                                };
                                                let (new_doc, _, _, group_right_edge) = self
                                                    .process_chord_group(
                                                        sys_doc,
                                                        &chord_group,
                                                        &state.current_clefs,
                                                        part_start_y,
                                                        content_start_x + current_time_x_offset,
                                                        content_width,
                                                        measure_time_map,
                                                        measure_total_dur,
                                                        measure_theoretical_dur,
                                                        group_time_pos,
                                                        &mut active_beams,
                                                        &mut state.active_slurs,
                                                        &mut state.active_ties,
                                                        &mut state.active_tuplets,
                                                        &mut state.active_lyrics,
                                                        &mut state.active_glissandi,
                                                        &mut state.active_slides,
                                                        &mut state.active_hammer_ons,
                                                        &mut state.active_pull_offs,
                                                        &mut state.active_tremolos,
                                                        &mut state.active_wavy_lines,
                                                        &state.active_octave_shifts,
                                                        &mut priority_elements,
                                                        staff_dist,
                                                        state.slash_active,
                                                        state.slash_use_stems,
                                                        state.slash_dots,
                                                        state.slash_note_type.as_deref(),
                                                        state.beat_repeat_active,
                                                        state.beat_repeat_slashes,
                                                        state.current_beats,
                                                        state.divisions,
                                                        current_time_x_offset,
                                                        &state.staff_lines,
                                                        state.lyric_baseline_y,
                                                        grace_offset,
                                                        &measure_staff_voices,
                                                    );
                                                sys_doc = new_doc;
                                                measure_render_right_edge =
                                                    measure_render_right_edge.max(group_right_edge);
                                                if state.beat_repeat_active {
                                                    state.beat_repeat_first = false;
                                                }
                                            }
                                            chord_group.clear();
                                        }
                                        let x_ratio =
                                            measure_time_map.get(&time_pos).cloned().unwrap_or(0.0);
                                        let bookmark_x = content_start_x
                                            + x_ratio * content_width
                                            + current_time_x_offset;
                                        sys_doc = self.draw_bookmark(
                                            sys_doc,
                                            id,
                                            bookmark_x,
                                            part_start_y - 65.0,
                                        );
                                    }
                                    MeasureElement::Barline(bl) => {
                                        if !chord_group.is_empty() {
                                            let grace_offset = if chord_group[0].grace.is_some() {
                                                let total_grace_w = *measure_spacings[m_idx]
                                                    .grace_widths
                                                    .get(&group_time_pos)
                                                    .unwrap_or(&0.0);
                                                let used = used_grace_widths
                                                    .entry(group_time_pos)
                                                    .or_insert(0.0);
                                                let current_note_w = chord_group
                                                    .iter()
                                                    .map(|n| self.estimate_grace_spacing(n))
                                                    .fold(0.0, f32::max);
                                                let offset = -(total_grace_w - *used);
                                                *used += current_note_w;
                                                offset
                                            } else {
                                                0.0
                                            };
                                            let (new_doc, _, _, group_right_edge) = self
                                                .process_chord_group(
                                                    sys_doc,
                                                    &chord_group,
                                                    &state.current_clefs,
                                                    part_start_y,
                                                    content_start_x + current_time_x_offset,
                                                    content_width,
                                                    measure_time_map,
                                                    measure_total_dur,
                                                    measure_theoretical_dur,
                                                    group_time_pos,
                                                    &mut active_beams,
                                                    &mut state.active_slurs,
                                                    &mut state.active_ties,
                                                    &mut state.active_tuplets,
                                                    &mut state.active_lyrics,
                                                    &mut state.active_glissandi,
                                                    &mut state.active_slides,
                                                    &mut state.active_hammer_ons,
                                                    &mut state.active_pull_offs,
                                                    &mut state.active_tremolos,
                                                    &mut state.active_wavy_lines,
                                                    &state.active_octave_shifts,
                                                    &mut priority_elements,
                                                    staff_dist,
                                                    state.slash_active,
                                                    state.slash_use_stems,
                                                    state.slash_dots,
                                                    state.slash_note_type.as_deref(),
                                                    state.beat_repeat_active,
                                                    state.beat_repeat_slashes,
                                                    state.current_beats,
                                                    state.divisions,
                                                    current_time_x_offset,
                                                    &state.staff_lines,
                                                    state.lyric_baseline_y,
                                                    grace_offset,
                                                    &measure_staff_voices,
                                                );
                                            sys_doc = new_doc;
                                            measure_render_right_edge =
                                                measure_render_right_edge.max(group_right_edge);
                                            if state.beat_repeat_active {
                                                state.beat_repeat_first = false;
                                            }
                                            chord_group.clear();
                                        }
                                        let bl_x = if bl.location == "left" {
                                            measure_x
                                        } else if bl.location == "middle" {
                                            let x_ratio = measure_time_map
                                                .get(&time_pos)
                                                .cloned()
                                                .unwrap_or(0.5);
                                            content_start_x
                                                + x_ratio * content_width
                                                + current_time_x_offset
                                                - 10.0
                                        } else {
                                            has_right_barline = true;
                                            measure_render_right_edge.max(measure_x + m_width)
                                        };
                                        sys_doc = self.process_barline(
                                            sys_doc,
                                            bl,
                                            part_start_y,
                                            bl_x,
                                            num_staves,
                                            &current_system_group_ranges,
                                            staff_dist,
                                            &state.staff_lines,
                                            state.lyric_baseline_y,
                                        );

                                        if let Some(ending) = &bl.ending {
                                            if id == &score.parts[0].id {
                                                match ending.ending_type.as_str() {
                                                    "start" => {
                                                        let mut ending_y = part_start_y - 35.0;
                                                        let measure_min = measure_occupancy
                                                            .values()
                                                            .map(|(m_min, _)| *m_min)
                                                            .fold(f32::MAX, f32::min);
                                                        if measure_min != f32::MAX {
                                                            ending_y = ending_y.min(
                                                                part_start_y + measure_min - 25.0,
                                                            );
                                                        }
                                                        let ending_min = ending_y - 10.0;
                                                        let ending_max = ending_y + 12.0;
                                                        let entry = measure_occupancy
                                                            .entry(time_pos)
                                                            .or_insert((f32::MAX, f32::MIN));
                                                        entry.0 =
                                                            entry.0.min(ending_min - part_start_y);
                                                        entry.1 =
                                                            entry.1.max(ending_max - part_start_y);
                                                        state.active_ending =
                                                            Some(EndingStartInfo {
                                                                x: bl_x + 2.0,
                                                                y: ending_y,
                                                                text: ending.text.clone(),
                                                                is_continuation: false,
                                                            });
                                                    }
                                                    "stop" | "discontinue" => {
                                                        if let Some(start_info) =
                                                            state.active_ending.take()
                                                        {
                                                            sys_doc = self.draw_volta_bracket(
                                                                sys_doc,
                                                                start_info.x,
                                                                start_info.y,
                                                                bl_x - 2.0,
                                                                start_info.y,
                                                                &start_info.text,
                                                                !start_info.is_continuation,
                                                                ending.ending_type == "stop",
                                                            );
                                                        }
                                                    }
                                                    _ => {}
                                                }
                                            }
                                        }
                                    }
                                    MeasureElement::Frame(frame) => {
                                        if !chord_group.is_empty() {
                                            let grace_offset = if chord_group[0].grace.is_some() {
                                                let total_grace_w = *measure_spacings[m_idx]
                                                    .grace_widths
                                                    .get(&group_time_pos)
                                                    .unwrap_or(&0.0);
                                                let used = used_grace_widths
                                                    .entry(group_time_pos)
                                                    .or_insert(0.0);
                                                let current_note_w = chord_group
                                                    .iter()
                                                    .map(|n| self.estimate_grace_spacing(n))
                                                    .fold(0.0, f32::max);
                                                let offset = -(total_grace_w - *used);
                                                *used += current_note_w;
                                                offset
                                            } else {
                                                0.0
                                            };
                                            let (new_doc, _, _, group_right_edge) = self
                                                .process_chord_group(
                                                    sys_doc,
                                                    &chord_group,
                                                    &state.current_clefs,
                                                    part_start_y,
                                                    content_start_x + current_time_x_offset,
                                                    content_width,
                                                    measure_time_map,
                                                    measure_total_dur,
                                                    measure_theoretical_dur,
                                                    group_time_pos,
                                                    &mut active_beams,
                                                    &mut state.active_slurs,
                                                    &mut state.active_ties,
                                                    &mut state.active_tuplets,
                                                    &mut state.active_lyrics,
                                                    &mut state.active_glissandi,
                                                    &mut state.active_slides,
                                                    &mut state.active_hammer_ons,
                                                    &mut state.active_pull_offs,
                                                    &mut state.active_tremolos,
                                                    &mut state.active_wavy_lines,
                                                    &state.active_octave_shifts,
                                                    &mut priority_elements,
                                                    staff_dist,
                                                    state.slash_active,
                                                    state.slash_use_stems,
                                                    state.slash_dots,
                                                    state.slash_note_type.as_deref(),
                                                    state.beat_repeat_active,
                                                    state.beat_repeat_slashes,
                                                    state.current_beats,
                                                    state.divisions,
                                                    current_time_x_offset,
                                                    &state.staff_lines,
                                                    state.lyric_baseline_y,
                                                    grace_offset,
                                                    &measure_staff_voices,
                                                );
                                            sys_doc = new_doc;
                                            measure_render_right_edge =
                                                measure_render_right_edge.max(group_right_edge);
                                            if state.beat_repeat_active {
                                                state.beat_repeat_first = false;
                                            }
                                            chord_group.clear();
                                        }
                                        let x_ratio =
                                            measure_time_map.get(&time_pos).cloned().unwrap_or(0.0);
                                        let frame_x = content_start_x
                                            + x_ratio * content_width
                                            + current_time_x_offset;
                                        sys_doc = self.draw_frame(
                                            sys_doc,
                                            frame,
                                            part_start_y - 95.0,
                                            frame_x,
                                        );
                                    }
                                    MeasureElement::FiguredBass(fb) => {
                                        if !chord_group.is_empty() {
                                            if state.beat_repeat_active && !state.beat_repeat_first
                                            {
                                                let x_ratio = measure_time_map
                                                    .get(&group_time_pos)
                                                    .cloned()
                                                    .unwrap_or(0.0);
                                                let slash_x = content_start_x
                                                    + x_ratio * content_width
                                                    + current_time_x_offset;
                                                let slash_y = part_start_y
                                                    + (chord_group[0].staff.unwrap_or(1) as f32
                                                        - 1.0)
                                                        * staff_dist
                                                    + 2.0 * self.staff_line_distance;
                                                let sym = self.get_beat_repeat_symbol(
                                                    state.beat_repeat_slashes,
                                                );
                                                sys_doc = sys_doc.add(
                                                    Text::new(sym)
                                                        .set("x", slash_x)
                                                        .set("y", slash_y)
                                                        .set(
                                                            "font-size",
                                                            self.staff_line_distance * 4.0,
                                                        )
                                                        .set("text-anchor", "middle")
                                                        .set("dominant-baseline", "central")
                                                        .set(
                                                            "font-family",
                                                            self.font_family.as_str(),
                                                        ),
                                                );
                                            } else {
                                                let grace_offset = if chord_group[0].grace.is_some()
                                                {
                                                    let total_grace_w = *measure_spacings[m_idx]
                                                        .grace_widths
                                                        .get(&group_time_pos)
                                                        .unwrap_or(&0.0);
                                                    let used = used_grace_widths
                                                        .entry(group_time_pos)
                                                        .or_insert(0.0);
                                                    let current_note_w = chord_group
                                                        .iter()
                                                        .map(|n| self.estimate_grace_spacing(n))
                                                        .fold(0.0, f32::max);
                                                    let offset = -(total_grace_w - *used);
                                                    *used += current_note_w;
                                                    offset
                                                } else {
                                                    0.0
                                                };
                                                let (new_doc, _, _, group_right_edge) = self
                                                    .process_chord_group(
                                                        sys_doc,
                                                        &chord_group,
                                                        &state.current_clefs,
                                                        part_start_y,
                                                        content_start_x + current_time_x_offset,
                                                        content_width,
                                                        measure_time_map,
                                                        measure_total_dur,
                                                        measure_theoretical_dur,
                                                        group_time_pos,
                                                        &mut active_beams,
                                                        &mut state.active_slurs,
                                                        &mut state.active_ties,
                                                        &mut state.active_tuplets,
                                                        &mut state.active_lyrics,
                                                        &mut state.active_glissandi,
                                                        &mut state.active_slides,
                                                        &mut state.active_hammer_ons,
                                                        &mut state.active_pull_offs,
                                                        &mut state.active_tremolos,
                                                        &mut state.active_wavy_lines,
                                                        &state.active_octave_shifts,
                                                        &mut priority_elements,
                                                        staff_dist,
                                                        state.slash_active,
                                                        state.slash_use_stems,
                                                        state.slash_dots,
                                                        state.slash_note_type.as_deref(),
                                                        state.beat_repeat_active,
                                                        state.beat_repeat_slashes,
                                                        state.current_beats,
                                                        state.divisions,
                                                        current_time_x_offset,
                                                        &state.staff_lines,
                                                        state.lyric_baseline_y,
                                                        grace_offset,
                                                        &measure_staff_voices,
                                                    );
                                                sys_doc = new_doc;
                                                measure_render_right_edge =
                                                    measure_render_right_edge.max(group_right_edge);
                                                if state.beat_repeat_active {
                                                    state.beat_repeat_first = false;
                                                }
                                            }
                                            chord_group.clear();
                                        }
                                        let x_ratio =
                                            measure_time_map.get(&time_pos).cloned().unwrap_or(0.0);
                                        let fb_x = content_start_x
                                            + x_ratio * content_width
                                            + current_time_x_offset;
                                        sys_doc = self.draw_figured_bass(
                                            sys_doc,
                                            fb,
                                            fb_x,
                                            part_start_y + (num_staves as f32 - 1.0) * staff_dist,
                                            &mut state.active_figured_bass,
                                        );
                                    }
                                    MeasureElement::Grouping(grouping) => {
                                        let num = grouping.number.unwrap_or(1);
                                        let x_ratio =
                                            measure_time_map.get(&time_pos).cloned().unwrap_or(0.0);
                                        let gx = content_start_x
                                            + x_ratio * content_width
                                            + current_time_x_offset;
                                        let gy = part_start_y - 20.0;

                                        if grouping.grouping_type == "start" {
                                            state.active_groupings.insert(
                                                num,
                                                GroupingStartInfo {
                                                    x: gx,
                                                    y: gy,
                                                    features: grouping.features.clone(),
                                                    is_continuation: false,
                                                },
                                            );
                                        } else if grouping.grouping_type == "stop" {
                                            if let Some(start_info) =
                                                state.active_groupings.remove(&num)
                                            {
                                                sys_doc = self.draw_grouping_bracket(
                                                    sys_doc,
                                                    start_info.x,
                                                    start_info.y,
                                                    gx,
                                                    gy,
                                                    &start_info.features,
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                            if !chord_group.is_empty() {
                                if state.beat_repeat_active && !state.beat_repeat_first {
                                    let x_ratio = measure_time_map
                                        .get(&group_time_pos)
                                        .cloned()
                                        .unwrap_or(0.0);
                                    let slash_x = content_start_x
                                        + x_ratio * content_width
                                        + current_time_x_offset;
                                    let slash_y = part_start_y
                                        + (chord_group[0].staff.unwrap_or(1) as f32 - 1.0)
                                            * staff_dist
                                        + 2.0 * self.staff_line_distance;
                                    let sym = match state.beat_repeat_slashes {
                                        2 => "\u{E502}",
                                        3 => "\u{E503}",
                                        4 => "\u{E504}",
                                        _ => "\u{E501}",
                                    };
                                    sys_doc = sys_doc.add(
                                        Text::new(sym)
                                            .set("x", slash_x)
                                            .set("y", slash_y)
                                            .set("font-size", self.staff_line_distance * 4.0)
                                            .set("text-anchor", "middle")
                                            .set("dominant-baseline", "central")
                                            .set("font-family", self.font_family.as_str()),
                                    );
                                } else {
                                    let grace_offset = if chord_group[0].grace.is_some() {
                                        let total_grace_w = *measure_spacings[m_idx]
                                            .grace_widths
                                            .get(&group_time_pos)
                                            .unwrap_or(&0.0);
                                        let used =
                                            used_grace_widths.entry(group_time_pos).or_insert(0.0);
                                        let current_note_w = chord_group
                                            .iter()
                                            .map(|n| self.estimate_grace_spacing(n))
                                            .fold(0.0, f32::max);
                                        let offset = -(total_grace_w - *used);
                                        *used += current_note_w;
                                        offset
                                    } else {
                                        0.0
                                    };
                                    let (new_doc, _, _, group_right_edge) = self
                                        .process_chord_group(
                                            sys_doc,
                                            &chord_group,
                                            &state.current_clefs,
                                            part_start_y,
                                            content_start_x + current_time_x_offset,
                                            content_width,
                                            measure_time_map,
                                            measure_total_dur,
                                            measure_theoretical_dur,
                                            group_time_pos,
                                            &mut active_beams,
                                            &mut state.active_slurs,
                                            &mut state.active_ties,
                                            &mut state.active_tuplets,
                                            &mut state.active_lyrics,
                                            &mut state.active_glissandi,
                                            &mut state.active_slides,
                                            &mut state.active_hammer_ons,
                                            &mut state.active_pull_offs,
                                            &mut state.active_tremolos,
                                            &mut state.active_wavy_lines,
                                            &state.active_octave_shifts,
                                            &mut priority_elements,
                                            staff_dist,
                                            state.slash_active,
                                            state.slash_use_stems,
                                            state.slash_dots,
                                            state.slash_note_type.as_deref(),
                                            state.beat_repeat_active,
                                            state.beat_repeat_slashes,
                                            state.current_beats,
                                            state.divisions,
                                            current_time_x_offset,
                                            &state.staff_lines,
                                            state.lyric_baseline_y,
                                            grace_offset,
                                            &measure_staff_voices,
                                        );
                                    sys_doc = new_doc;
                                    measure_render_right_edge =
                                        measure_render_right_edge.max(group_right_edge);
                                    if state.beat_repeat_active {
                                        state.beat_repeat_first = false;
                                    }
                                }
                            }
                            sys_doc = self.flush_active_beams(sys_doc, &mut active_beams);
                            // Beam groups closed by the flush: apply their final
                            // stem tips before drawing what depends on them.
                            let beam_tips: Vec<BeamedStemTip> =
                                self.beamed_stem_tips.borrow_mut().drain(..).collect();
                            Self::reanchor_slurs_to_beams(&mut state.active_slurs, &beam_tips);
                            self.apply_beam_tips_to_placed_notes(&beam_tips);
                            // Anything still waiting has no beam coming: draw it
                            // where its note put it.
                            sys_doc = self.place_beam_marks(sys_doc, &beam_tips, true);
                            sys_doc = self.flush_known_end_slurs(sys_doc, &mut state.active_slurs);
                            sys_doc = self.flush_pending_tuplets(sys_doc);
                            // A slur stop whose start never appeared in this measure
                            // can't be paired; drop it.
                            state.active_slurs.retain(|_, s| s.pending_stop.is_none());

                            // --- PRIORITY STACKING PASS ---
                            priority_elements.sort_by_key(|e| e.priority);

                            let mut measure_min_y = f32::MAX;
                            let mut measure_max_y = f32::MIN;
                            // Skip f32::MAX/MIN sentinel entries (nothing placed).
                            for &(m_min, m_max) in measure_occupancy
                                .values()
                                .filter(|o| o.0.abs() < 1e6 && o.1.abs() < 1e6)
                            {
                                measure_min_y = measure_min_y.min(m_min);
                                measure_max_y = measure_max_y.max(m_max);
                            }
                            self.measure_span.set((measure_x, measure_x + m_width));
                            self.current_measure_idx.set(m_idx);
                            let mut measure_extreme_occ = if measure_min_y <= measure_max_y {
                                Some((measure_min_y + part_start_y, measure_max_y + part_start_y))
                            } else {
                                None
                            };
                            // Slurs crossing this measure (their curves, not just the
                            // endpoints recorded in the occupancy) also bound where
                            // dynamics, hairpins and other directions may go. Only
                            // slurs within this part's vertical band count.
                            let part_band_top = part_start_y - 6.0 * self.staff_line_distance;
                            let part_band_bottom = part_start_y
                                + (num_staves as f32 - 1.0) * staff_dist
                                + 10.0 * self.staff_line_distance;
                            for sl in self.placed_slurs.borrow().iter() {
                                let ((sx1, sx2), top, bottom) = (sl.x_range(), sl.top, sl.bottom);
                                let in_measure = sx1 < measure_x + m_width && sx2 > measure_x;
                                let in_part = bottom > part_band_top && top < part_band_bottom;
                                if in_measure && in_part {
                                    measure_extreme_occ = Some(match measure_extreme_occ {
                                        Some((lo, hi)) => (lo.min(top), hi.max(bottom)),
                                        None => (top, bottom),
                                    });
                                }
                            }
                            // So do this part's tuplet brackets.
                            for &(t_part_y, _, tx1, tx2, top, bottom) in
                                self.placed_tuplets.borrow().iter()
                            {
                                if (t_part_y - part_start_y).abs() < 0.5
                                    && tx1 < measure_x + m_width
                                    && tx2 > measure_x
                                {
                                    measure_extreme_occ = Some(match measure_extreme_occ {
                                        Some((lo, hi)) => (lo.min(top), hi.max(bottom)),
                                        None => (top, bottom),
                                    });
                                }
                            }

                            let mut harmony_baseline_y: Option<f32> = None;
                            let mut lyrics_baselines_y: HashMap<i32, f32> = HashMap::new(); // verse -> y

                            // Whole-measure occupancy captured just before the first
                            // pedal is placed (pedals come after notes, dynamics and
                            // hairpins in the stacking order). Every pedal in the
                            // measure clears this — not each other, which would stack
                            // successive pedals into a staircase.
                            let occupancy_extent = |occ: &HashMap<i32, (f32, f32)>| {
                                occ.values()
                                    .filter(|o| o.0.abs() < 1e6 && o.1.abs() < 1e6)
                                    .fold(None, |acc: Option<(f32, f32)>, &(mn, mx)| {
                                        Some(acc.map_or((mn, mx), |(a, b)| (a.min(mn), b.max(mx))))
                                    })
                            };
                            let mut pedal_occ_snapshot: Option<Option<(f32, f32)>> = None;
                            // A hairpin continued from the previous system keeps
                            // that system's height; move it onto this system's
                            // dynamics line (before this measure's dynamics are
                            // placed), staying clear of every measure it crosses.
                            // Hairpins already running also stay clear of slurs drawn
                            // since they started (their line was fixed earlier); the
                            // dynamics placed below then join the adjusted line.
                            for w in state.active_wedges.values_mut() {
                                if w.is_continuation || w.resumed_line.is_some() {
                                    continue;
                                }
                                // A hairpin running on from an earlier measure must
                                // also clear this measure's notes (its line was set
                                // where it started).
                                if w.x < measure_x {
                                    let line = self.dynamics_line_y(
                                        w.is_below,
                                        w.staff,
                                        num_staves,
                                        Some(w.in_gap),
                                        part_start_y,
                                        staff_dist,
                                        measure_extreme_occ,
                                        None,
                                    );
                                    w.y = if w.is_below { w.y.max(line) } else { w.y.min(line) };
                                }
                                let half = w.start_spread.max(12.0) / 2.0;
                                // Between staves, the other staff's slurs are
                                // cleared by widening the gap instead.
                                let between = w.in_gap;
                                for sl in self.placed_slurs.borrow().iter() {
                                    let ((sx1, sx2), top, bottom) =
                                        (sl.x_range(), sl.top, sl.bottom);
                                    if sx1 >= measure_x + m_width || sx2 <= w.x {
                                        continue;
                                    }
                                    // (Nor cross-staff ones, whose far end
                                    // moves with the gap.)
                                    if between && (sl.staff != w.staff || sl.cross_staff) {
                                        continue;
                                    }
                                    // Only slurs whose curve actually reaches the
                                    // hairpin's band.
                                    let crosses =
                                        top < w.y + half + 4.0 && bottom > w.y - half - 4.0;
                                    if !crosses {
                                        continue;
                                    }
                                    if w.is_below {
                                        w.y = w.y.max(bottom + half + 6.0);
                                    } else {
                                        w.y = w.y.min(top - half - 6.0);
                                    }
                                }
                                // Likewise this staff's tuplet brackets under it.
                                for &(t_part_y, t_staff, tx1, tx2, top, bottom) in
                                    self.placed_tuplets.borrow().iter()
                                {
                                    if (t_part_y - part_start_y).abs() >= 0.5
                                        || t_staff != w.staff
                                        || tx1 >= measure_x + m_width
                                        || tx2 <= w.x
                                    {
                                        continue;
                                    }
                                    let crosses =
                                        top < w.y + half + 4.0 && bottom > w.y - half - 4.0;
                                    if !crosses {
                                        continue;
                                    }
                                    if w.is_below {
                                        w.y = w.y.max(bottom + half + 6.0);
                                    } else {
                                        w.y = w.y.min(top - half - 6.0);
                                    }
                                }
                            }
                            for w in state.active_wedges.values_mut() {
                                let Some(placed) = w.resumed_line else { continue };
                                let line = self.dynamics_line_y(
                                    w.is_below,
                                    w.staff,
                                    num_staves,
                                    Some(w.in_gap),
                                    part_start_y,
                                    staff_dist,
                                    measure_extreme_occ,
                                    None,
                                );
                                let line = match placed {
                                    Some(prev) if w.is_below => prev.max(line),
                                    Some(prev) => prev.min(line),
                                    None => line,
                                };
                                w.y = line;
                                w.resumed_line = Some(Some(line));
                            }
                            // Hairpins are only drawn at their end; register the
                            // stretch running through this measure now so the
                            // directions placed below clear it.
                            for w in state.active_wedges.values() {
                                if w.is_continuation && w.resumed_line.is_none() {
                                    continue;
                                }
                                let half = w.start_spread.max(12.0) / 2.0;
                                self.push_gap_item(
                                    w.staff,
                                    w.is_below,
                                    w.x.max(measure_x),
                                    measure_x + m_width,
                                    w.y - half,
                                    w.y + half,
                                );
                            }
                            for element in priority_elements {
                                let time = element.time_pos;
                                let x_ratio = measure_time_map.get(&time).cloned().unwrap_or(0.0);
                                let mut local_occ = measure_occupancy
                                    .get(&time)
                                    .map(|&(min, max)| (min + part_start_y, max + part_start_y));
                                // A pedal line spans the measure, so it must clear
                                // everything placed in it so far (notes, dynamics,
                                // hairpins at other beats), not just its own beat.
                                if matches!(
                                    element.item,
                                    StackedItem::DirectionType(
                                        crate::models::DirectionType::Pedal(_),
                                        _,
                                        _,
                                        _
                                    )
                                ) {
                                    local_occ = pedal_occ_snapshot
                                        .get_or_insert_with(|| occupancy_extent(&measure_occupancy))
                                        .map(|(mn, mx)| (mn + part_start_y, mx + part_start_y));
                                }

                                match element.item {
                                    StackedItem::DirectionType(dtype, placement, staff, label) => {
                                        let is_below = placement.as_deref() == Some("below");
                                        let _target_staff =
                                            staff.unwrap_or(if is_below { num_staves } else { 1 });

                                        let has_metronome = matches!(
                                            dtype,
                                            crate::models::DirectionType::Metronome(_)
                                        );
                                        let dir_x = if has_metronome {
                                            measure_x + 12.0 + x_ratio * (m_width - 24.0)
                                        } else {
                                            content_start_x
                                                + x_ratio * content_width
                                                + current_time_x_offset
                                        };

                                        let temp_dir = crate::models::Direction {
                                            placement,
                                            types: label
                                                .map(crate::models::DirectionType::Words)
                                                .into_iter()
                                                .chain([dtype])
                                                .collect(),
                                            staff,
                                        };

                                        let (new_doc, new_occ) = self.draw_direction(
                                            sys_doc,
                                            &temp_dir,
                                            part_start_y,
                                            dir_x,
                                            num_staves,
                                            local_occ,
                                            staff_dist,
                                            &mut state.active_brackets,
                                            &mut state.active_dashes,
                                            &mut state.active_wedges,
                                            &mut state.active_glissandi,
                                            &mut state.active_octave_shifts,
                                            &mut state.active_pedals,
                                            measure_extreme_occ,
                                        );
                                        sys_doc = new_doc;
                                        if let Some((new_min, new_max)) = new_occ {
                                            measure_occupancy.insert(
                                                time,
                                                (new_min - part_start_y, new_max - part_start_y),
                                            );
                                        }
                                    }
                                    StackedItem::Harmony(harmony) => {
                                        if harmony_baseline_y.is_none() {
                                            let mut best_y = part_start_y - 20.0;
                                            for (&_t, &(m_min, _)) in &measure_occupancy {
                                                best_y = best_y.min(m_min + part_start_y - 6.0);
                                            }
                                            if harmony.frame.is_some() {
                                                best_y -= 60.0;
                                            }
                                            harmony_baseline_y = Some(best_y);
                                        }
                                        let harmony_x = content_start_x
                                            + x_ratio * content_width
                                            + current_time_x_offset;
                                        sys_doc = self.render_harmony(
                                            sys_doc,
                                            &harmony,
                                            harmony_x,
                                            harmony_baseline_y.unwrap(),
                                        );
                                        let entry =
                                            measure_occupancy.entry(time).or_insert((0.0, 0.0));
                                        entry.0 = entry
                                            .0
                                            .min(harmony_baseline_y.unwrap() - part_start_y - 6.0);
                                    }
                                    StackedItem::Lyric(lyric, staff) => {
                                        let verse = lyric.number.unwrap_or(1);
                                        if !lyrics_baselines_y.contains_key(&verse) {
                                            let mut best_y =
                                                part_start_y + (staff as f32) * staff_dist + 8.0;
                                            for (&_t, &(_, m_max)) in &measure_occupancy {
                                                best_y = best_y.max(
                                                    m_max
                                                        + part_start_y
                                                        + 8.0
                                                        + (verse - 1) as f32 * 10.0,
                                                );
                                            }
                                            lyrics_baselines_y.insert(verse, best_y);
                                        }
                                        let lyric_x = content_start_x
                                            + x_ratio * content_width
                                            + current_time_x_offset;
                                        let base = system_lyric_baselines
                                            .entry(verse)
                                            .or_insert(f32::MIN);
                                        *base = base.max(*lyrics_baselines_y.get(&verse).unwrap());
                                        pending_lyrics.push((lyric, lyric_x, staff));
                                        let entry =
                                            measure_occupancy.entry(time).or_insert((0.0, 0.0));
                                        entry.1 = entry.1.max(
                                            *lyrics_baselines_y.get(&verse).unwrap() - part_start_y
                                                + 8.0,
                                        );
                                    }
                                }
                            }
                            // A bracket stop whose start never came in this
                            // measure can't be paired; drop it.
                            state.active_brackets.retain(|_, b| b.pending_stop.is_none());
                            // Pedal lines still open after this measure must also
                            // clear it, so the whole line stays flat below (or
                            // above) everything it passes.
                            // (Uses the pre-pedal snapshot when this measure placed
                            // pedals, so a pedal doesn't push itself down.)
                            if let Some((lo, hi)) = pedal_occ_snapshot
                                .unwrap_or_else(|| occupancy_extent(&measure_occupancy))
                            {
                                for p in state.active_pedals.values_mut() {
                                    if p.is_continuation {
                                        continue;
                                    }
                                    if p.is_below {
                                        p.y = p.y.max(hi + part_start_y + 30.0);
                                    } else {
                                        p.y = p.y.min(lo + part_start_y - 30.0);
                                    }
                                }
                            }

                        }

                        if stop_beat_repeat_after_this {
                            state.beat_repeat_active = false;
                        }

                        if !has_right_barline {
                            let mut y_bottom_total = part_start_y;
                            let right_barline_x =
                                measure_render_right_edge.max(measure_x + m_width);
                            for s in 0..num_staves {
                                let lines = *state.staff_lines.get(&(s + 1)).unwrap_or(&5);
                                let start_i = (5 - lines) / 2;
                                let staff_top = part_start_y
                                    + (s as f32 * staff_dist)
                                    + (start_i as f32 * self.staff_line_distance);
                                let staff_bottom =
                                    staff_top + ((lines - 1) as f32 * self.staff_line_distance);
                                y_bottom_total = y_bottom_total.max(staff_bottom);
                            }
                            sys_doc = sys_doc.add(
                                Line::new()
                                    .set("x1", right_barline_x)
                                    .set("y1", part_start_y)
                                    .set("x2", right_barline_x)
                                    .set("y2", y_bottom_total)
                                    .set("stroke", "black")
                                    .set("stroke-width", 1),
                            );
                            for (group, g_start_y, g_end_y) in &current_system_group_ranges {
                                if group.barline.as_deref() == Some("yes")
                                    && (part_start_y - *g_start_y).abs() < 1.0
                                {
                                    sys_doc = sys_doc.add(
                                        Line::new()
                                            .set("x1", right_barline_x)
                                            .set("y1", *g_start_y)
                                            .set("x2", right_barline_x)
                                            .set("y2", *g_end_y)
                                            .set("stroke", "black")
                                            .set("stroke-width", 1),
                                    );
                                }
                            }
                        }

                        // System break handling for active spanners
                        if m_idx == *system.measure_indices.last().unwrap() {
                            // 0. Lyrics collected over the system, before the
                            // system-end extenders below continue them.
                            Self::stack_verse_baselines(&mut system_lyric_baselines);
                            for (lyric, lyric_x, staff) in pending_lyrics.drain(..) {
                                let verse = lyric.number.unwrap_or(1);
                                let (new_doc, _) = self.draw_lyric(
                                    sys_doc,
                                    &lyric,
                                    lyric_x,
                                    part_start_y,
                                    staff,
                                    0.0,
                                    staff_dist,
                                    &mut state.active_lyrics,
                                    system_lyric_baselines[&verse],
                                );
                                sys_doc = new_doc;
                            }
                            let system_end_x = measure_x + m_width;

                            // 1. Endings
                            if id == &score.parts[0].id {
                                if let Some(start_info) = state.active_ending.clone() {
                                    sys_doc = self.draw_volta_bracket(
                                        sys_doc,
                                        start_info.x,
                                        start_info.y,
                                        system_end_x,
                                        start_info.y,
                                        &start_info.text,
                                        !start_info.is_continuation,
                                        false,
                                    );
                                    state.active_ending = Some(EndingStartInfo {
                                        x: current_margin_left,
                                        y: start_info.y - part_start_y,
                                        text: "".to_string(),
                                        is_continuation: true,
                                    });
                                }
                            }
                            // 2. Pedals
                            for (&_num, start_info) in state.active_pedals.iter_mut() {
                                start_info.y = self.pedal_y_clear_of_wedges(
                                    start_info.y,
                                    start_info.x,
                                    system_end_x,
                                    start_info.is_below,
                                    &state.active_wedges,
                                    self.current_part_y.get(),
                                );
                                sys_doc = self.draw_pedal_line(
                                    sys_doc,
                                    start_info.x,
                                    start_info.y,
                                    system_end_x,
                                    start_info.is_initial,
                                    false,
                                    false,
                                    &start_info.glyph,
                                    start_info.is_below,
                                );
                                start_info.x = current_margin_left;
                                start_info.y = start_info.y - part_start_y;
                                start_info.is_initial = true;
                                start_info.glyph = "\u{E651}".to_string(); // (Ped.)
                                start_info.is_continuation = true;
                            }
                            // 2.5 Wedges
                            for (&_num, start_info) in state.active_wedges.iter_mut() {
                                let mid_spread = 8.0; // Intermediate spread for broken wedges
                                sys_doc = self.draw_wedge(
                                    sys_doc,
                                    start_info.x,
                                    start_info.y,
                                    system_end_x,
                                    start_info.y,
                                    &start_info.wedge_type,
                                    start_info.start_spread,
                                    mid_spread,
                                    start_info.staff,
                                    start_info.is_below,
                                );
                                start_info.x = current_margin_left;
                                start_info.y = start_info.y - part_start_y; // Store as relative offset
                                start_info.is_continuation = true;
                                start_info.start_spread = mid_spread;
                                // Re-placed on the next system's dynamics line.
                                start_info.resumed_line = Some(None);
                            }
                            // 3. Octave Shifts
                            for (&_num, start_info) in state.active_octave_shifts.iter_mut() {
                                start_info.y = self.octave_shift_clear_y(start_info, system_end_x);
                                sys_doc = self.draw_octave_shift(
                                    sys_doc,
                                    start_info.x,
                                    start_info.y,
                                    system_end_x,
                                    start_info.y,
                                    start_info.size,
                                    &start_info.shift_type,
                                    start_info.is_below,
                                    start_info.staff.unwrap_or(1),
                                );
                                start_info.x = current_margin_left;
                                // Relative to its own staff's top line.
                                start_info.y -= part_start_y
                                    + (start_info.staff.unwrap_or(1) - 1) as f32 * staff_dist;
                                start_info.is_continuation = true;
                            }
                            // 4. Groupings
                            for (&_num, start_info) in state.active_groupings.iter_mut() {
                                sys_doc = self.draw_grouping_bracket(
                                    sys_doc,
                                    start_info.x,
                                    start_info.y,
                                    system_end_x,
                                    start_info.y,
                                    &start_info.features,
                                );
                                start_info.x = current_margin_left;
                                start_info.y = start_info.y - part_start_y;
                                start_info.is_continuation = true;
                            }
                            // 5. Dashes
                            for (&_num, start_info) in state.active_dashes.iter_mut() {
                                let dash_len = start_info.dash_length.unwrap_or(5.0);
                                let space_len = start_info.space_length.unwrap_or(5.0);
                                let mut line = Line::new()
                                    .set("x1", start_info.x)
                                    .set("y1", start_info.y)
                                    .set("x2", system_end_x)
                                    .set("y2", start_info.y)
                                    .set("stroke", "black")
                                    .set("stroke-width", 1);
                                line = line
                                    .set("stroke-dasharray", format!("{},{}", dash_len, space_len));
                                sys_doc = sys_doc.add(line);
                                start_info.x = current_margin_left;
                                start_info.y = start_info.y - part_start_y; // Store as relative
                                start_info.is_continuation = true;
                            }
                            // 5.5 Brackets: run to the system's end (open there,
                            // with the start's hook) and resume on the next.
                            for (&_num, start_info) in state.active_brackets.iter_mut() {
                                sys_doc = self.draw_bracket_span(
                                    sys_doc,
                                    start_info,
                                    system_end_x,
                                    &None,
                                );
                                start_info.x = current_margin_left;
                                start_info.y = start_info.y - part_start_y;
                                start_info.line_end = None;
                                start_info.label = None;
                                start_info.is_continuation = true;
                            }
                            // 6. Slurs
                            // Unmatched pending stops (and, defensively, any unflushed
                            // finished slur) never carry over a system break.
                            state.active_slurs
                                .retain(|_, s| s.pending_stop.is_none() && s.known_end.is_none());
                            for (&_num, start_info) in state.active_slurs.iter_mut() {
                                // End this system's segment at the height of the last
                                // note before the break (not the start note's height),
                                // so a slur over a rising/falling line doesn't have to
                                // bulge over the whole passage to come back to where it
                                // began. The next system's segment resumes at that same
                                // height.
                                let end_y = Self::slur_break_y(start_info);
                                let start_y =
                                    Self::resumed_slur_start_y(start_info, &start_info.obstacles);
                                sys_doc = self.draw_slur(
                                    sys_doc,
                                    start_info.x,
                                    start_y,
                                    system_end_x,
                                    end_y,
                                    start_info.is_above,
                                    &start_info.obstacles,
                                    start_info.staff,
                                );
                                start_info.obstacles.clear();
                                start_info.x = current_margin_left;
                                // Store relative to the slur's own staff.
                                start_info.y = end_y
                                    - part_start_y
                                    - (start_info.staff - 1).max(0) as f32 * staff_dist;
                                start_info.is_continuation = true;
                            }
                            // 7. Ties
                            for (_, start_info) in state.active_ties.iter_mut() {
                                sys_doc = self.draw_tie(
                                    sys_doc,
                                    start_info.x,
                                    start_info.y,
                                    system_end_x,
                                    start_info.y,
                                    start_info.is_above,
                                );
                                start_info.x = current_margin_left;
                                start_info.y = start_info.y - part_start_y; // Store as relative
                                start_info.is_continuation = true;
                            }
                            // 8. Wavy Lines
                            for (&_num, start_info) in state.active_wavy_lines.iter_mut() {
                                sys_doc = self.draw_wavy_line(
                                    sys_doc,
                                    start_info.x,
                                    start_info.y,
                                    system_end_x,
                                    start_info.y,
                                );
                                start_info.x = current_margin_left;
                                start_info.y = start_info.y - part_start_y; // Store as relative offset
                                start_info.is_continuation = true;
                            }
                            // 9. Tremolos
                            for (&_num, start_info) in state.active_tremolos.iter_mut() {
                                sys_doc = self.draw_multi_note_tremolo(
                                    sys_doc,
                                    start_info.x,
                                    start_info.y,
                                    system_end_x,
                                    start_info.y,
                                    start_info.bars,
                                );
                                start_info.x = current_margin_left;
                                start_info.y = start_info.y - part_start_y;
                                start_info.is_continuation = true;
                            }
                            // 10. Glissandi
                            for (&_num, start_info) in state.active_glissandi.iter_mut() {
                                sys_doc = self.draw_glissando(
                                    sys_doc,
                                    start_info.x,
                                    start_info.y,
                                    system_end_x,
                                    start_info.y,
                                    &start_info.line_type,
                                    &start_info.text,
                                    start_info.is_continuation,
                                    true,
                                );
                                start_info.x = current_margin_left;
                                start_info.y = start_info.y - part_start_y;
                                start_info.is_continuation = true;
                                start_info.text = None; // Only draw text on the first segment
                            }
                            // 11. Slides
                            for (&_num, start_info) in state.active_slides.iter_mut() {
                                sys_doc = self.draw_slide(
                                    sys_doc,
                                    start_info.x,
                                    start_info.y,
                                    system_end_x,
                                    start_info.y,
                                    &start_info.line_type,
                                    start_info.is_continuation,
                                    true,
                                );
                                start_info.x = current_margin_left;
                                start_info.y = start_info.y - part_start_y;
                                start_info.is_continuation = true;
                            }
                            // 12. Lyrics
                            for (&_num, start_info) in state.active_lyrics.iter_mut() {
                                let line = Line::new()
                                    .set("x1", start_info.x)
                                    .set("y1", start_info.y)
                                    .set("x2", system_end_x)
                                    .set("y2", start_info.y)
                                    .set("stroke", "black")
                                    .set("stroke-width", 1);
                                sys_doc = sys_doc.add(line);
                                start_info.x = current_margin_left;
                                start_info.y = start_info.y - part_start_y; // Store as relative
                                start_info.is_continuation = true;
                            }
                        }

                        if m_width > 0.0 {
                            measure_x += m_width;
                        }
                    }
                    // The system's last measure was skipped (e.g. inside a
                    // multi-measure rest): still draw what was collected.
                    Self::stack_verse_baselines(&mut system_lyric_baselines);
                    for (lyric, lyric_x, staff) in pending_lyrics.drain(..) {
                        let verse = lyric.number.unwrap_or(1);
                        let (new_doc, _) = self.draw_lyric(
                            sys_doc,
                            &lyric,
                            lyric_x,
                            part_start_y,
                            staff,
                            0.0,
                            staff_dist,
                            &mut state.active_lyrics,
                            system_lyric_baselines[&verse],
                        );
                        sys_doc = new_doc;
                    }
                }
            }
            if layout_attempt < 2 {
                let needed = self.inter_staff_slur_overlaps(
                    score,
                    &part_states,
                    &part_y_offsets,
                    &part_staff_distances,
                    system_render_y,
                );
                let part_needed = self.part_overlaps(score, &part_y_offsets, system_render_y);
                if !needed.is_empty() || !part_needed.is_empty() {
                    for (id, extra) in needed {
                        *extra_staff_gap.entry(id).or_insert(0.0) += extra;
                    }
                    for (id, extra) in part_needed {
                        *extra_part_gap.entry(id).or_insert(0.0) += extra;
                    }
                    part_states = part_states_at_system_start.clone();
                    layout_attempt += 1;
                    continue;
                }
            }
            sys_doc = self.flush_deferred_pedals(sys_doc);
            current_y = system_render_y + total_system_height;
            // Grow this system's slice to fit slurs/pedal lines that reach past
            // the laid-out height: push the following systems down by the bottom overflow,
            // and start the slice higher for a top overflow (that space is the
            // previous system's trailing gap; each slice only holds its own
            // system's elements, so the slices may overlap).
            // Text (directions, fingerings, articulation glyphs, ...) can also
            // be placed past the laid-out height. Measure what was actually
            // drawn, ignoring anything implausibly far from this system.
            let mut text_ext = None;
            Self::text_extent(
                sys_doc.get_children(),
                sys_y_before - 300.0,
                current_y + 300.0,
                &mut text_ext,
            );
            const SLUR_SLICE_PAD: f32 = 4.0;
            let mut slice_y_start = sys_y_before;
            if let Some((slur_top, slur_bottom)) = self.drawn_extent.get() {
                current_y = current_y.max(slur_bottom + SLUR_SLICE_PAD);
                slice_y_start = slice_y_start.min(slur_top - SLUR_SLICE_PAD);
            }
            // The slice also holds the gap below the system, so text only
            // pushes the next system down once it reaches past that gap.
            let gap_below = if sys_idx + 1 < systems.len() { 100.0 } else { 0.0 };
            if let Some((top, bottom)) = text_ext {
                slice_y_start = slice_y_start.min(top - SLUR_SLICE_PAD);
                current_y = current_y.max(bottom + SLUR_SLICE_PAD - gap_below);
            }
            current_y += gap_below; // Space between systems

            // Record system boundary and measure-to-system mapping
            sys_boundaries_vec.push(crate::models::SystemBoundary {
                y_start: slice_y_start,
                height: (current_y - slice_y_start).max(1.0),
                measure_start: *system.measure_indices.first().unwrap_or(&0),
                measure_end: *system.measure_indices.last().unwrap_or(&0),
            });
            for &mi in &system.measure_indices {
                measure_to_sys.insert(mi, sys_idx);
            }

            // Wrap this system's elements in a named group and add to document
            {
                let s = sys_doc.to_string();
                let inner = if let (Some(gt), Some(end)) = (s.find('>'), s.rfind("</svg>")) {
                    if gt + 1 < end { &s[gt + 1..end] } else { "" }
                } else {
                    ""
                };
                document = document.add(Blob::new(format!("<g id=\"s{}\">{}</g>", sys_idx, inner)));
            }

            for (m_idx, x, w, beats, beat_type, _div, time_map) in measure_coords_in_system {
                all_measure_render_info.insert(
                    m_idx,
                    (x, w, beats, beat_type, _div, time_map, system_metadata_y),
                );
            }
                    break;
            }
}

        let mut metadata = PlayMetadata::default();
        let timeline = crate::timeline::TimelineSolver::solve(score_in);
        let timing_map = crate::midi_engine::MidiEngine::generate_timing_map(score_in, &timeline);
        let mut cursor_anchors: HashMap<usize, Vec<(i32, f32)>> = HashMap::new();

        for timing_event in timing_map {
            let m_idx = timing_event.measure_index;
            if let Some(&(
                x_base,
                m_width,
                _beats,
                _beat_type,
                _divisions,
                ref time_to_x,
                (y_start, y_end),
            )) = all_measure_render_info.get(&m_idx)
            {
                let t = timing_event.tick_offset;
                let t_scaled = (t as f64 * 10080.0 / 480.0).round() as i32;

                // Find X coordinate: the drawn onset (or grace note, at the
                // time it sounds) nearest in time.
                let anchors = cursor_anchors
                    .entry(m_idx)
                    .or_insert_with(|| self.cursor_anchors(m_idx, x_base, m_width, time_to_x));
                let final_x = anchors
                    .iter()
                    .min_by_key(|&&(at, _)| (at - t_scaled).abs())
                    .map_or(x_base, |&(_, x)| x);

                metadata.beats.push(crate::models::BeatEvent {
                    x: final_x,
                    y_start,
                    y_end,
                    measure_index: m_idx,
                    measure_number: timing_event.measure_number.clone(),
                    beat_number: timing_event.beat_number,
                    absolute_beat: timing_event.absolute_beat,
                    time_seconds: timing_event.time_seconds,
                    system_index: *measure_to_sys.get(&m_idx).unwrap_or(&0),
                });
            }
        }

        if self.debug_metadata {
            for (idx, beat) in metadata.beats.iter().enumerate() {
                document = document.add(
                    Line::new()
                        .set("x1", beat.x)
                        .set("y1", beat.y_start)
                        .set("x2", beat.x)
                        .set("y2", beat.y_end)
                        .set("stroke", "red")
                        .set("stroke-width", 1)
                        .set("stroke-opacity", 0.5),
                );

                let y_offset = 5.0 + (idx % 3) as f32 * 10.0;
                document = document.add(
                    Text::new(format!("{}:{:.2}s", beat.absolute_beat, beat.time_seconds))
                        .set("x", beat.x)
                        .set("y", beat.y_start - y_offset)
                        .set("font-size", 10)
                        .set("fill", "red")
                        .set("text-anchor", "middle")
                        .set("font-family", "sans-serif"),
                );
            }
        }

        metadata.system_boundaries = sys_boundaries_vec;

        (
            document
                .set(
                    "viewBox",
                    (0, 0, final_width, current_y + self.margin_right),
                )
                .set("width", final_width)
                .set("height", current_y + self.margin_right)
                .to_string(),
            metadata,
        )
    }

    /// Each verse's lyric line at least one line below the previous verse's,
    /// so verses never overprint (a measure whose notes stay inside the staff
    /// would otherwise give every verse the same height).
    fn stack_verse_baselines(baselines: &mut HashMap<i32, f32>) {
        const LYRIC_LINE: f32 = 15.0;
        let mut verses: Vec<i32> = baselines.keys().copied().collect();
        verses.sort_unstable();
        let mut prev: Option<f32> = None;
        for v in verses {
            let y = baselines.get_mut(&v).unwrap();
            if let Some(p) = prev {
                *y = y.max(p + LYRIC_LINE);
            }
            prev = Some(*y);
        }
    }

    /// Direction text whose SMuFL symbols (private-use code points, e.g. the
    /// flat MuseScore writes after a chord root) are set in the music font:
    /// the text font has no glyphs for them.
    fn words_text(&self, text: &str) -> Text {
        let is_smufl = |c: char| ('\u{E000}'..='\u{F8FF}').contains(&c);
        if !text.chars().any(is_smufl) {
            return Text::new(text);
        }
        let mut node = Text::new("");
        let mut run = String::new();
        let mut run_smufl = false;
        for c in text.chars().chain(std::iter::once('\0')) {
            let end = c == '\0';
            if !run.is_empty() && (end || is_smufl(c) != run_smufl) {
                let span = svg::node::element::TSpan::new(std::mem::take(&mut run));
                node = node.add(if run_smufl {
                    span.set("font-family", self.font_family.as_str())
                        .set("font-style", "normal")
                } else {
                    span
                });
            }
            if !end {
                run_smufl = is_smufl(c);
                run.push(c);
            }
        }
        node
    }

    fn draw_direction(
        &self,
        mut doc: Document,
        dir: &Direction,
        part_start_y: f32,
        x: f32,
        num_staves: i32,
        occupancy: Option<(f32, f32)>,
        staff_dist: f32,
        active_brackets: &mut HashMap<i32, BracketStartInfo>,
        active_dashes: &mut HashMap<i32, DashesStartInfo>,
        active_wedges: &mut HashMap<i32, WedgeStartInfo>,
        _active_glissandi: &mut HashMap<i32, GlissandoStartInfo>,
        active_octave_shifts: &mut HashMap<i32, OctaveShiftStartInfo>,
        active_pedals: &mut HashMap<i32, PedalStartInfo>,
        measure_extreme_occ: Option<(f32, f32)>,
    ) -> (Document, Option<(f32, f32)>) {
        let mut local_occ = occupancy;

        let is_below = dir.placement.as_deref() == Some("below");
        let target_staff = dir.staff.unwrap_or(if is_below { num_staves } else { 1 });
        let mut shared_y = if is_below {
            part_start_y + (target_staff as f32 - 1.0) * staff_dist + 6.0 * self.staff_line_distance
        } else {
            part_start_y + (target_staff as f32 - 1.0) * staff_dist - 2.0 * self.staff_line_distance
        };
        let between = self.in_staff_gap(target_staff, is_below, num_staves);
        // Between staves, clear only this staff's notes (see `dynamics_line_y`).
        let clear_occ = if between {
            self.staff_extent_in_measure(target_staff)
        } else {
            local_occ
        };
        if let Some((min_y, max_y)) = clear_occ {
            if is_below {
                shared_y = shared_y.max(max_y + 20.0);
            } else {
                shared_y = shared_y.min(min_y - 20.0);
            }
        }

        let mut local_x = x;
        // Text leading a bracket or dashes line started here: printed at the
        // line's height, the line going on right after it (not stacked apart).
        let line_label: Option<&str> = dir
            .types
            .iter()
            .any(|t| {
                matches!(t, crate::models::DirectionType::Bracket(b) if b.bracket_type == "start")
                    || matches!(t, crate::models::DirectionType::Dashes(d) if d.dashes_type == "start")
            })
            .then(|| {
                dir.types.iter().find_map(|t| match t {
                    crate::models::DirectionType::Words(w) => Some(w.as_str()),
                    _ => None,
                })
            })
            .flatten();
        let draw_line_label = |doc: Document, x: f32| -> (Document, f32) {
            let Some(text) = line_label else { return (doc, x) };
            let text_w = text.chars().count() as f32 * 6.5;
            let node = self
                .words_text(text)
                .set("x", x)
                .set("y", shared_y)
                .set("font-size", 12)
                .set("text-anchor", "start")
                .set("dominant-baseline", "central")
                .set("font-family", "serif")
                .set("font-style", "italic");
            (doc.add(node), x + text_w + 4.0)
        };

        for dtype in &dir.types {
            match dtype {
                crate::models::DirectionType::Dynamics(dyns) => {
                    let mut dyn_y = self.dynamics_line_y(
                        is_below,
                        target_staff,
                        num_staves,
                        None,
                        part_start_y,
                        staff_dist,
                        measure_extreme_occ,
                        occupancy,
                    );
                    // A dynamic under a hairpin still in progress (typically the
                    // one it ends on) joins the hairpin's line — which may have
                    // been pushed further out by earlier measures it crosses.
                    for w in active_wedges.values() {
                        if w.is_continuation || w.is_below != is_below || w.x >= local_x {
                            continue;
                        }
                        if w.staff == target_staff || !is_below {
                            dyn_y = if is_below { dyn_y.max(w.y) } else { dyn_y.min(w.y) };
                        }
                    }

                    let mut dyn_str = String::new();
                    let combined: String = dyns.join("");
                    let smufl_combined = self.dynamic_to_smufl(&combined);
                    if smufl_combined != combined {
                        dyn_str = smufl_combined;
                    } else {
                        for d in dyns {
                            dyn_str.push_str(&self.dynamic_to_smufl(&d));
                        }
                    }
                    let half_w = self.estimate_dynamics_width(dyns) / 2.0;
                    // Also clear the drawn notes right at the dynamic (their
                    // stems as finally set by beams, which the measured
                    // occupancy doesn't know).
                    dyn_y = self.clear_of_placed_notes(
                        local_x - half_w,
                        local_x + half_w,
                        10.0,
                        6.0,
                        dyn_y,
                        is_below,
                        between.then_some(target_staff),
                    );
                    self.push_gap_item(
                        target_staff,
                        is_below,
                        local_x - half_w,
                        local_x + half_w,
                        dyn_y - 10.0,
                        dyn_y + 6.0,
                    );
                    self.placed_text_boxes.borrow_mut().push((
                        local_x - half_w,
                        local_x + half_w,
                        dyn_y - 10.0,
                        dyn_y + 10.0,
                        is_below,
                    ));
                    self.placed_dynamics.borrow_mut().push((
                        local_x - half_w,
                        local_x + half_w,
                        dyn_y,
                        is_below,
                        target_staff,
                    ));
                    let text = Text::new(dyn_str)
                        .set("x", local_x)
                        .set("y", dyn_y)
                        .set("font-size", self.staff_line_distance * 2.6)
                        .set("text-anchor", "middle")
                        .set("dominant-baseline", "central")
                        .set("font-family", self.font_family.as_str());
                    doc = doc.add(text);
                    if is_below {
                        local_occ = Some((local_occ.map(|o| o.0).unwrap_or(dyn_y), dyn_y + 15.0));
                    } else {
                        local_occ = Some((dyn_y - 15.0, local_occ.map(|o| o.1).unwrap_or(dyn_y)));
                    }
                }
                crate::models::DirectionType::Words(_) if line_label.is_some() => {
                    // Drawn with its line below.
                }
                crate::models::DirectionType::Words(text_str) => {
                    let mut text_y = shared_y;
                    if let Some((min_y, max_y)) = measure_extreme_occ.filter(|_| !between) {
                        if is_below {
                            text_y = text_y.max(max_y + 15.0);
                        } else {
                            text_y = text_y.min(min_y - 15.0);
                        }
                    }
                    // Keep clear of direction texts already placed nearby.
                    let text_w = text_str.chars().count() as f32 * 6.5;
                    text_y = self.clear_of_text_boxes(
                        local_x,
                        local_x + text_w,
                        7.0,
                        7.0,
                        text_y,
                        is_below,
                    );
                    if between {
                        // Also clear the hairpins and other lines of this staff
                        // in the gap (the part-wide occupancy isn't used here).
                        text_y = self.clear_of_gap_items(
                            target_staff,
                            local_x,
                            local_x + text_w,
                            7.0,
                            7.0,
                            text_y,
                            is_below,
                        );
                    }
                    self.placed_text_boxes.borrow_mut().push((
                        local_x,
                        local_x + text_w,
                        text_y - 7.0,
                        text_y + 7.0,
                        is_below,
                    ));
                    self.push_gap_item(
                        target_staff,
                        is_below,
                        local_x,
                        local_x + text_w,
                        text_y - 7.0,
                        text_y + 7.0,
                    );
                    let text = self
                        .words_text(text_str)
                        .set("x", local_x)
                        .set("y", text_y)
                        .set("font-size", 12)
                        .set("text-anchor", "start")
                        .set("dominant-baseline", "central")
                        .set("font-family", "serif")
                        .set("font-style", "italic");
                    doc = doc.add(text);
                    local_x += text_str.len() as f32 * 7.0 + 5.0; // Rough estimate of text width
                    if is_below {
                        local_occ = Some((local_occ.map(|o| o.0).unwrap_or(text_y), text_y + 10.0));
                    } else {
                        local_occ = Some((text_y - 10.0, local_occ.map(|o| o.1).unwrap_or(text_y)));
                    }
                }
                crate::models::DirectionType::Metronome(met) => {
                    let is_below = dir.placement.as_deref() == Some("below");
                    let (new_doc, min_y, max_y) = self.draw_metronome_mark(
                        doc,
                        &met,
                        x,
                        part_start_y,
                        num_staves,
                        local_occ,
                        staff_dist,
                        is_below,
                    );
                    doc = new_doc;
                    local_occ = Some((min_y, max_y));
                }
                crate::models::DirectionType::Rehearsal(text_str) => {
                    let mut y_offset = part_start_y - 4.0 * self.staff_line_distance;
                    if let Some((min_y, _)) = local_occ {
                        y_offset = y_offset.min(min_y - 30.0);
                    }

                    let font_size = 12.0;
                    let padding = 4.0;
                    let text_w = text_str.len() as f32 * 7.0;
                    let box_w = text_w + padding * 2.0;
                    let box_h = font_size + padding * 2.0;

                    let rect = svg::node::element::Rectangle::new()
                        .set("x", x - box_w / 2.0)
                        .set("y", y_offset - box_h / 2.0)
                        .set("width", box_w)
                        .set("height", box_h)
                        .set("fill", "none")
                        .set("stroke", "black")
                        .set("stroke-width", 1.5);

                    let text = Text::new(text_str.as_str())
                        .set("x", x)
                        .set("y", y_offset + 4.0) // Lowered by 4px for better vertical centering
                        .set("font-size", font_size)
                        .set("text-anchor", "middle")
                        .set("dominant-baseline", "central")
                        .set("font-family", "serif")
                        .set("font-weight", "bold");

                    doc = doc.add(rect).add(text);
                    local_occ = Some((y_offset - 20.0, local_occ.map(|o| o.1).unwrap_or(y_offset)));
                }
                crate::models::DirectionType::Bracket(bracket) => {
                    let num = bracket.number.unwrap_or(1);
                    if bracket.bracket_type == "start" {
                        // The line and its label are drawn at its stop (or the
                        // system's end), at a height clearing all it spans.
                        let label_w = line_label.map_or(0.0, |t| t.chars().count() as f32 * 6.5 + 4.0);
                        // Its stop already came (later in time or at the same
                        // beat, earlier in the file).
                        let early_stop = active_brackets
                            .get(&num)
                            .and_then(|b| b.pending_stop.clone())
                            .filter(|(stop_x, _)| *stop_x > x - 0.5);
                        if early_stop.is_some() {
                            active_brackets.remove(&num);
                        }
                        active_brackets.insert(
                            num,
                            BracketStartInfo {
                                x: x + label_w,
                                y: shared_y,
                                line_type: bracket.line_type.clone(),
                                line_end: bracket.line_end.clone(),
                                is_continuation: false,
                                label: line_label.map(|t| (t.to_string(), x)),
                                is_below,
                                staff: target_staff,
                                pending_stop: None,
                            },
                        );
                        if let Some((stop_x, stop_end)) = early_stop {
                            let start_info = active_brackets.remove(&num).unwrap();
                            // (One starting and stopping on the same beat spans nothing.)
                            if stop_x > start_info.x + 1.0 {
                                doc = self.draw_bracket_span(doc, &start_info, stop_x, &stop_end);
                            }
                        }
                    } else if bracket.bracket_type == "stop" {
                        match active_brackets.remove(&num) {
                            Some(start_info) if start_info.pending_stop.is_none() => {
                                doc = self.draw_bracket_span(doc, &start_info, x, &bracket.line_end);
                            }
                            _ => {
                                // No start yet: keep the stop for a start at an
                                // earlier beat further on in this measure.
                                active_brackets.insert(
                                    num,
                                    BracketStartInfo {
                                        x,
                                        y: shared_y,
                                        line_type: None,
                                        line_end: None,
                                        is_continuation: false,
                                        label: None,
                                        is_below,
                                        staff: target_staff,
                                        pending_stop: Some((x, bracket.line_end.clone())),
                                    },
                                );
                            }
                        }
                    }
                    // A stop takes no new room: the line was placed at its
                    // start, and a bracket starting on the same beat (the next
                    // barre) should stay on that line, not stack above it.
                    if bracket.bracket_type != "stop" {
                        if is_below {
                            local_occ = Some((
                                local_occ.map(|o| o.0).unwrap_or(shared_y),
                                shared_y + 10.0,
                            ));
                        } else {
                            local_occ = Some((
                                shared_y - 10.0,
                                local_occ.map(|o| o.1).unwrap_or(shared_y),
                            ));
                        }
                    }
                }
                crate::models::DirectionType::Coda => {
                    let mut y_offset = part_start_y - 2.5 * self.staff_line_distance;
                    if let Some((min_y, _)) = local_occ {
                        y_offset = y_offset.min(min_y - 20.0);
                    }
                    let text = Text::new("\u{E048}")
                        .set("x", x)
                        .set("y", y_offset)
                        .set("font-size", self.staff_line_distance * 2.0)
                        .set("text-anchor", "middle")
                        .set("dominant-baseline", "central")
                        .set("font-family", self.font_family.as_str());
                    doc = doc.add(text);
                    local_occ = Some((y_offset - 15.0, local_occ.map(|o| o.1).unwrap_or(y_offset)));
                }
                crate::models::DirectionType::Segno => {
                    let mut y_offset = part_start_y - 2.5 * self.staff_line_distance;
                    if let Some((min_y, _)) = local_occ {
                        y_offset = y_offset.min(min_y - 20.0);
                    }
                    let text = Text::new("\u{E047}")
                        .set("x", x)
                        .set("y", y_offset)
                        .set("font-size", self.staff_line_distance * 2.0)
                        .set("text-anchor", "middle")
                        .set("dominant-baseline", "central")
                        .set("font-family", self.font_family.as_str());
                    doc = doc.add(text);
                    local_occ = Some((y_offset - 15.0, local_occ.map(|o| o.1).unwrap_or(y_offset)));
                }
                crate::models::DirectionType::Wedge(wedge) => {
                    let num = wedge.number.unwrap_or(1);
                    let is_below = dir.placement.as_deref() != Some("above");
                    // Same line as the dynamics (mf, f, …) of this staff/measure.
                    // Not pushed by this beat's own occupancy, which would put
                    // the hairpin under a dynamic at the same beat.
                    let wedge_staff = dir.staff.unwrap_or(if is_below { num_staves } else { 1 });
                    // In the gap or outside the part: decided once for the
                    // whole hairpin. A running one keeps its decision.
                    let mut wedge_in_gap = match active_wedges.get(&num) {
                        Some(w) if wedge.wedge_type == "stop" => w.in_gap,
                        _ => {
                            Self::between_staves(wedge_staff, is_below, num_staves)
                                && !self.outer_wedges.borrow().get(&*self.current_part_id.borrow()).is_some_and(
                                    |set| set.contains(&(self.current_measure_idx.get(), num)),
                                )
                        }
                    };
                    let mut y_offset = self.dynamics_line_y(
                        is_below,
                        wedge_staff,
                        num_staves,
                        Some(wedge_in_gap),
                        part_start_y,
                        staff_dist,
                        measure_extreme_occ,
                        occupancy,
                    );
                    // A hairpin starting at a dynamic continues on that
                    // dynamic's line. Between staves only its own staff's: the
                    // other staff's line lies on the far side of that staff.
                    // If that dynamic sits outside the part (on the line of a
                    // hairpin it ends, say), this hairpin goes there too, for
                    // its whole length.
                    let wedge_between = wedge_in_gap;
                    if wedge.wedge_type == "crescendo" || wedge.wedge_type == "diminuendo" {
                        let staff_top = |st: i32| part_start_y + (st as f32 - 1.0) * staff_dist;
                        for d in self.placed_dynamics.borrow().iter() {
                            if d.3 == is_below
                                && (!wedge_between || d.4 == wedge_staff)
                                && d.0 <= x + 2.0
                                && d.1 >= x - 2.0
                            {
                                y_offset = if is_below {
                                    y_offset.max(d.2)
                                } else {
                                    y_offset.min(d.2)
                                };
                                let beyond_other_staff = if is_below {
                                    d.2 > staff_top(wedge_staff + 1)
                                } else {
                                    d.2 < staff_top(wedge_staff - 1) + 4.0 * self.staff_line_distance
                                };
                                if wedge_in_gap && beyond_other_staff {
                                    wedge_in_gap = false;
                                }
                            }
                        }
                    }

                    if wedge.wedge_type == "crescendo" || wedge.wedge_type == "diminuendo" {
                        let start_spread = if wedge.wedge_type == "crescendo" {
                            0.0
                        } else {
                            wedge.spread.unwrap_or(12.0)
                        };
                        // Start after a dynamic sitting at the hairpin's start.
                        let start_x = self
                            .placed_dynamics
                            .borrow()
                            .iter()
                            .filter(|d| {
                                d.3 == is_below
                                    && (d.2 - y_offset).abs() < 12.0
                                    && d.0 <= x + 2.0
                                    && d.1 >= x - 2.0
                            })
                            .map(|d| d.1 + 4.0)
                            .fold(x, f32::max);
                        // Provisional extent to the end of the measure (see the
                        // per-measure registration of running hairpins).
                        let half = start_spread.max(12.0) / 2.0;
                        self.push_gap_item(
                            wedge_staff,
                            is_below,
                            start_x,
                            self.measure_span.get().1,
                            y_offset - half,
                            y_offset + half,
                        );
                        active_wedges.insert(
                            num,
                            WedgeStartInfo {
                                x: start_x,
                                y: y_offset,
                                wedge_type: wedge.wedge_type.clone(),
                                is_continuation: false,
                                start_spread,
                                is_below,
                                staff: wedge_staff,
                                resumed_line: None,
                                in_gap: wedge_in_gap,
                            },
                        );
                    } else if wedge.wedge_type == "stop" {
                        if let Some(start_info) = active_wedges.remove(&num) {
                            let end_spread = if start_info.wedge_type == "crescendo" {
                                wedge.spread.unwrap_or(12.0)
                            } else {
                                0.0
                            };
                            // Shorten the hairpin so it ends before the first
                            // dynamic on its line between its start and end
                            // (including one sitting right at the end).
                            let end_x = self
                                .placed_dynamics
                                .borrow()
                                .iter()
                                .filter(|d| {
                                    d.3 == is_below
                                        && (d.2 - start_info.y).abs() < 12.0
                                        && d.1 > start_info.x
                                        && d.0 < x + 2.0
                                })
                                .map(|d| d.0 - 4.0)
                                .fold(x, f32::min)
                                .max(start_info.x + 8.0);
                            // Use start_info.y for both start and end to keep it horizontal
                            doc = self.draw_wedge(
                                doc,
                                start_info.x,
                                start_info.y,
                                end_x,
                                start_info.y,
                                &start_info.wedge_type,
                                start_info.start_spread,
                                end_spread,
                                start_info.staff,
                                start_info.is_below,
                            );
                        }
                    }
                    if is_below {
                        local_occ =
                            Some((local_occ.map(|o| o.0).unwrap_or(y_offset), y_offset + 10.0));
                    } else {
                        local_occ =
                            Some((y_offset - 10.0, local_occ.map(|o| o.1).unwrap_or(y_offset)));
                    }
                }
                crate::models::DirectionType::OctaveShift(shift) => {
                    let num = shift.number.unwrap_or(1);
                    let staff_number = dir.staff.unwrap_or(1);
                    let staff_index = staff_number as f32 - 1.0;

                    let is_below = Self::octave_shift_below(
                        dir.placement.as_deref(),
                        &shift.shift_type,
                    );
                    // Between two staves (below an upper staff or above a lower
                    // one) the part-wide occupancy would include the other
                    // staff's notes: start close to the staff and clear only its
                    // own notes when the line is drawn.
                    let between_staves = if is_below {
                        staff_number < num_staves
                    } else {
                        staff_number > 1
                    };

                    let mut y_offset = if between_staves {
                        if is_below {
                            part_start_y + staff_index * staff_dist + 6.5 * self.staff_line_distance
                        } else {
                            part_start_y + staff_index * staff_dist - 2.5 * self.staff_line_distance
                        }
                    } else if is_below {
                        part_start_y + staff_index * staff_dist + 6.0 * self.staff_line_distance
                    } else {
                        part_start_y + staff_index * staff_dist - 4.0 * self.staff_line_distance
                    };

                    // Use BOTH local and measure-wide occupancy for octave shifts
                    for occ in [measure_extreme_occ, local_occ] {
                        let Some((min_y, max_y)) = occ.filter(|_| !between_staves) else {
                            continue;
                        };
                        if is_below {
                            y_offset = y_offset.max(max_y + 10.0);
                        } else {
                            y_offset = y_offset.min(min_y - 15.0);
                        }
                    }

                    if shift.shift_type == "up" || shift.shift_type == "down" {
                        let mut start_info = OctaveShiftStartInfo {
                            x,
                            y: y_offset,
                            size: shift.size,
                            is_continuation: false,
                            shift_type: shift.shift_type.clone(),
                            staff: dir.staff,
                            is_below,
                            ended: false,
                        };
                        // Clear the notes drawn so far after the start now, so
                        // directions placed after this one make room for it.
                        start_info.y = self.octave_shift_clear_y(&start_info, f32::MAX);
                        y_offset = start_info.y;
                        active_octave_shifts.insert(num, start_info);
                    } else if shift.shift_type == "stop" {
                        if let Some(mut start_info) = active_octave_shifts.remove(&num) {
                            // Ensure the final drawing uses the most extreme y discovered in this measure
                            if is_below {
                                start_info.y = start_info.y.max(y_offset);
                            } else {
                                start_info.y = start_info.y.min(y_offset);
                            }
                            start_info.y = self.octave_shift_clear_y(&start_info, x);
                            doc = self.draw_octave_shift(
                                doc,
                                start_info.x,
                                start_info.y,
                                x,
                                start_info.y,
                                start_info.size,
                                &start_info.shift_type,
                                start_info.is_below,
                                start_info.staff.unwrap_or(1),
                            );
                        }
                    }

                    // Widen (never shrink) the occupied band: a line placed
                    // between the staves must not hide what is further out.
                    let (lo, hi) = if is_below {
                        (y_offset, y_offset + 10.0)
                    } else {
                        (y_offset - 15.0, y_offset)
                    };
                    local_occ = Some(match local_occ {
                        Some((a, b)) => (a.min(lo), b.max(hi)),
                        None => (lo, hi),
                    });
                }
                crate::models::DirectionType::Pedal(pedal) => {
                    let num = pedal.number.unwrap_or(1);
                    let is_below = dir.placement.as_deref() != Some("above");
                    let mut y_offset = if is_below {
                        part_start_y
                            + (num_staves as f32 - 1.0) * staff_dist
                            + 10.0 * self.staff_line_distance
                    } else {
                        part_start_y - 4.0 * self.staff_line_distance
                    };
                    if let Some((min_y, max_y)) = local_occ {
                        if is_below {
                            y_offset = y_offset.max(max_y + 30.0);
                        } else {
                            y_offset = y_offset.min(min_y - 30.0);
                        }
                    }

                    if pedal.line {
                        match pedal.pedal_type.as_str() {
                            "start" => {
                                active_pedals.insert(
                                    num,
                                    PedalStartInfo {
                                        x,
                                        y: y_offset,
                                        _line: true,
                                        is_initial: true,
                                        glyph: "\u{E650}".to_string(),
                                        is_continuation: false,
                                        is_below,
                                    },
                                );
                            }
                            "resume" => {
                                active_pedals.insert(
                                    num,
                                    PedalStartInfo {
                                        x,
                                        y: y_offset,
                                        _line: true,
                                        is_initial: true,
                                        glyph: "\u{E651}".to_string(),
                                        is_continuation: false,
                                        is_below,
                                    },
                                );
                            }
                            // Start and end heights are each pushed clear of whatever
                            // is nearby, so they usually differ. Draw every segment
                            // flat at the more extreme of the two (lower when below
                            // the staff, higher when above) so it clears both ends.
                            "change" => {
                                if let Some(start_info) = active_pedals.get_mut(&num) {
                                    let line_y = if is_below {
                                        start_info.y.max(y_offset)
                                    } else {
                                        start_info.y.min(y_offset)
                                    };
                                    let line_y = self.pedal_y_clear_of_wedges(
                                        line_y,
                                        start_info.x,
                                        x,
                                        is_below,
                                        active_wedges,
                                        self.current_part_y.get(),
                                    );
                                    doc = self.draw_pedal_line(
                                        doc,
                                        start_info.x,
                                        line_y,
                                        x,
                                        start_info.is_initial,
                                        true,
                                        false,
                                        &start_info.glyph,
                                        is_below,
                                    );
                                    start_info.x = x + 5.0;
                                    start_info.y = line_y;
                                    start_info.is_initial = false;
                                }
                            }
                            "stop" | "discontinue" => {
                                let is_terminal = pedal.pedal_type == "stop";
                                if let Some(start_info) = active_pedals.remove(&num) {
                                    let line_y = if is_below {
                                        start_info.y.max(y_offset)
                                    } else {
                                        start_info.y.min(y_offset)
                                    };
                                    let line_y = self.pedal_y_clear_of_wedges(
                                        line_y,
                                        start_info.x,
                                        x,
                                        is_below,
                                        active_wedges,
                                        self.current_part_y.get(),
                                    );
                                    doc = self.draw_pedal_line(
                                        doc,
                                        start_info.x,
                                        line_y,
                                        x,
                                        start_info.is_initial,
                                        false,
                                        is_terminal,
                                        &start_info.glyph,
                                        is_below,
                                    );
                                }
                            }
                            _ => {}
                        }
                    } else {
                        if pedal.pedal_type == "start" {
                            doc = doc.add(
                                Text::new("\u{E650}") // pedalDamper
                                    .set("x", x)
                                    .set("y", y_offset)
                                    .set("font-size", self.staff_line_distance * 3.5)
                                    .set("dominant-baseline", "central")
                                    .set("font-family", self.font_family.as_str()),
                            );
                        } else if pedal.pedal_type == "stop" {
                            doc = doc.add(
                                Text::new("\u{E655}") // pedalDamperUp
                                    .set("x", x)
                                    .set("y", y_offset)
                                    .set("font-size", self.staff_line_distance * 3.5)
                                    .set("dominant-baseline", "central")
                                    .set("font-family", self.font_family.as_str()),
                            );
                        }
                    }
                    if is_below {
                        local_occ =
                            Some((local_occ.map(|o| o.0).unwrap_or(y_offset), y_offset + 15.0));
                    } else {
                        local_occ =
                            Some((y_offset - 15.0, local_occ.map(|o| o.1).unwrap_or(y_offset)));
                    }
                }
                crate::models::DirectionType::Dashes(dashes) => {
                    let num = dashes.number.unwrap_or(1);
                    if dashes.dashes_type == "start" {
                        let (new_doc, line_x) = draw_line_label(doc, local_x);
                        doc = new_doc;
                        active_dashes.insert(
                            num,
                            DashesStartInfo {
                                x: line_x,
                                y: shared_y,
                                dash_length: dashes.dash_length,
                                space_length: dashes.space_length,
                                is_continuation: false,
                            },
                        );
                    } else if dashes.dashes_type == "stop" {
                        if let Some(start_info) = active_dashes.remove(&num) {
                            let mut line = Line::new()
                                .set("x1", start_info.x)
                                .set("y1", start_info.y)
                                .set("x2", local_x)
                                .set("y2", start_info.y)
                                .set("stroke", "black")
                                .set("stroke-width", 1);
                            let dash_len = start_info.dash_length.unwrap_or(5.0);
                            let space_len = start_info.space_length.unwrap_or(5.0);
                            line =
                                line.set("stroke-dasharray", format!("{},{}", dash_len, space_len));
                            doc = doc.add(line);
                        }
                    }
                    if is_below {
                        local_occ =
                            Some((local_occ.map(|o| o.0).unwrap_or(shared_y), shared_y + 10.0));
                    } else {
                        local_occ =
                            Some((shared_y - 10.0, local_occ.map(|o| o.1).unwrap_or(shared_y)));
                    }
                }
                crate::models::DirectionType::Damp => {
                    let symbol = "\u{1D1B4}";
                    let text = Text::new(symbol)
                        .set("x", x)
                        .set("y", shared_y)
                        .set("font-size", self.staff_line_distance * 4.0)
                        .set("text-anchor", "middle")
                        .set("dominant-baseline", "central")
                        .set("font-family", self.font_family.as_str());
                    doc = doc.add(text);
                    if is_below {
                        local_occ =
                            Some((local_occ.map(|o| o.0).unwrap_or(shared_y), shared_y + 15.0));
                    } else {
                        local_occ =
                            Some((shared_y - 15.0, local_occ.map(|o| o.1).unwrap_or(shared_y)));
                    }
                }
                crate::models::DirectionType::DampAll => {
                    let symbol = "\u{E639}";
                    let text = Text::new(symbol)
                        .set("x", x)
                        .set("y", shared_y)
                        .set("font-size", self.staff_line_distance * 4.0)
                        .set("text-anchor", "middle")
                        .set("dominant-baseline", "central")
                        .set("font-family", self.font_family.as_str());
                    doc = doc.add(text);
                    if is_below {
                        local_occ =
                            Some((local_occ.map(|o| o.0).unwrap_or(shared_y), shared_y + 15.0));
                    } else {
                        local_occ =
                            Some((shared_y - 15.0, local_occ.map(|o| o.1).unwrap_or(shared_y)));
                    }
                }
                _ => {}
            }
        }
        (doc, local_occ)
    }

    fn draw_wedge(
        &self,
        doc: Document,
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        _wedge_type: &str,
        start_spread: f32,
        end_spread: f32,
        staff: i32,
        is_below: bool,
    ) -> Document {
        let half_start = start_spread / 2.0;
        let half_end = end_spread / 2.0;
        let half = half_start.max(half_end);
        self.push_gap_item(staff, is_below, x1, x2, y1.min(y2) - half, y1.max(y2) + half);
        self.placed_wedges.borrow_mut().push((
            x1.min(x2),
            x1.max(x2),
            y1.max(y2) + half,
            y1.min(y2) - half,
            self.current_part_y.get(),
        ));

        let data = Data::new()
            .move_to((x1, y1 - half_start))
            .line_to((x2, y2 - half_end))
            .move_to((x1, y1 + half_start))
            .line_to((x2, y2 + half_end));

        let path = Path::new()
            .set("fill", "none")
            .set("stroke", "black")
            .set("stroke-width", 1.2)
            .set("d", data);
        doc.add(path)
    }

    /// Octave-shift side: the given placement, else 8va/15ma above and
    /// 8vb/15mb below (`type="up"` marks notes written higher than sounding,
    /// i.e. an ottava bassa).
    fn octave_shift_below(placement: Option<&str>, shift_type: &str) -> bool {
        match placement {
            Some("below") => true,
            Some("above") => false,
            _ => shift_type == "up",
        }
    }

    /// Moves an octave-shift line running to `x2` outward until it clears
    /// the noteheads and stems (beamed ones at their final tips) of its staff
    /// drawn under it, with room for the "8va" text.
    fn octave_shift_clear_y(&self, shift: &OctaveShiftStartInfo, x2: f32) -> f32 {
        const GAP: f32 = 14.0;
        let part_y = self.current_part_y.get();
        let staff = shift.staff.unwrap_or(1);
        let mut y = shift.y;
        for pn in self.placed_notes.borrow().iter() {
            if pn.part_y != part_y || pn.staff != staff || pn.x < shift.x - 6.0 || pn.x > x2 + 6.0
            {
                continue;
            }
            let (top, bottom) = match pn.stem {
                Some((_, tip, _)) => (pn.top.min(tip), pn.bottom.max(tip)),
                None => (pn.top, pn.bottom),
            };
            y = if shift.is_below { y.max(bottom + GAP) } else { y.min(top - GAP) };
        }
        y
    }

    fn draw_octave_shift(
        &self,
        mut doc: Document,
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        size: i32,
        shift_type: &str,
        is_below: bool,
        staff: i32,
    ) -> Document {
        self.push_gap_item(staff, is_below, x1, x2, y1.min(y2) - 8.0, y1.max(y2) + 8.0);
        let is_up = shift_type == "up";
        let text = match size {
            8 => {
                if is_up {
                    "8vb"
                } else {
                    "8va"
                }
            }
            15 => {
                if is_up {
                    "15mb"
                } else {
                    "15ma"
                }
            }
            22 => {
                if is_up {
                    "22mb"
                } else {
                    "22ma"
                }
            }
            _ => "8va",
        };

        // Draw text
        doc = doc.add(
            Text::new(text)
                .set("x", x1)
                .set("y", y1)
                .set("font-size", 12)
                .set("font-style", "italic")
                .set("font-family", "serif")
                .set("dominant-baseline", "central"),
        );

        // Draw dashed line
        let tick_len = 8.0;
        let mut data = Data::new().move_to((x1 + 25.0, y1)).line_to((x2, y2));
        // End tick: if below, tick goes UP; if above, tick goes DOWN to point to the staff
        let tick_y = if is_below {
            y2 - tick_len
        } else {
            y2 + tick_len
        };
        data = data.line_to((x2, tick_y));

        let path = Path::new()
            .set("fill", "none")
            .set("stroke", "black")
            .set("stroke-width", 1.0)
            .set("stroke-dasharray", "5,5")
            .set("d", data);

        doc.add(path)
    }
    /// Draws a bracket line (with its label) from its start to `x2`, moved
    /// out from where it started until it clears the notes (stems and beams
    /// included) and slurs of its staff all along the way.
    fn draw_bracket_span(
        &self,
        mut doc: Document,
        b: &BracketStartInfo,
        x2: f32,
        stop_end: &Option<String>,
    ) -> Document {
        const CLEARANCE: f32 = 12.0;
        let part_y = self.current_part_y.get();
        let x1 = b.label.as_ref().map_or(b.x, |(_, lx)| *lx);
        let within = |x: f32| x >= x1 - 4.0 && x <= x2 + 4.0;
        let mut y = b.y;
        let mut clear = |edge: f32| {
            y = if b.is_below { y.max(edge + CLEARANCE) } else { y.min(edge - CLEARANCE) };
        };
        for pn in self.placed_notes.borrow().iter() {
            if pn.part_y != part_y || pn.staff != b.staff || !within(pn.x) {
                continue;
            }
            let (top, bottom) = self.note_extent_own_staff(pn);
            clear(if b.is_below { bottom } else { top });
        }
        for sl in self.placed_slurs.borrow().iter() {
            if sl.cross_staff || sl.staff != b.staff || (sl.part_y - part_y).abs() >= 0.5 {
                continue;
            }
            let (a, c) = sl.x_range();
            let (lo, hi) = (a.max(x1), c.min(x2));
            if lo < hi {
                for i in 0..=8 {
                    clear(sl.y_at(lo + (hi - lo) * i as f32 / 8.0));
                }
            }
        }
        // Fingerings over the notes (about 6px either side of their centre).
        for &(f_part_y, fx, fy) in self.placed_fingerings.borrow().iter() {
            if (f_part_y - part_y).abs() < 0.5 && within(fx) {
                clear(if b.is_below { fy + 6.0 } else { fy - 6.0 });
            }
        }
        if let Some((text, lx)) = &b.label {
            doc = doc.add(
                self.words_text(text)
                    .set("x", *lx)
                    .set("y", y)
                    .set("font-size", 12)
                    .set("text-anchor", "start")
                    .set("dominant-baseline", "central")
                    .set("font-family", "serif")
                    .set("font-style", "italic"),
            );
        }
        self.note_drawn_extent(y - 8.0, y + 8.0);
        self.draw_horizontal_bracket(doc, b.x, y, x2, y, &b.line_type, &b.line_end, stop_end)
    }

    fn draw_horizontal_bracket(
        &self,
        doc: Document,
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        line_type: &Option<String>,
        start_end: &Option<String>,
        stop_end: &Option<String>,
    ) -> Document {
        let mut data = Data::new().move_to((x1, y1)).line_to((x2, y2));

        // Start tick
        if let Some(end) = start_end {
            let tick_y = if end == "down" {
                y1 + 10.0
            } else if end == "up" {
                y1 - 10.0
            } else {
                y1
            };
            data = data
                .move_to((x1, tick_y))
                .line_to((x1, y1))
                .move_to((x1, y1))
                .line_to((x2, y2));
        }
        // Stop tick
        if let Some(end) = stop_end {
            let tick_y = if end == "down" {
                y2 + 10.0
            } else if end == "up" {
                y2 - 10.0
            } else {
                y2
            };
            data = data.line_to((x2, y2)).line_to((x2, tick_y));
        }

        let mut path = Path::new()
            .set("fill", "none")
            .set("stroke", "black")
            .set("stroke-width", 1.0)
            .set("d", data);
        match line_type.as_deref() {
            Some("dashed") => path = path.set("stroke-dasharray", "5,5"),
            Some("dotted") => path = path.set("stroke-dasharray", "1,3"),
            _ => {}
        }
        doc.add(path)
    }

    /// Queues a (flat) pedal line segment; it is drawn by
    /// `flush_deferred_pedals` at the end of the system.
    fn draw_pedal_line(
        &self,
        doc: Document,
        x1: f32,
        y: f32,
        x2: f32,
        is_initial: bool,
        is_change: bool,
        is_terminal: bool,
        glyph: &str,
        is_below: bool,
    ) -> Document {
        self.deferred_pedals.borrow_mut().push(DeferredPedal {
            x1,
            x2,
            y,
            is_initial,
            is_change,
            is_terminal,
            glyph: glyph.to_string(),
            is_below,
            part_y: self.current_part_y.get(),
        });
        doc
    }

    /// Draws the system's queued pedal lines, each moved clear of every
    /// hairpin drawn under it (hairpins can still move after a pedal is
    /// placed, e.g. to clear a slur drawn in a later measure).
    fn flush_deferred_pedals(&self, mut doc: Document) -> Document {
        let pedals: Vec<DeferredPedal> = self.deferred_pedals.borrow_mut().drain(..).collect();
        let no_active = HashMap::new();
        for p in pedals {
            let y = self.pedal_y_clear_of_wedges(p.y, p.x1, p.x2, p.is_below, &no_active, p.part_y);
            doc = self.render_pedal_line(
                doc,
                p.x1,
                y,
                p.x2,
                y,
                p.is_initial,
                p.is_change,
                p.is_terminal,
                &p.glyph,
            );
        }
        doc
    }

    fn render_pedal_line(
        &self,
        mut doc: Document,
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        is_initial: bool,
        is_change: bool,
        is_terminal: bool,
        glyph: &str,
    ) -> Document {
        // Draw the pedal glyph at the start
        if is_initial {
            doc = doc.add(
                Text::new(glyph)
                    .set("x", x1)
                    .set("y", y1)
                    .set("font-size", self.staff_line_distance * 3.0)
                    .set("dominant-baseline", "central")
                    .set("font-family", self.font_family.as_str()),
            );
        }
        // Glyph (~1 staff space either side) and the 8px end tick / notch.
        self.note_drawn_extent(y1.min(y2) - 10.0, y1.max(y2) + 10.0);
        let line_x1 = if is_initial { x1 + 25.0 } else { x1 };
        let line_x2 = if is_change { x2 - 5.0 } else { x2 };
        let mut data = Data::new().move_to((line_x1, y1)).line_to((line_x2, y2));

        if is_change {
            // V-notch for pedal change
            data = data
                .move_to((x2 - 5.0, y2))
                .line_to((x2, y2 - 8.0))
                .line_to((x2 + 5.0, y2));
        } else if is_terminal {
            // End tick for stop
            data = data.move_to((x2, y2)).line_to((x2, y2 - 8.0));
        }

        let path = Path::new()
            .set("fill", "none")
            .set("stroke", "black")
            .set("stroke-width", 1.0)
            .set("d", data);
        doc.add(path)
    }

    fn draw_volta_bracket(
        &self,
        doc: Document,
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        text: &str,
        has_start_tick: bool,
        has_stop_tick: bool,
    ) -> Document {
        let tick_len = 12.0;
        let mut data = Data::new().move_to((x1, y1));
        if has_start_tick {
            data = data.move_to((x1, y1 + tick_len)).line_to((x1, y1));
        }
        data = data.line_to((x2, y2));
        if has_stop_tick {
            data = data.line_to((x2, y2 + tick_len));
        }

        let path = Path::new()
            .set("fill", "none")
            .set("stroke", "black")
            .set("stroke-width", 1.2)
            .set("d", data);
        if !text.is_empty() {
            let txt = Text::new(text)
                .set("x", x1 + 5.0)
                .set("y", y1 + 10.0)
                .set("font-size", 11)
                .set("font-family", "serif")
                .set("font-weight", "bold");
            doc.add(path).add(txt)
        } else {
            doc.add(path)
        }
    }

    fn draw_bookmark(&self, _doc: Document, _id: &str, _x: f32, _y: f32) -> Document {
        // Bookmark is non-visual, so we skip rendering it.
        _doc
    }

    fn draw_figured_bass(
        &self,
        mut doc: Document,
        fb: &crate::models::FiguredBass,
        x: f32,
        y: f32,
        active_extensions: &mut HashMap<usize, FiguredBassStartInfo>,
    ) -> Document {
        let mut cur_y = if let Some(dy) = fb.default_y {
            // MusicXML default-y is in tenths, positive up.
            // Here we assume it's relative to part_start_y (top of staff).
            y - dy * (self.staff_line_distance / 10.0) // Very rough mapping
        } else {
            y
        };

        let accidental_to_smufl = |acc: &str| -> &str {
            match acc {
                "flat" => "\u{E260}",
                "sharp" => "\u{E262}",
                "natural" => "\u{E261}",
                "double-flat" => "\u{E264}",
                "double-sharp" => "\u{E263}",
                _ => "",
            }
        };

        for (i, figure) in fb.figures.iter().enumerate() {
            let mut x_offset = 0.0;

            // Render prefix accidental
            if let Some(prefix) = &figure.prefix {
                let sym = accidental_to_smufl(prefix);
                if !sym.is_empty() {
                    doc = doc.add(
                        Text::new(sym)
                            .set("x", x - 10.0)
                            .set("y", cur_y)
                            .set("font-size", self.staff_line_distance * 1.5)
                            .set("text-anchor", "middle")
                            .set("dominant-baseline", "central")
                            .set("font-family", self.font_family.as_str()),
                    );
                }
            }

            // Render figure number
            if let Some(num) = &figure.number {
                doc = doc.add(
                    Text::new(num)
                        .set("x", x)
                        .set("y", cur_y)
                        .set("font-size", 12)
                        .set("text-anchor", "middle")
                        .set("dominant-baseline", "central")
                        .set("font-family", "serif")
                        .set("font-weight", "bold"),
                );
                x_offset = 10.0;
            }

            // Render suffix accidental
            if let Some(suffix) = &figure.suffix {
                let sym = accidental_to_smufl(suffix);
                if !sym.is_empty() {
                    doc = doc.add(
                        Text::new(sym)
                            .set("x", x + x_offset - self.staff_line_distance * 0.4)
                            .set("y", cur_y - self.staff_line_distance * 0.5)
                            .set("font-size", self.staff_line_distance * 1.5)
                            .set("text-anchor", "middle")
                            .set("dominant-baseline", "central")
                            .set("font-family", self.font_family.as_str()),
                    );
                }
            }

            if let Some(ext) = &figure.extend {
                if ext == "start" {
                    active_extensions.insert(
                        i,
                        FiguredBassStartInfo {
                            x: x + 8.0,
                            y: cur_y,
                        },
                    );
                } else if ext == "stop" {
                    if let Some(start) = active_extensions.remove(&i) {
                        let line = Line::new()
                            .set("x1", start.x)
                            .set("y1", start.y)
                            .set("x2", x - 8.0)
                            .set("y2", start.y)
                            .set("stroke", "black")
                            .set("stroke-width", 1);
                        doc = doc.add(line);
                    }
                }
            }
            cur_y += 15.0; // Stack vertically
        }
        doc
    }

    fn draw_metronome_mark(
        &self,
        doc: Document,
        met: &crate::models::MetronomeMark,
        x: f32,
        part_start_y: f32,
        _num_staves: i32,
        occupancy: Option<(f32, f32)>,
        _staff_dist: f32,
        is_below: bool,
    ) -> (Document, f32, f32) {
        let mut y_offset = if is_below {
            part_start_y + 8.0 * self.staff_line_distance
        } else {
            part_start_y - 4.5 * self.staff_line_distance
        };

        if let Some((min_y, max_y)) = occupancy {
            if is_below {
                y_offset = y_offset.max(max_y + 20.0);
            } else {
                y_offset = y_offset.min(min_y - 20.0);
            }
        }

        // Measure the mark's width with a throwaway draw, then keep it clear
        // of direction texts already placed nearby.
        const MET_UP: f32 = 24.0;
        const MET_DOWN: f32 = 9.0;
        let (_, end_x) = self.draw_metronome_body(Document::new(), met, x, y_offset);
        y_offset = self.clear_of_text_boxes(x, end_x, MET_UP, MET_DOWN, y_offset, is_below);
        self.placed_text_boxes.borrow_mut().push((
            x,
            end_x,
            y_offset - MET_UP,
            y_offset + MET_DOWN,
            is_below,
        ));
        let (doc, _) = self.draw_metronome_body(doc, met, x, y_offset);
        let min_y = y_offset - 20.0;
        let max_y = y_offset + 20.0;
        (doc, min_y, max_y)
    }

    /// Moves an element spanning `x1..x2` (reaching `up` above and `down`
    /// below its y) outward until it clears the notes of the current part
    /// already drawn there (noteheads and stems).
    fn clear_of_placed_notes(
        &self,
        x1: f32,
        x2: f32,
        up: f32,
        down: f32,
        mut y: f32,
        is_below: bool,
        only_staff: Option<i32>,
    ) -> f32 {
        const GAP: f32 = 3.0;
        let part_y = self.current_part_y.get();
        let notes = self.placed_notes.borrow();
        for _ in 0..16 {
            let mut moved = false;
            for pn in notes
                .iter()
                .filter(|pn| pn.part_y == part_y && only_staff.is_none_or(|st| pn.staff == st))
            {
                // Notehead box, then the stem as a thin box.
                let mut parts = vec![(pn.x - 6.0, pn.x + 6.0, pn.top, pn.bottom)];
                if let Some((sx, tip, _)) = pn.stem {
                    parts.push((sx - 1.0, sx + 1.0, tip.min(pn.top), tip.max(pn.bottom)));
                }
                for (bx1, bx2, top, bottom) in parts {
                    if x1 < bx2 && x2 > bx1 && y - up < bottom + GAP && y + down > top - GAP {
                        y = if is_below { bottom + GAP + up } else { top - GAP - down };
                        moved = true;
                    }
                }
            }
            if !moved {
                break;
            }
        }
        y
    }

    /// Moves an element of `staff` spanning `x1..x2` (reaching `up` above
    /// and `down` below its y) outward until it clears that staff's
    /// directions already placed on the same side.
    fn clear_of_gap_items(
        &self,
        staff: i32,
        x1: f32,
        x2: f32,
        up: f32,
        down: f32,
        mut y: f32,
        is_below: bool,
    ) -> f32 {
        const GAP: f32 = 3.0;
        let part_y = self.current_part_y.get();
        let items = self.gap_items.borrow();
        for _ in 0..16 {
            let mut moved = false;
            for it in items.iter().filter(|it| {
                it.part_y == part_y && it.staff == staff && it.is_below == is_below
            }) {
                if x1 < it.x2 && x2 > it.x1 && y - up < it.bottom + GAP && y + down > it.top - GAP {
                    y = if is_below { it.bottom + GAP + up } else { it.top - GAP - down };
                    moved = true;
                }
            }
            if !moved {
                break;
            }
        }
        y
    }

    /// Moves a direction text spanning `x1..x2` (reaching `up` above and
    /// `down` below its y) outward from the staff until it no longer
    /// overlaps any direction text already placed on the same side.
    fn clear_of_text_boxes(
        &self,
        x1: f32,
        x2: f32,
        up: f32,
        down: f32,
        mut y: f32,
        is_below: bool,
    ) -> f32 {
        const GAP: f32 = 3.0;
        let boxes = self.placed_text_boxes.borrow();
        for _ in 0..32 {
            let hit = boxes.iter().find(|b| {
                b.4 == is_below
                    && x1 < b.1 + GAP
                    && x2 > b.0 - GAP
                    && y - up < b.3 + GAP
                    && y + down > b.2 - GAP
            });
            match hit {
                Some(b) if is_below => y = b.3 + GAP + up,
                Some(b) => y = b.2 - GAP - down,
                None => break,
            }
        }
        y
    }

    /// Draws a metronome mark's content starting at `x` on line `y_offset`;
    /// returns the document and the x where the mark ends.
    fn draw_metronome_body(
        &self,
        mut doc: Document,
        met: &crate::models::MetronomeMark,
        x: f32,
        y_offset: f32,
    ) -> (Document, f32) {
        let mut current_x = x;

        if met.parentheses {
            let left_paren = Text::new("(")
                .set("x", current_x)
                .set("y", y_offset)
                .set("font-size", 16)
                .set("dominant-baseline", "central")
                .set("font-family", "serif");
            doc = doc.add(left_paren);
            current_x += 8.0;
        }

        if !met.metronome_notes.is_empty() {
            let mut active_beams: HashMap<i32, f32> = HashMap::new(); // number -> start_x
            let mut tuplet_start_x: Option<f32> = None;

            for (idx, mn) in met.metronome_notes.iter().enumerate() {
                if idx > 0 && idx == met.metronome_notes.len() / 2 && met.relation.is_some() {
                    let rel_text = match met.relation.as_deref() {
                        Some("equals") => " = ",
                        _ => " = ",
                    };
                    let text = Text::new(rel_text)
                        .set("x", current_x)
                        .set("y", y_offset)
                        .set("font-size", 16)
                        .set("dominant-baseline", "central")
                        .set("font-family", "serif");
                    doc = doc.add(text);
                    current_x += 25.0;
                }

                let note_start_x = current_x;
                self.render_metronome_note(&mut doc, mn, &mut current_x, y_offset);

                // Beams
                for beam in &mn.beams {
                    if beam.value == crate::models::BeamValue::Begin {
                        active_beams.insert(beam.number, note_start_x);
                    } else if beam.value == crate::models::BeamValue::End {
                        if let Some(bx1) = active_beams.remove(&beam.number) {
                            let bx2 = note_start_x;
                            // Fine-tuned for E1D5 stem tip alignment (Right and Up)
                            let by = y_offset - 15.5;
                            let beam_line = Line::new()
                                .set("x1", bx1 + 6.5)
                                .set("y1", by)
                                .set("x2", bx2 + 6.5)
                                .set("y2", by)
                                .set("stroke", "black")
                                .set("stroke-width", 2.5);
                            doc = doc.add(beam_line);
                        }
                    }
                }

                // Tuplets
                if let Some(tuplet) = &mn.tuplet {
                    if tuplet.tuplet_type == "start" {
                        tuplet_start_x = Some(note_start_x);
                    } else if tuplet.tuplet_type == "stop" {
                        if let Some(tx1) = tuplet_start_x.take() {
                            let tx2 = note_start_x + 10.0;
                            // Raise tuplet bracket further up for clarity
                            let ty = y_offset - 25.0;
                            let bracket_data = Data::new()
                                .move_to((tx1, ty + 3.0))
                                .line_to((tx1, ty))
                                .line_to((tx2, ty))
                                .line_to((tx2, ty + 3.0));
                            let bracket = Path::new()
                                .set("fill", "none")
                                .set("stroke", "black")
                                .set("stroke-width", 1)
                                .set("d", bracket_data);
                            let num_text = Text::new("3")
                                .set("x", (tx1 + tx2) / 2.0)
                                .set("y", ty - 5.0)
                                .set("font-size", 9)
                                .set("text-anchor", "middle")
                                .set("dominant-baseline", "central")
                                .set("font-family", "serif")
                                .set("font-weight", "bold");
                            doc = doc.add(bracket).add(num_text);
                        }
                    }
                }

                current_x += 5.0;
            }
        } else {
            self.render_metronome_unit(&mut doc, met, &mut current_x, y_offset);

            if let Some(bpm) = &met.bpm {
                let text = Text::new(format!(" = {}", bpm))
                    .set("x", current_x)
                    .set("y", y_offset)
                    .set("font-size", 16)
                    .set("dominant-baseline", "central")
                    .set("font-family", "serif")
                    .set("font-weight", "bold");
                doc = doc.add(text);
                current_x += bpm.len() as f32 * 8.0 + 20.0;
            } else if let Some(to_unit) = &met.to_beat_unit {
                let text = Text::new(" = ")
                    .set("x", current_x)
                    .set("y", y_offset)
                    .set("font-size", 16)
                    .set("dominant-baseline", "central")
                    .set("font-family", "serif");
                doc = doc.add(text);
                current_x += 25.0;

                let mut to_m = crate::models::MetronomeMark::default();
                to_m.beat_unit = to_unit.clone();
                to_m.beat_unit_dot = met.to_beat_unit_dot;
                self.render_metronome_unit(&mut doc, &to_m, &mut current_x, y_offset);
            }
        }

        if met.parentheses {
            let right_paren = Text::new(")")
                .set("x", current_x)
                .set("y", y_offset)
                .set("font-size", 16)
                .set("dominant-baseline", "central")
                .set("font-family", "serif");
            doc = doc.add(right_paren);
            current_x += 8.0;
        }

        (doc, current_x)
    }

    fn render_metronome_note(
        &self,
        doc: &mut Document,
        mn: &crate::models::MetronomeNote,
        x: &mut f32,
        y: f32,
    ) {
        let has_beams = !mn.beams.is_empty();
        let note_sym = if has_beams {
            "\u{E1D5}"
        } else {
            self.beat_unit_to_smufl(&mn.beat_unit)
        };

        let text = Text::new(note_sym)
            .set("x", *x)
            .set("y", y)
            .set("font-size", 20)
            .set("dominant-baseline", "central")
            .set("font-family", self.font_family.as_str());
        *doc = doc.clone().add(text);

        // Draw dots
        let mut dot_x = *x + 15.0;
        for _ in 0..mn.dots {
            let dot = svg::node::element::Circle::new()
                .set("cx", dot_x)
                .set("cy", y + 2.0)
                .set("r", 1.5)
                .set("fill", "black");
            *doc = doc.clone().add(dot);
            dot_x += 6.0;
        }

        *x += 15.0 + mn.dots as f32 * 6.0;
    }

    fn render_metronome_unit(
        &self,
        doc: &mut Document,
        met: &crate::models::MetronomeMark,
        x: &mut f32,
        y: f32,
    ) {
        // Draw note
        let note_sym = self.beat_unit_to_smufl(&met.beat_unit);
        let text = Text::new(note_sym)
            .set("x", *x)
            .set("y", y)
            .set("font-size", 20) // Slightly larger for clarity
            .set("dominant-baseline", "central")
            .set("font-family", self.font_family.as_str());
        *doc = doc.clone().add(text);
        *x += 15.0;

        // Draw dots
        for _ in 0..met.beat_unit_dot {
            let dot = svg::node::element::Circle::new()
                .set("cx", *x)
                .set("cy", y + 2.0)
                .set("r", 1.5)
                .set("fill", "black");
            *doc = doc.clone().add(dot);
            *x += 6.0;
        }

        // Draw tie and next unit recursively
        if let Some(tied) = &met.tied_unit {
            let tie_x1 = *x - 5.0;
            let tie_x2 = *x + 5.0;
            let tie_y = y + 5.0;

            let data = Data::new().move_to((tie_x1, tie_y)).quadratic_curve_to((
                (tie_x1 + tie_x2) / 2.0,
                tie_y + 4.0,
                tie_x2,
                tie_y,
            ));
            let path = Path::new()
                .set("fill", "none")
                .set("stroke", "black")
                .set("stroke-width", 1.2)
                .set("d", data);
            *doc = doc.clone().add(path);
            *x += 10.0;

            self.render_metronome_unit(doc, tied, x, y);
        }
    }

    fn beat_unit_to_smufl(&self, unit: &str) -> &'static str {
        match unit {
            "breve" => "\u{E1D1}",
            "whole" => "\u{E1D2}",
            "half" => "\u{E1D3}",
            "quarter" => "\u{E1D5}",
            "eighth" => "\u{E1D7}",
            "16th" => "\u{E1D9}",
            "32nd" => "\u{E1DB}",
            "64th" => "\u{E1DD}",
            _ => "\u{E1D5}",
        }
    }

    fn dynamic_to_smufl(&self, d: &str) -> String {
        match d {
            "p" => "\u{E520}".to_string(),
            "m" => "\u{E521}".to_string(),
            "f" => "\u{E522}".to_string(),
            "r" => "\u{E523}".to_string(),
            "s" => "\u{E524}".to_string(),
            "z" => "\u{E525}".to_string(),
            "n" => "\u{E526}".to_string(),
            "pppppp" => "\u{E527}".to_string(),
            "ppppp" => "\u{E528}".to_string(),
            "pppp" => "\u{E529}".to_string(),
            "ppp" => "\u{E52A}".to_string(),
            "pp" => "\u{E52B}".to_string(),
            "mp" => "\u{E52C}".to_string(),
            "mf" => "\u{E52D}".to_string(),
            "pf" => "\u{E52E}".to_string(),
            "ff" => "\u{E52F}".to_string(),
            "fff" => "\u{E530}".to_string(),
            "ffff" => "\u{E531}".to_string(),
            "fffff" => "\u{E532}".to_string(),
            "ffffff" => "\u{E533}".to_string(),
            "fp" => "\u{E534}".to_string(),
            "fz" => "\u{E535}".to_string(),
            "sf" => "\u{E536}".to_string(),
            "sfp" => "\u{E537}".to_string(),
            "sfpp" => "\u{E538}".to_string(),
            "sfz" => "\u{E539}".to_string(),
            "sfzp" => "\u{E53A}".to_string(),
            "sffz" => "\u{E53B}".to_string(),
            "rf" => "\u{E53C}".to_string(),
            "rfz" => "\u{E53D}".to_string(),
            _ => d.to_string(),
        }
    }

    fn estimate_dynamics_width(&self, dyns: &[String]) -> f32 {
        let combined: String = dyns.join("");
        let len = combined.len();
        let scale = self.staff_line_distance / 10.0;
        let char_w = 14.0 * scale;
        (len as f32 * char_w).max(14.0)
    }

    fn draw_part_name(
        &self,
        doc: Document,
        name: &str,
        start_y: f32,
        end_y: f32,
        margin_left: f32,
        is_first: bool,
    ) -> Document {
        let x = margin_left - 16.0;
        let midpoint = (start_y + end_y) / 2.0;
        let font_size = if is_first { 16 } else { 12 };
        let text = Text::new(name)
            .set("x", x)
            .set("y", midpoint)
            .set("font-size", font_size)
            .set("text-anchor", "end")
            .set("dominant-baseline", "central")
            .set("font-family", "serif");
        doc.add(text)
    }

    fn draw_group_symbol(
        &self,
        mut doc: Document,
        group: &PartGroup,
        start_y: f32,
        end_y: f32,
        margin_left: f32,
    ) -> Document {
        let x = margin_left - 4.0;
        let height = end_y - start_y;
        if height <= 0.0 {
            return doc;
        }
        match group.symbol {
            Some(GroupSymbol::Brace) => {
                let base_font_size = 40.0;
                let scale_y = height / base_font_size;
                let text = Text::new("\u{E000}")
                    .set("x", x)
                    .set("y", end_y)
                    .set("font-size", base_font_size)
                    .set("text-anchor", "end")
                    .set("dominant-baseline", "alphabetic")
                    .set("font-family", self.font_family.as_str())
                    // Stretch about (x, end_y) in the transform itself (see
                    // draw_arpeggio: not every renderer honours transform-origin).
                    .set(
                        "transform",
                        format!(
                            "translate({}, {}) scale(1.5, {}) translate({}, {})",
                            x, end_y, scale_y, -x, -end_y
                        ),
                    );
                doc = doc.add(text);
            }
            Some(GroupSymbol::Bracket) => {
                let font_size = 40.0;
                let protrusion = 4.0;
                let top_text = Text::new("\u{E003}")
                    .set("x", x)
                    .set("y", start_y - protrusion)
                    .set("font-size", font_size)
                    .set("text-anchor", "start")
                    .set("dominant-baseline", "alphabetic")
                    .set("font-family", self.font_family.as_str());
                let bottom_text = Text::new("\u{E004}")
                    .set("x", x)
                    .set("y", end_y + protrusion)
                    .set("font-size", font_size)
                    .set("text-anchor", "start")
                    .set("dominant-baseline", "alphabetic")
                    .set("font-family", self.font_family.as_str());
                let bar_data = Data::new()
                    .move_to((x, start_y - protrusion))
                    .line_to((x, end_y + protrusion));
                let bar = Path::new()
                    .set("fill", "none")
                    .set("stroke", "black")
                    .set("stroke-width", 2.5)
                    .set("d", bar_data);
                doc = doc.add(top_text).add(bottom_text).add(bar);
            }
            _ => {}
        }
        doc
    }

    fn draw_group_name(
        &self,
        doc: Document,
        name: &str,
        start_y: f32,
        end_y: f32,
        margin_left: f32,
        is_first: bool,
    ) -> Document {
        let mid_y = (start_y + end_y) / 2.0;
        let font_size = if is_first { 14 } else { 10 };
        let display_name = if name.chars().count() > 14 {
            let mut truncated: String = name.chars().take(12).collect();
            truncated.push_str("...");
            truncated
        } else {
            name.to_string()
        };
        let text = Text::new(display_name)
            .set("x", margin_left - 18.0)
            .set("y", mid_y)
            .set("font-size", font_size)
            .set("text-anchor", "end")
            .set("dominant-baseline", "central")
            .set("font-family", "serif")
            .set("font-style", "italic");
        doc.add(text)
    }

    fn draw_frame(
        &self,
        mut doc: Document,
        frame: &crate::models::Frame,
        y: f32,
        x: f32,
    ) -> Document {
        let string_spacing = 8.0;
        let fret_spacing = 10.0;
        let frame_width = (frame.strings - 1) as f32 * string_spacing;
        let frame_height = frame.frets as f32 * fret_spacing;
        let start_x = x - frame_width * 0.5;

        // Draw vertical lines (strings)
        for i in 0..frame.strings {
            let sx = start_x + i as f32 * string_spacing;
            doc = doc.add(
                Line::new()
                    .set("x1", sx)
                    .set("y1", y)
                    .set("x2", sx)
                    .set("y2", y + frame_height)
                    .set("stroke", "black")
                    .set("stroke-width", 1),
            );
        }

        // Draw horizontal lines (frets)
        for i in 0..=frame.frets {
            let fy = y + i as f32 * fret_spacing;
            let stroke_w = if i == 0 && frame.first_fret.unwrap_or(1) == 1 {
                3.0
            } else {
                1.0
            };
            doc = doc.add(
                Line::new()
                    .set("x1", start_x)
                    .set("y1", fy)
                    .set("x2", start_x + frame_width)
                    .set("y2", fy)
                    .set("stroke", "black")
                    .set("stroke-width", stroke_w),
            );
        }

        // Fret number
        if let Some(first) = frame.first_fret {
            if first > 1 {
                let fret_text = frame
                    .first_fret_text
                    .as_deref()
                    .unwrap_or(&first.to_string())
                    .to_string();
                doc = doc.add(
                    Text::new(fret_text)
                        .set("x", start_x + frame_width + 5.0)
                        .set("y", y + fret_spacing * 0.5)
                        .set("font-size", 10)
                        .set("dominant-baseline", "central")
                        .set("font-family", "serif"),
                );
            }
        }

        // Track played strings to mark unplayed ones
        let mut played_strings = HashSet::new();
        let mut barre_starts: HashMap<i32, f32> = HashMap::new();

        for note in &frame.notes {
            played_strings.insert(note.string);
            let nx = start_x + (frame.strings - note.string) as f32 * string_spacing;

            if note.fret > 0 {
                let relative_fret = if let Some(first) = frame.first_fret {
                    (note.fret - first).max(0)
                } else {
                    note.fret - 1
                };
                let ny = y + relative_fret as f32 * fret_spacing + fret_spacing * 0.5;
                doc = doc.add(
                    svg::node::element::Circle::new()
                        .set("cx", nx)
                        .set("cy", ny)
                        .set("r", 3.5)
                        .set("fill", "black"),
                );

                // Barre
                if let Some(barre) = &note.barre {
                    if barre == "start" {
                        barre_starts.insert(note.fret, nx);
                    } else if barre == "stop" {
                        if let Some(bx1) = barre_starts.remove(&note.fret) {
                            let bx2 = nx;
                            let data = Data::new().move_to((bx1, ny - 2.0)).cubic_curve_to((
                                bx1,
                                ny - 6.0,
                                bx2,
                                ny - 6.0,
                                bx2,
                                ny - 2.0,
                            ));
                            doc = doc.add(
                                Path::new()
                                    .set("fill", "none")
                                    .set("stroke", "black")
                                    .set("stroke-width", 1.5)
                                    .set("d", data),
                            );
                        }
                    }
                }
            } else {
                // Open string 'o'
                doc = doc.add(
                    Text::new("o")
                        .set("x", nx)
                        .set("y", y - 8.0)
                        .set("font-size", 10)
                        .set("text-anchor", "middle")
                        .set("dominant-baseline", "central")
                        .set("font-family", "serif"),
                );
            }

            // Fingering BELOW the frame
            if let Some(fingering) = &note.fingering {
                let fy = y + frame_height + 10.0;
                doc = doc.add(
                    Text::new(fingering.as_str())
                        .set("x", nx)
                        .set("y", fy)
                        .set("font-size", 10)
                        .set("text-anchor", "middle")
                        .set("dominant-baseline", "central")
                        .set("font-family", "serif"),
                );
            }
        }

        // Unplayed strings 'x'
        for s in 1..=frame.strings {
            if !played_strings.contains(&s) {
                let nx = start_x + (frame.strings - s) as f32 * string_spacing;
                doc = doc.add(
                    Text::new("x")
                        .set("x", nx)
                        .set("y", y - 8.0)
                        .set("font-size", 10)
                        .set("text-anchor", "middle")
                        .set("dominant-baseline", "central")
                        .set("font-family", "serif"),
                );
            }
        }

        doc
    }

    /// For each multi-staff part, how much wider each staff gap must be so
    /// that slurs hanging below a staff don't collide with slurs rising above
    /// the next one (slurs whose endpoints span both staves are ignored).
    fn inter_staff_slur_overlaps(
        &self,
        score: &Score,
        part_states: &HashMap<String, PartState>,
        part_y_offsets: &HashMap<String, f32>,
        part_staff_distances: &HashMap<String, f32>,
        system_render_y: f32,
    ) -> Vec<(String, f32)> {
        const SLUR_GAP: f32 = 6.0;
        let slurs = self.placed_slurs.borrow();
        let slurs: Vec<&PlacedSlur> = slurs.iter().filter(|s| !s.cross_staff).collect();
        let mut result = Vec::new();
        for part in &score.parts {
            let id = &part.id;
            let Some(state) = part_states.get(id) else { continue };
            if state.num_staves < 2 {
                continue;
            }
            let (Some(&offset), Some(&staff_dist)) =
                (part_y_offsets.get(id), part_staff_distances.get(id))
            else {
                continue;
            };
            let part_start_y = system_render_y + offset;
            let mut extra: f32 = 0.0;
            for s in 1..state.num_staves {
                let upper_bottom =
                    part_start_y + (s as f32 - 1.0) * staff_dist + 4.0 * self.staff_line_distance;
                let lower_top = part_start_y + s as f32 * staff_dist;
                let upper_top = part_start_y + (s as f32 - 1.0) * staff_dist;
                let lower_bottom = lower_top + 4.0 * self.staff_line_distance;
                let owned = |sl: &PlacedSlur, staff: i32| {
                    sl.staff == staff && (sl.part_y - part_start_y).abs() < 0.5
                };
                for a in slurs.iter() {
                    // Hangs below the upper staff. It must stay out of the
                    // lower staff itself, not only clear that staff's slurs.
                    if !(owned(a, s) && a.bottom > upper_bottom) {
                        continue;
                    }
                    extra = extra.max(a.bottom + SLUR_GAP - lower_top);
                    for b in slurs.iter() {
                        // Rises above the lower staff.
                        if !(owned(b, s + 1) && b.top < lower_top) {
                            continue;
                        }
                        // Cheap reject on bounding boxes, then compare the
                        // actual curves where both exist.
                        let (a1, a2) = a.x_range();
                        let (b1, b2) = b.x_range();
                        let (lo, hi) = (a1.max(b1), a2.min(b2));
                        if lo >= hi || a.bottom + SLUR_GAP <= b.top {
                            continue;
                        }
                        for i in 0..=24 {
                            let x = lo + (hi - lo) * i as f32 / 24.0;
                            let overlap = a.y_at(x) + SLUR_GAP - b.y_at(x);
                            extra = extra.max(overlap);
                        }
                    }
                }
                for b in slurs.iter().filter(|b| owned(b, s + 1) && b.top < lower_top) {
                    extra = extra.max(upper_bottom + SLUR_GAP - b.top);
                }
                // Directions placed in the gap (hairpins, dynamics, words,
                // octave lines) must clear the other staff, its notes and its
                // own directions in the gap.
                {
                    const DIR_GAP: f32 = 8.0;
                    let items = self.gap_items.borrow();
                    let in_part = |it: &&GapItem| (it.part_y - part_start_y).abs() < 0.5;
                    // Not those sent beyond the other staff (outside the part,
                    // beside a cross-staff beam).
                    let uppers: Vec<&GapItem> = items
                        .iter()
                        .filter(in_part)
                        .filter(|it| it.staff == s && it.is_below && it.top < lower_bottom)
                        .collect();
                    let lowers: Vec<&GapItem> = items
                        .iter()
                        .filter(in_part)
                        .filter(|it| it.staff == s + 1 && !it.is_below && it.bottom > upper_top)
                        .collect();
                    let notes = self.placed_notes.borrow();
                    let staff_notes = |staff: i32, x1: f32, x2: f32| {
                        notes
                            .iter()
                            .filter(move |pn| {
                                (pn.part_y - part_start_y).abs() < 0.5
                                    && pn.staff == staff
                                    && pn.x > x1 - 6.0
                                    && pn.x < x2 + 6.0
                            })
                            .map(|pn| self.note_extent_own_staff(pn))
                    };
                    // Lowest (highest) point of a slur over an item's span.
                    let slur_y_over = |sl: &PlacedSlur, x1: f32, x2: f32, lowest: bool| {
                        let (a, b) = sl.x_range();
                        let (lo, hi) = (x1.max(a), x2.min(b));
                        (lo < hi).then(|| {
                            (0..=8)
                                .map(|i| sl.y_at(lo + (hi - lo) * i as f32 / 8.0))
                                .fold(if lowest { f32::MIN } else { f32::MAX }, |acc, y| {
                                    if lowest { acc.max(y) } else { acc.min(y) }
                                })
                        })
                    };
                    for u in &uppers {
                        extra = extra.max(u.bottom + DIR_GAP - lower_top);
                        for b in slurs.iter().filter(|b| owned(b, s + 1)) {
                            if let Some(y) = slur_y_over(b, u.x1, u.x2, false) {
                                extra = extra.max(u.bottom + DIR_GAP - y);
                            }
                        }
                        for (top, _) in staff_notes(s + 1, u.x1, u.x2) {
                            extra = extra.max(u.bottom + DIR_GAP - top);
                        }
                        for l in lowers.iter().filter(|l| l.x1 < u.x2 && l.x2 > u.x1) {
                            extra = extra.max(u.bottom + DIR_GAP - l.top);
                        }
                    }
                    for l in &lowers {
                        extra = extra.max(upper_bottom + DIR_GAP - l.top);
                        for a in slurs.iter().filter(|a| owned(a, s)) {
                            if let Some(y) = slur_y_over(a, l.x1, l.x2, true) {
                                extra = extra.max(y + DIR_GAP - l.top);
                            }
                        }
                        for (_, bottom) in staff_notes(s, l.x1, l.x2) {
                            extra = extra.max(bottom + DIR_GAP - l.top);
                        }
                    }
                }
                // A slur reaching into the gap must also clear the other
                // staff's noteheads and stems hanging into it.
                for pn in self.placed_notes.borrow().iter().filter(|pn| pn.part_y == part_start_y) {
                    // (Not a cross-staff beam's stem: its length follows the gap.)
                    let stem_tip = |up: bool| match pn.stem {
                        Some((sx, tip, is_up)) if is_up == up && !pn.cross_beam => Some((sx, tip)),
                        _ => None,
                    };
                    if pn.staff == s && pn.top > upper_top - 150.0 && pn.bottom < lower_top {
                        // Upper-staff note: its lowest points vs slurs rising
                        // above the lower staff.
                        let mut pts = vec![(pn.x, pn.bottom)];
                        pts.extend(stem_tip(false));
                        for b in slurs.iter() {
                            // An over-slur starting and ending below the upper
                            // staff (its ends may sit on ledger lines in the gap).
                            let ends_top = b.y1.min(b.y2);
                            if !(ends_top > upper_bottom + 5.0
                                && b.top < ends_top - 1.0
                                && b.top > upper_top)
                            {
                                continue;
                            }
                            let (b1, b2) = b.x_range();
                            for &(x, y) in &pts {
                                if x > b1 + 2.0 && x < b2 - 2.0 {
                                    extra = extra.max(y + SLUR_GAP - b.y_at(x));
                                }
                            }
                        }
                    } else if pn.staff == s + 1
                        && pn.top > upper_bottom
                        && pn.bottom < lower_bottom + 150.0
                    {
                        // Lower-staff note: its highest points vs slurs hanging
                        // below the upper staff.
                        let mut pts = vec![(pn.x, pn.top)];
                        pts.extend(stem_tip(true));
                        for a in slurs.iter() {
                            // An under-slur starting and ending above the lower
                            // staff.
                            // One sagging past the lower staff is not in the gap:
                            // widening it would only stretch the curve further.
                            let ends_bottom = a.y1.max(a.y2);
                            if !(ends_bottom < lower_top - 5.0
                                && a.bottom > ends_bottom + 1.0
                                && a.bottom < lower_bottom)
                            {
                                continue;
                            }
                            let (a1, a2) = a.x_range();
                            for &(x, y) in &pts {
                                if x > a1 + 2.0 && x < a2 - 2.0 {
                                    extra = extra.max(a.y_at(x) + SLUR_GAP - y);
                                }
                            }
                        }
                    }
                }
            }
            if extra > 0.5 {
                result.push((id.clone(), extra.ceil()));
            }
        }
        result
    }

    /// Cursor positions (onset time, x) for measure `m_idx`: its drawn onsets,
    /// plus its grace notes at the times the MIDI engine plays them. A run
    /// ending the measure sounds within the last three quarters of the note
    /// before it; any other run takes a 32nd per grace from its onset on,
    /// delaying the note it precedes.
    fn cursor_anchors(
        &self,
        m_idx: usize,
        x_base: f32,
        m_width: f32,
        time_to_x: &HashMap<i32, f32>,
    ) -> Vec<(i32, f32)> {
        const GRACE: i32 = 10080 / 8;
        let mut anchors: HashMap<i32, f32> =
            time_to_x.iter().map(|(&t, &r)| (t, x_base + r * m_width)).collect();
        let end = time_to_x.keys().copied().max().unwrap_or(0);
        let mut runs: HashMap<(String, i32, i32), Vec<f32>> = HashMap::new();
        for m in self.grace_marks.borrow().iter().filter(|m| m.measure_idx == m_idx) {
            runs.entry((m.part_id.clone(), m.voice, m.time)).or_default().push(m.x);
        }
        let mut runs: Vec<_> = runs.into_iter().collect();
        runs.sort_by_key(|((_, _, t), _)| *t);
        for ((_, _, t), mut xs) in runs {
            xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let n = xs.len() as i32;
            if t >= end && end > 0 {
                let prev = time_to_x.keys().copied().filter(|&k| k < t).max().unwrap_or(0);
                let unit = GRACE.min((t - prev) * 3 / 4 / n.max(1)).max(1);
                for (k, x) in xs.into_iter().enumerate() {
                    anchors.insert(t - (n - k as i32) * unit, x);
                }
            } else {
                let main_x = anchors.get(&t).copied();
                for (k, x) in xs.into_iter().enumerate() {
                    anchors.insert(t + k as i32 * GRACE, x);
                }
                if let Some(mx) = main_x {
                    anchors.insert(t + n * GRACE, mx);
                }
            }
        }
        anchors.into_iter().collect()
    }

    /// How far each part must move down so that what its upper neighbour
    /// drew late (pedal lines, slurs, directions, beamed stems) clears its
    /// staff and the notes, slurs and directions above it.
    fn part_overlaps(
        &self,
        score: &Score,
        part_y_offsets: &HashMap<String, f32>,
        system_render_y: f32,
    ) -> Vec<(String, f32)> {
        const MARGIN: f32 = 8.0;
        let ids: Vec<&String> = score
            .part_list
            .iter()
            .filter_map(|item| match item {
                PartListItem::Part { id, .. } if part_y_offsets.contains_key(id) => Some(id),
                _ => None,
            })
            .collect();
        let near = |a: f32, b: f32| (a - b).abs() < 0.5;
        let no_active = HashMap::new();
        let notes = self.placed_notes.borrow();
        let slurs = self.placed_slurs.borrow();
        let items = self.gap_items.borrow();
        let pedals = self.deferred_pedals.borrow();
        let mut result = Vec::new();
        for pair in ids.windows(2) {
            let py = system_render_y + part_y_offsets[pair[0]];
            let qy = system_render_y + part_y_offsets[pair[1]];
            // (x1, x2, bottom) of the upper part's elements.
            let mut upper: Vec<(f32, f32, f32)> = Vec::new();
            for p in pedals.iter().filter(|p| near(p.part_y, py) && p.is_below) {
                let y = self.pedal_y_clear_of_wedges(p.y, p.x1, p.x2, p.is_below, &no_active, p.part_y);
                upper.push((p.x1, p.x2, y + 10.0));
            }
            upper.extend(slurs.iter().filter(|sl| near(sl.part_y, py)).map(|sl| {
                let (a, b) = sl.x_range();
                (a, b, sl.bottom)
            }));
            upper.extend(items.iter().filter(|it| near(it.part_y, py)).map(|it| (it.x1, it.x2, it.bottom)));
            upper.extend(notes.iter().filter(|pn| near(pn.part_y, py)).map(|pn| {
                let bottom = pn.stem.map_or(pn.bottom, |(_, tip, _)| pn.bottom.max(tip));
                (pn.x - 6.0, pn.x + 6.0, bottom)
            }));
            // (x1, x2, top) of the lower part's elements.
            let mut lower: Vec<(f32, f32, f32)> = Vec::new();
            lower.extend(slurs.iter().filter(|sl| near(sl.part_y, qy)).map(|sl| {
                let (a, b) = sl.x_range();
                (a, b, sl.top)
            }));
            lower.extend(items.iter().filter(|it| near(it.part_y, qy)).map(|it| (it.x1, it.x2, it.top)));
            lower.extend(notes.iter().filter(|pn| near(pn.part_y, qy)).map(|pn| {
                let top = pn.stem.map_or(pn.top, |(_, tip, _)| pn.top.min(tip));
                (pn.x - 6.0, pn.x + 6.0, top)
            }));
            let mut extra: f32 = 0.0;
            for &(a1, a2, bottom) in &upper {
                // The lower part's top staff line runs the whole width.
                extra = extra.max(bottom + MARGIN - qy);
                for &(b1, b2, top) in &lower {
                    if a1 < b2 && a2 > b1 {
                        extra = extra.max(bottom + MARGIN - top);
                    }
                }
            }
            if extra > 0.5 {
                result.push((pair[1].clone(), extra.ceil()));
            }
        }
        result
    }

    /// Pushes a pedal line spanning x1..x2 clear of the hairpins under it:
    /// those already drawn in this system and those still running (which only
    /// get drawn at their stop, possibly on a later system). The pedal glyph
    /// rises about a staff space above its line, hence the extra margin.
    fn pedal_y_clear_of_wedges(
        &self,
        y: f32,
        x1: f32,
        x2: f32,
        is_below: bool,
        active_wedges: &HashMap<i32, WedgeStartInfo>,
        part_y: f32,
    ) -> f32 {
        const PEDAL_WEDGE_GAP: f32 = 22.0;
        let mut y = y;
        let overlaps = |a1: f32, a2: f32| a1 < x2 && a2 > x1;
        // Only the pedal's own part: another part's hairpins under the same x
        // would drag the pedal down past them, and the part below with it.
        for &(w1, w2, bottom, top, w_part_y) in self.placed_wedges.borrow().iter() {
            if (w_part_y - part_y).abs() < 0.5 && overlaps(w1, w2) {
                y = if is_below {
                    y.max(bottom + PEDAL_WEDGE_GAP)
                } else {
                    y.min(top - PEDAL_WEDGE_GAP)
                };
            }
        }
        for w in active_wedges.values() {
            if w.is_continuation || w.is_below != is_below || !overlaps(w.x, x2) {
                continue;
            }
            let half = w.start_spread.max(12.0) / 2.0;
            y = if is_below {
                y.max(w.y + half + PEDAL_WEDGE_GAP)
            } else {
                y.min(w.y - half - PEDAL_WEDGE_GAP)
            };
        }
        y
    }

    /// Addresses of `part`'s notes starting a wavy line with no matching stop
    /// (see `Renderer::open_wavy_notes`).
    /// Where a note is drawn on the staff: its pitch, or for an unpitched
    /// (percussion) note its display step and octave. None for a rest (whose
    /// display step, kept in the same field, places the rest glyph).
    fn display_pitch(n: &Note) -> Option<crate::models::Pitch> {
        if n.rest {
            return None;
        }
        n.pitch.clone().or_else(|| {
            n.unpitched.as_ref().map(|u| crate::models::Pitch {
                step: u.display_step.clone(),
                octave: u.display_octave,
                alter: None,
            })
        })
    }

    /// Addresses of a measure's notes (chord members included) in beam groups
    /// spanning two staves. Their stems run into the staff gap and take their
    /// length from it, so the gap mustn't be sized to hold them.
    fn cross_staff_beam_notes(measure: &crate::models::Measure) -> std::collections::HashSet<usize> {
        use crate::models::BeamValue;
        struct Group {
            staves: std::collections::HashSet<i32>,
            notes: Vec<usize>,
            ended: bool,
        }
        let mut result = std::collections::HashSet::new();
        let mut finish = |g: Group| {
            if g.staves.len() > 1 {
                result.extend(g.notes);
            }
        };
        // voice -> its current level-1 beam group (kept past its end note
        // until the next main note, so the end note's chord joins it)
        let mut groups: HashMap<i32, Group> = HashMap::new();
        for el in &measure.elements {
            let MeasureElement::Note(n) = el else { continue };
            let voice = n.voice.unwrap_or(1);
            let addr = n as *const Note as usize;
            let staff = n.staff.unwrap_or(1);
            if n.is_chord {
                if let Some(g) = groups.get_mut(&voice) {
                    g.staves.insert(staff);
                    g.notes.push(addr);
                }
                continue;
            }
            if groups.get(&voice).is_some_and(|g| g.ended) {
                finish(groups.remove(&voice).unwrap());
            }
            match n.beams.iter().find(|b| b.number == 1).map(|b| &b.value) {
                Some(BeamValue::Begin) => {
                    if let Some(g) = groups.remove(&voice) {
                        finish(g);
                    }
                    groups.insert(
                        voice,
                        Group { staves: [staff].into(), notes: vec![addr], ended: false },
                    );
                }
                Some(v @ (BeamValue::Continue | BeamValue::End)) => {
                    let g = groups.entry(voice).or_insert_with(|| Group {
                        staves: Default::default(),
                        notes: Vec::new(),
                        ended: false,
                    });
                    g.staves.insert(staff);
                    g.notes.push(addr);
                    g.ended = matches!(v, BeamValue::End);
                }
                _ => {
                    if let Some(g) = groups.remove(&voice) {
                        finish(g);
                    }
                }
            }
        }
        for (_, g) in groups {
            finish(g);
        }
        result
    }

    fn open_wavy_notes(part: &crate::models::Part) -> Vec<usize> {
        let mut open = Vec::new();
        let mut pending: HashMap<i32, usize> = HashMap::new();
        for m in &part.measures {
            for el in &m.elements {
                let MeasureElement::Note(n) = el else { continue };
                for notation in &n.notations {
                    let Notation::Ornaments(orns) = notation else { continue };
                    for o in orns {
                        let crate::models::Ornament::WavyLine { wavy_type, number, .. } = o else {
                            continue;
                        };
                        match wavy_type.as_str() {
                            "start" => {
                                if let Some(prev) = pending.insert(*number, n as *const Note as usize) {
                                    open.push(prev);
                                }
                            }
                            "stop" => {
                                pending.remove(number);
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        open.extend(pending.into_values());
        open
    }

    /// Hairpins of `part`, as (start measure index, number), that cross a
    /// measure holding a beam joining notes on two staves (see
    /// `Renderer::outer_wedges`).
    fn outer_wedges(part: &crate::models::Part) -> HashSet<(usize, i32)> {
        use crate::models::BeamValue;
        // Measures holding (part of) a cross-staff beam group.
        let mut cross: HashSet<usize> = HashSet::new();
        // voice -> (measure the group began in, staves of its notes)
        let mut groups: HashMap<i32, (usize, HashSet<i32>)> = HashMap::new();
        for (mi, m) in part.measures.iter().enumerate() {
            for el in &m.elements {
                let MeasureElement::Note(n) = el else { continue };
                let voice = n.voice.unwrap_or(1);
                let staff = n.staff.unwrap_or(1);
                if n.is_chord {
                    if let Some(g) = groups.get_mut(&voice) {
                        g.1.insert(staff);
                    }
                    continue;
                }
                match n.beams.iter().find(|b| b.number == 1).map(|b| &b.value) {
                    Some(BeamValue::Begin) => {
                        groups.insert(voice, (mi, HashSet::from([staff])));
                    }
                    Some(BeamValue::Continue) => {
                        groups.entry(voice).or_insert_with(|| (mi, HashSet::new())).1.insert(staff);
                    }
                    Some(BeamValue::End) => {
                        if let Some((start, mut staves)) = groups.remove(&voice) {
                            staves.insert(staff);
                            if staves.len() > 1 {
                                cross.extend(start..=mi);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        let mut out = HashSet::new();
        let mut open: HashMap<i32, usize> = HashMap::new();
        let close = |start: usize, end: usize, num: i32, out: &mut HashSet<(usize, i32)>| {
            if (start..=end).any(|k| cross.contains(&k)) {
                out.insert((start, num));
            }
        };
        for (mi, m) in part.measures.iter().enumerate() {
            for el in &m.elements {
                let MeasureElement::Direction(d) = el else { continue };
                for t in &d.types {
                    let crate::models::DirectionType::Wedge(w) = t else { continue };
                    let num = w.number.unwrap_or(1);
                    match w.wedge_type.as_str() {
                        "crescendo" | "diminuendo" => {
                            // A start reusing an open number replaces that
                            // hairpin, which runs up to here (as when drawn).
                            if let Some(start) = open.insert(num, mi) {
                                close(start, mi, num, &mut out);
                            }
                        }
                        "stop" => {
                            if let Some(start) = open.remove(&num) {
                                close(start, mi, num, &mut out);
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        let last = part.measures.len().saturating_sub(1);
        for (num, start) in open {
            close(start, last, num, &mut out);
        }
        out
    }

    /// Whether a direction on `staff` placed on that side sits between two
    /// staves of the part.
    fn between_staves(staff: i32, is_below: bool, num_staves: i32) -> bool {
        if is_below { staff < num_staves } else { staff > 1 }
    }

    /// Whether a direction on `staff` on that side goes in the gap to the
    /// next staff: it does unless a beam in the current measure joins the two
    /// staves (it fills the gap), in which case it goes outside the part as
    /// before.
    fn in_staff_gap(&self, staff: i32, is_below: bool, num_staves: i32) -> bool {
        if !Self::between_staves(staff, is_below, num_staves) {
            return false;
        }
        let other = if is_below { staff + 1 } else { staff - 1 };
        let (x1, x2) = self.measure_span.get();
        let part_y = self.current_part_y.get();
        !self.placed_notes.borrow().iter().any(|pn| {
            pn.part_y == part_y
                && (pn.staff == staff || pn.staff == other)
                && pn.x >= x1
                && pn.x <= x2
                && pn.cross_beam
        })
    }

    /// Vertical extent of `staff`'s notes (noteheads and stems at their final
    /// tips), slurs (not cross-staff ones) and tuplet brackets drawn in the
    /// current measure.
    fn staff_extent_in_measure(&self, staff: i32) -> Option<(f32, f32)> {
        let (x1, x2) = self.measure_span.get();
        let part_y = self.current_part_y.get();
        let notes = self.placed_notes.borrow();
        let note_ext = notes
            .iter()
            .filter(|pn| pn.part_y == part_y && pn.staff == staff && pn.x >= x1 && pn.x <= x2)
            .map(|pn| self.note_extent_own_staff(pn));
        let slurs = self.placed_slurs.borrow();
        let slur_ext = slurs
            .iter()
            .filter(|sl| {
                sl.staff == staff && !sl.cross_staff && (sl.part_y - part_y).abs() < 0.5
            })
            .filter_map(|sl| {
                let (a, b) = sl.x_range();
                let (lo, hi) = (x1.max(a), x2.min(b));
                (lo < hi).then(|| {
                    (0..=8)
                        .map(|i| sl.y_at(lo + (hi - lo) * i as f32 / 8.0))
                        .fold((f32::MAX, f32::MIN), |(t, b), y| (t.min(y), b.max(y)))
                })
            });
        let tuplets = self.placed_tuplets.borrow();
        let tuplet_ext = tuplets
            .iter()
            .filter(|&&(t_part_y, t_staff, tx1, tx2, _, _)| {
                (t_part_y - part_y).abs() < 0.5 && t_staff == staff && tx1 < x2 && tx2 > x1
            })
            .map(|&(_, _, _, _, top, bottom)| (top, bottom));
        note_ext
            .chain(slur_ext)
            .chain(tuplet_ext)
            .reduce(|a, b| (a.0.min(b.0), a.1.max(b.1)))
    }

    /// A placed note's extent with its stem, unless the stem joins a
    /// cross-staff beam: such a stem's length follows the staff gap, so it
    /// can't bound what is placed in that gap.
    fn note_extent_own_staff(&self, pn: &PlacedNote) -> (f32, f32) {
        match pn.stem {
            Some((_, tip, _)) if !pn.cross_beam => (pn.top.min(tip), pn.bottom.max(tip)),
            _ => (pn.top, pn.bottom),
        }
    }

    fn push_gap_item(&self, staff: i32, is_below: bool, x1: f32, x2: f32, top: f32, bottom: f32) {
        self.gap_items.borrow_mut().push(GapItem {
            part_y: self.current_part_y.get(),
            staff,
            is_below,
            x1: x1.min(x2),
            x2: x1.max(x2),
            top,
            bottom,
        });
    }

    /// Shared baseline for dynamics and hairpins of a staff in a measure:
    /// a fixed distance from the staff, pushed clear of the measure's notes.
    fn dynamics_line_y(
        &self,
        is_below: bool,
        staff: i32,
        num_staves: i32,
        in_gap: Option<bool>,
        part_start_y: f32,
        staff_dist: f32,
        measure_extreme_occ: Option<(f32, f32)>,
        beat_occ: Option<(f32, f32)>,
    ) -> f32 {
        let staff_y = part_start_y + (staff as f32 - 1.0) * staff_dist;
        let base = if is_below {
            staff_y + 6.0 * self.staff_line_distance
        } else {
            staff_y - 2.0 * self.staff_line_distance
        };
        // Between two staves only this staff's own notes count (the part-wide
        // occupancy would push it into the other staff); the staff gap is
        // widened afterwards if it then meets the other staff.
        // A hairpin passes its own decision (made for its whole length);
        // anything else decides for the current measure.
        let in_gap = in_gap.unwrap_or_else(|| self.in_staff_gap(staff, is_below, num_staves));
        if in_gap {
            return match self.staff_extent_in_measure(staff) {
                Some((_, bottom)) if is_below => base.max(bottom + 15.0),
                Some((top, _)) if !is_below => base.min(top - 15.0),
                _ => base,
            };
        }
        match measure_extreme_occ.or(beat_occ) {
            Some((_, max_y)) if is_below => base.max(max_y + 15.0),
            Some((min_y, _)) => base.min(min_y - 15.0),
            None => base,
        }
    }

    fn get_priority(&self, dtype: &crate::models::DirectionType) -> i32 {
        match dtype {
            crate::models::DirectionType::Dynamics(_) => 1,
            crate::models::DirectionType::OctaveShift(_) => 2,
            crate::models::DirectionType::Dashes(_) | crate::models::DirectionType::Bracket(_) => 3,
            crate::models::DirectionType::Wedge(_) => 4,
            crate::models::DirectionType::Pedal(_) => 5,
            crate::models::DirectionType::Segno
            | crate::models::DirectionType::Coda
            | crate::models::DirectionType::Damp
            | crate::models::DirectionType::DampAll => 6,
            crate::models::DirectionType::Words(_) | crate::models::DirectionType::Other(_) => 7,
            crate::models::DirectionType::Metronome(_) => 10,
            crate::models::DirectionType::Rehearsal(_) => 11,
        }
    }

    fn get_beat_repeat_symbol(&self, slashes: i32) -> &str {
        match slashes {
            2 => "\u{E502}",
            3 => "\u{E503}",
            4 => "\u{E504}",
            _ => "\u{E504}", // User requested E504 for beat-repeat
        }
    }

    fn note_type_to_duration(&self, note_type: &str, divisions: i32) -> f32 {
        let base = match note_type {
            "maxima" => 32.0,
            "long" => 16.0,
            "breve" => 8.0,
            "whole" => 4.0,
            "half" => 2.0,
            "quarter" => 1.0,
            "eighth" => 0.5,
            "16th" => 0.25,
            "32nd" => 0.125,
            "64th" => 0.0625,
            "128th" => 0.03125,
            "256th" => 0.015625,
            _ => 1.0,
        };
        base * divisions as f32
    }

    fn infer_note_type_from_duration(&self, duration: i32, divisions: i32) -> &'static str {
        let quarter = divisions.max(1) as f32;
        let ratio = duration.max(1) as f32 / quarter;
        const TYPES: [(&str, f32); 10] = [
            ("whole", 4.0),
            ("half", 2.0),
            ("quarter", 1.0),
            ("eighth", 0.5),
            ("16th", 0.25),
            ("32nd", 0.125),
            ("64th", 0.0625),
            ("128th", 0.03125),
            ("256th", 0.015625),
            ("512th", 0.0078125),
        ];
        TYPES
            .iter()
            .min_by(|a, b| {
                (ratio - a.1)
                    .abs()
                    .partial_cmp(&(ratio - b.1).abs())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(name, _)| *name)
            .unwrap_or("quarter")
    }

    fn process_chord_group(
        &self,
        mut doc: Document,
        chord_group: &[&Note],
        current_clefs: &HashMap<i32, Clef>,
        part_start_y: f32,
        start_x: f32,
        width: f32,
        time_map: &HashMap<i32, f32>,
        measure_total_dur: i32,
        measure_theoretical_dur: i32,
        time_pos: i32,
        active_beams: &mut HashMap<(i32, i32), Vec<(StemInfo, Vec<crate::models::Beam>)>>,
        active_slurs: &mut HashMap<i32, SlurStartInfo>,
        active_ties: &mut HashMap<(String, i32, i32), TieStartInfo>,
        active_tuplets: &mut HashMap<i32, TupletStartInfo>,
        _active_lyrics: &mut HashMap<i32, LyricStartInfo>,
        active_glissandi: &mut HashMap<i32, GlissandoStartInfo>,
        active_slides: &mut HashMap<i32, SlideStartInfo>,
        active_hammer_ons: &mut HashMap<i32, HammerOnPullOffStartInfo>,
        active_pull_offs: &mut HashMap<i32, HammerOnPullOffStartInfo>,
        active_tremolos: &mut HashMap<i32, TremoloStartInfo>,
        active_wavy_lines: &mut HashMap<i32, WavyLineStartInfo>,
        active_octave_shifts: &HashMap<i32, OctaveShiftStartInfo>,
        priority_elements: &mut Vec<StackedElement>,
        staff_dist: f32,
        slash_active: bool,
        slash_use_stems: bool,
        slash_dots: i32,
        slash_note_type: Option<&str>,
        beat_repeat_active: bool,
        beat_repeat_slashes: i32,
        current_beats: i32,
        divisions: i32,
        current_time_x_offset: f32,
        staff_lines: &HashMap<i32, i32>,
        _lyric_baseline_y: f32,
        grace_offset: f32,
        voice_info: &MeasureVoiceInfo,
    ) -> (Document, f32, f32, f32) {
        let staff_num = chord_group[0].staff.unwrap_or(1);
        let staff_idx = (staff_num - 1).max(0);
        let staff_y_offset = part_start_y + (staff_idx as f32 * staff_dist);
        let line_count = *staff_lines.get(&staff_num).unwrap_or(&5);
        let default_clef = Clef {
            number: 1,
            sign: "G".to_string(),
            line: Some(2),
            ..Default::default()
        };
        let clef = current_clefs.get(&staff_num).unwrap_or(&default_clef);
        let is_measure_rest = chord_group[0].rest_measure;
        let duration_units = if divisions > 0 {
            chord_group[0].duration as f32 * 10080.0 / divisions as f32
        } else {
            chord_group[0].duration as f32
        };
        let is_full_measure_rest = is_measure_rest
            && chord_group[0].note_type.is_none()
            && measure_theoretical_dur > 0
            && duration_units >= measure_theoretical_dur as f32 - 0.5;

        // Apply octave shift to visual representation if active
        let mut visual_notes = Vec::new(); // Owned notes for visual shift

        let mut shift_amount = 0;
        for shift in active_octave_shifts.values() {
            if shift.staff.unwrap_or(1) == staff_num && !shift.ended {
                let octaves = if shift.size >= 22 {
                    3
                } else if shift.size >= 15 {
                    2
                } else {
                    1
                };
                shift_amount = if shift.shift_type == "down" {
                    -octaves
                } else if shift.shift_type == "up" {
                    octaves
                } else {
                    0
                };
                break;
            }
        }

        for note in chord_group {
            if shift_amount != 0 {
                let mut visual_note = (*note).clone();
                if let Some(pitch) = &mut visual_note.pitch {
                    pitch.octave += shift_amount;
                }
                visual_notes.push(visual_note);
            }
        }

        let effective_chord_group: Vec<&Note> = if shift_amount != 0 {
            visual_notes.iter().collect()
        } else {
            chord_group.to_vec()
        };
        // Duplicate unisons in one chord print as extra stemless noteheads; collapse them.
        let effective_chord_group = utils::dedupe_chord_unisons(effective_chord_group);

        let total_dur = effective_chord_group[0].duration;
        let mut beat_dur = if current_beats > 0 && measure_total_dur > 0 {
            measure_total_dur as f32 / current_beats as f32
        } else {
            divisions as f32
        }
        .max(1.0);

        if slash_active {
            if let Some(nt) = slash_note_type {
                let mut base_dur = self.note_type_to_duration(nt, divisions);
                // Apply dots
                let mut added = base_dur * 0.5;
                for _ in 0..slash_dots {
                    base_dur += added;
                    added *= 0.5;
                }
                // Normalize beat_dur to the 10080 internal resolution
                beat_dur = base_dur * (10080.0 / divisions as f32);
            }
        }

        // rhythmic slash notation: standard behavior is to draw one slash per beat
        // regardless of the actual notes in the measure (unless it's 'beat-repeat' or 'measure-repeat').
        if slash_active {
            let num_beats = if measure_total_dur > 0 {
                (measure_total_dur as f32 / beat_dur).round() as i32
            } else {
                current_beats
            }
            .max(1);
            let group_start_x = start_x;
            let group_end_x = start_x + width;
            let group_width = (group_end_x - group_start_x).max(0.0);

            for i in 0..num_beats {
                let bx = group_start_x + (i as f32 + 0.5) * (group_width / num_beats as f32);
                let by = staff_y_offset + 2.0 * self.staff_line_distance;
                let slash_notehead = crate::models::Notehead {
                    value: "slash".to_string(),
                    filled: None,
                };
                doc = self.draw_notehead(doc, bx, by, "quarter", Some(&slash_notehead), false);
            }
            return (
                doc,
                staff_y_offset,
                staff_y_offset + 40.0,
                start_x + self.note_head_width * 0.5 + 4.0,
            );
        }

        if (beat_repeat_active) && (total_dur as f32 > beat_dur || is_full_measure_rest) {
            let num_beats = if is_full_measure_rest {
                current_beats
            } else {
                (total_dur as f32 / beat_dur).round() as i32
            }
            .max(1);
            let _group_start_ratio = if is_full_measure_rest {
                0.0
            } else {
                time_map.get(&time_pos).cloned().unwrap_or(0.0)
            };
            let group_end_time = if is_full_measure_rest {
                measure_total_dur
            } else {
                time_pos + total_dur
            };
            let group_end_ratio = time_map.get(&group_end_time).cloned().unwrap_or(1.0);

            // Critical: Available width must account for the shift caused by attributes
            let group_start_x = start_x; // start_x already includes current_time_x_offset
            let group_end_x = (start_x - current_time_x_offset) + group_end_ratio * width;
            let group_width = (group_end_x - group_start_x).max(0.0);

            for i in 0..num_beats {
                let bx = group_start_x + (i as f32 + 0.5) * (group_width / num_beats as f32);
                let by = staff_y_offset + 2.0 * self.staff_line_distance;
                if beat_repeat_active {
                    let sym = self.get_beat_repeat_symbol(beat_repeat_slashes);
                    doc = doc.add(
                        Text::new(sym)
                            .set("x", bx)
                            .set("y", by)
                            .set("font-size", self.staff_line_distance * 4.0)
                            .set("text-anchor", "middle")
                            .set("dominant-baseline", "central")
                            .set("font-family", self.font_family.as_str()),
                    );
                }
            }
            return (
                doc,
                staff_y_offset,
                staff_y_offset + 40.0,
                start_x + self.note_head_width * 0.5 + 4.0,
            );
        }

        let x_offset = if is_full_measure_rest {
            if measure_total_dur > 0 {
                0.5 * (width - current_time_x_offset)
            } else {
                0.0
            }
        } else {
            let ratio = time_map.get(&time_pos).cloned().unwrap_or(0.0);
            ratio * width
        };
        // Nudge right when this voice's noteheads would overlap another voice's.
        let voice_offset = voice_info
            .offsets
            .get(&(staff_num, time_pos, chord_group[0].voice.unwrap_or(1)))
            .map_or(0.0, |o| o * self.note_head_width);
        let x = start_x + x_offset + grace_offset * self.grace_scale.get() + voice_offset;
        // (Measurement passes run with no width; only real drawing counts.)
        if chord_group[0].grace.is_some() && width > 0.0 {
            let mark = GraceMark {
                measure_idx: self.current_measure_idx.get(),
                part_id: self.current_part_id.borrow().clone(),
                voice: chord_group[0].voice.unwrap_or(1),
                time: time_pos,
                x,
            };
            let mut marks = self.grace_marks.borrow_mut();
            // A system laid out again draws its graces again at the same place.
            let dup = marks.iter().any(|m| {
                m.measure_idx == mark.measure_idx
                    && m.part_id == mark.part_id
                    && m.voice == mark.voice
                    && m.time == mark.time
                    && (m.x - mark.x).abs() < 0.5
            });
            if !dup {
                marks.push(mark);
            }
        }
        let (new_doc, stem, note_x, note_y, pitches_y, note_xs, note_ys) = self.draw_chord_group_at(
            doc,
            x,
            &effective_chord_group,
            clef,
            staff_y_offset,
            slash_active,
            slash_use_stems,
            slash_dots,
            slash_note_type,
            line_count,
            measure_total_dur,
            measure_theoretical_dur,
            divisions,
            time_pos,
            voice_info,
        );
        doc = new_doc;
        let note_right_edge = note_x + self.note_head_width * 0.5 + 4.0;
        // An invisible note (print-object="no") has no drawn position (its y is
        // a meaningless 0): keep beam groups going and close spanners that end
        // on it, and report no vertical extent, so it can't drag directions
        // placed around the measure off the page. Slurs still attach where its
        // notehead would be, as MuseScore draws them: exporters hide a voice's
        // note under another voice's identical notehead and keep the slur on
        // the hidden one.
        if effective_chord_group.iter().all(|n| n.print_object == Some(false)) {
            doc = self.process_beams(doc, &effective_chord_group, stem, active_beams);
            let beam_tips: Vec<BeamedStemTip> =
                self.beamed_stem_tips.borrow_mut().drain(..).collect();
            Self::reanchor_slurs_to_beams(active_slurs, &beam_tips);
            let hidden_ys: Vec<f32> = effective_chord_group
                .iter()
                .filter_map(|n| n.pitch.as_ref())
                .map(|p| self.pitch_to_y(p, clef, staff_y_offset))
                .collect();
            if !hidden_ys.is_empty() && clef.sign != "TAB" {
                let voice_sharing =
                    Self::voice_sharing(chord_group[0], time_pos, divisions, voice_info);
                let (new_doc, _, _) = self.process_slurs(
                    doc,
                    &effective_chord_group,
                    note_x,
                    note_y,
                    None,
                    active_slurs,
                    false,
                    &hidden_ys,
                    &vec![note_x; effective_chord_group.len()],
                    &[],
                    voice_sharing,
                );
                doc = new_doc;
            }
            // Exporters may also put the trill itself (mark and wavy line) on
            // the first hidden note of the written-out trill, under the
            // visible note: draw it where that note would be, like slurs.
            // Only when it starts a wavy line and trills are all it carries; a
            // lone trill mark on a hidden note duplicates the visible one's.
            let ornaments: Vec<&crate::models::Ornament> = effective_chord_group
                .iter()
                .flat_map(|n| n.notations.iter())
                .filter_map(|nt| match nt {
                    Notation::Ornaments(orns) => Some(orns.iter()),
                    _ => None,
                })
                .flatten()
                .collect();
            let trill_on_hidden = !hidden_ys.is_empty()
                && ornaments.iter().any(|o| {
                    matches!(o, crate::models::Ornament::WavyLine { wavy_type, .. } if wavy_type == "start")
                })
                && ornaments.iter().all(|o| {
                    matches!(
                        o,
                        crate::models::Ornament::TrillMark
                            | crate::models::Ornament::WavyLine { .. }
                    )
                });
            if trill_on_hidden {
                let top = hidden_ys.iter().copied().fold(f32::MAX, f32::min) - 5.0;
                let bottom = hidden_ys.iter().copied().fold(f32::MIN, f32::max) + 5.0;
                let (new_doc, _, _) = self.process_ornaments(
                    doc,
                    &effective_chord_group,
                    note_x,
                    top,
                    bottom,
                    None,
                    active_tremolos,
                    active_wavy_lines,
                    staff_y_offset,
                );
                doc = new_doc;
            }
            for n in &effective_chord_group {
                for notation in &n.notations {
                    match notation {
                        Notation::Slur { number, note_type, .. }
                            if note_type == "stop" && hidden_ys.is_empty() =>
                        {
                            active_slurs.remove(number);
                        }
                        Notation::Tied { note_type } if note_type == "stop" => {
                            if let Some(p) = &n.pitch {
                                active_ties.remove(&(p.step.clone(), p.octave, n.staff.unwrap_or(1)));
                            }
                        }
                        // A trill's wavy line may end on a hidden note of the
                        // written-out trill: end it there, as the stop on a
                        // visible note does. (Starts are handled below.)
                        Notation::Ornaments(orns) if !trill_on_hidden => {
                            for o in orns {
                                let crate::models::Ornament::WavyLine {
                                    wavy_type,
                                    number,
                                    relative_x,
                                } = o
                                else {
                                    continue;
                                };
                                if wavy_type != "stop" {
                                    continue;
                                }
                                if let Some(info) = active_wavy_lines.remove(number) {
                                    let end_x = note_x + relative_x.unwrap_or(20.0);
                                    doc = self.draw_wavy_line(doc, info.x, info.y, end_x, info.y);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            return (doc, f32::MAX, f32::MIN, note_x);
        }
        // Every notehead of the chord counts (not just the first one), so a
        // wide chord's far notes are cleared by what is placed around it.
        let mut min_y = pitches_y.iter().copied().fold(note_y, f32::min) - 5.0;
        let mut max_y = pitches_y.iter().copied().fold(note_y, f32::max) + 5.0;
        if let Some(s) = stem {
            min_y = min_y.min(s.y_tip).min(s.y_start);
            max_y = max_y.max(s.y_tip).max(s.y_start);
        }
        let is_tab = clef.sign == "TAB";
        doc = self.process_beams(doc, &effective_chord_group, stem, active_beams);
        // Beam groups just drawn may have moved stem tips; re-anchor open slurs
        // and their stem obstacles, and hand the tips to process_slurs so a
        // slur ending on this note uses the final tip too.
        let beam_tips: Vec<BeamedStemTip> = self.beamed_stem_tips.borrow_mut().drain(..).collect();
        Self::reanchor_slurs_to_beams(active_slurs, &beam_tips);
        let (new_doc, new_min, new_max) = self.process_tuplets(
            doc,
            &effective_chord_group,
            note_x,
            min_y,
            max_y,
            stem,
            active_tuplets,
        );
        doc = new_doc;
        if new_min != f32::MAX {
            min_y = min_y.min(new_min);
        }
        if new_max != f32::MIN {
            max_y = max_y.max(new_max);
        }

        let voice_sharing = Self::voice_sharing(chord_group[0], time_pos, divisions, voice_info);
        let (new_doc, new_min, new_max) = self.process_slurs(
            doc,
            &effective_chord_group,
            note_x,
            note_y,
            stem,
            active_slurs,
            is_tab,
            &pitches_y,
            &note_xs,
            &beam_tips,
            voice_sharing,
        );
        doc = new_doc;
        if new_min != f32::MAX {
            min_y = min_y.min(new_min);
        }
        if new_max != f32::MIN {
            max_y = max_y.max(new_max);
        }
        let (new_doc, new_min, new_max) = self.process_ties(
            doc,
            &effective_chord_group,
            note_x,
            note_y,
            stem,
            active_ties,
            &note_xs,
            &note_ys,
        );
        doc = new_doc;
        if new_min != f32::MAX {
            min_y = min_y.min(new_min);
        }
        if new_max != f32::MIN {
            max_y = max_y.max(new_max);
        }
        let (new_doc, new_min, new_max) = self.process_articulations(
            doc,
            &effective_chord_group,
            note_x,
            min_y,
            max_y,
            stem,
            &pitches_y,
            staff_y_offset,
        );
        doc = new_doc;
        min_y = new_min;
        max_y = new_max;
        let (new_doc, new_min, new_max) =
            self.process_fermatas(doc, &effective_chord_group, note_x, min_y, max_y, stem, {
                // The staff's outer lines; a 1-line staff draws its line in the middle.
                let (top, bottom) = if line_count == 1 {
                    let mid = staff_y_offset + 2.0 * self.staff_line_distance;
                    (mid, mid)
                } else {
                    let span = (line_count - 1) as f32 * self.staff_line_distance;
                    (staff_y_offset, staff_y_offset + span)
                };
                (top, bottom)
            });
        doc = new_doc;
        min_y = new_min;
        max_y = new_max;
        // Tuplets were already drawn before slurs so the slur layer sits on top.

        for slur in active_slurs.values().filter(|s| s.pending_stop.is_none()) {
            // Include the notes the slur must clear, not just its start point.
            if slur.is_above {
                let top = slur.obstacles.iter().map(|o| o.1).fold(slur.y, f32::min);
                min_y = min_y.min(top - 20.0);
            } else {
                let bottom = slur.obstacles.iter().map(|o| o.1).fold(slur.y, f32::max);
                max_y = max_y.max(bottom + 20.0);
            }
        }

        for tie in active_ties.values() {
            if tie.is_above {
                min_y = min_y.min(tie.y - 12.0);
            } else {
                max_y = max_y.max(tie.y + 12.0);
            }
        }

        let (new_doc, new_min, new_max) =
            self.process_accidental_marks(doc, &effective_chord_group, note_x, min_y, max_y, stem);
        doc = new_doc;
        min_y = new_min;
        max_y = new_max;
        let (new_doc, new_min, new_max) = self.process_ornaments(
            doc,
            &effective_chord_group,
            note_x,
            min_y,
            max_y,
            stem,
            active_tremolos,
            active_wavy_lines,
            staff_y_offset,
        );
        doc = new_doc;
        min_y = new_min;
        max_y = new_max;
        // A wavy line that is never stopped ends where its note does: at the
        // next onset, or just before the barline.
        for note in chord_group {
            if !self.open_wavy_notes.borrow().contains(&(*note as *const Note as usize)) {
                continue;
            }
            for notation in &note.notations {
                let Notation::Ornaments(orns) = notation else { continue };
                for o in orns {
                    let crate::models::Ornament::WavyLine { wavy_type, number, .. } = o else {
                        continue;
                    };
                    if wavy_type != "start" {
                        continue;
                    }
                    if let Some(info) = active_wavy_lines.remove(number) {
                        let end_time = time_pos + duration_units.round() as i32;
                        let end_x = match time_map.get(&end_time) {
                            Some(r) => start_x + r * width - self.note_head_width,
                            None => start_x + width - current_time_x_offset - 4.0,
                        };
                        doc = self.draw_wavy_line(doc, info.x, info.y, end_x.max(info.x + 10.0), info.y);
                    }
                }
            }
        }
        let (new_doc, new_min, new_max) = self.process_technical(
            doc,
            &effective_chord_group,
            note_x,
            min_y,
            max_y,
            stem,
            staff_y_offset,
        );
        doc = new_doc;
        min_y = new_min;
        max_y = new_max;
        doc = self.place_beam_marks(doc, &beam_tips, false);

        // Collect lyrics and harmonies for prioritized stacking
        for note in &effective_chord_group {
            for lyric in &note.lyrics {
                priority_elements.push(StackedElement {
                    priority: 9,
                    time_pos,
                    item: StackedItem::Lyric(lyric.clone(), note.staff.unwrap_or(1)),
                });
            }
            for harmony in &note.harmonies {
                priority_elements.push(StackedElement {
                    priority: 8,
                    time_pos,
                    item: StackedItem::Harmony(harmony.clone()),
                });
            }
        }

        doc = self.process_arpeggios(doc, &effective_chord_group, note_x, &pitches_y);

        if let Some(first_note) = effective_chord_group.first() {
            if first_note
                .notations
                .iter()
                .any(|n| matches!(n, Notation::NonArpeggiate { .. }))
            {
                if !pitches_y.is_empty() {
                    let mut sorted = pitches_y.clone();
                    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
                    let top_y = sorted[0];
                    let bottom_y = *sorted.last().unwrap();

                    let has_accidental = effective_chord_group.iter().any(|n| {
                        n.accidental.is_some()
                            || n.pitch.as_ref().map_or(false, |p| {
                                p.alter.is_some() && p.alter.unwrap().abs() > 0.01
                            })
                    });
                    let offset = if has_accidental { -28.0 } else { -15.0 };

                    doc = self.draw_non_arpeggio(doc, note_x + offset, top_y, bottom_y);
                }
            }
        }

        // Find the top-most note index and any available glissando text in this group
        let mut top_idx = 0;
        let mut min_y_val = f32::MAX;
        let mut combined_text = None;
        for (i, &py) in pitches_y.iter().enumerate() {
            if py < min_y_val {
                min_y_val = py;
                top_idx = i;
            }
            if combined_text.is_none() {
                for notation in &chord_group[i].notations {
                    if let Notation::Glissando(gliss) = notation {
                        if gliss.text.is_some() {
                            combined_text = gliss.text.clone();
                        }
                    }
                }
            }
        }

        for (idx, (note, &n_y)) in chord_group.iter().zip(pitches_y.iter()).enumerate() {
            for notation in &note.notations {
                if let Notation::Glissando(gliss) = notation {
                    if gliss.gliss_type == "start" {
                        // Assign the combined text only to the top-most note's line
                        let text_to_store = if idx == top_idx {
                            combined_text.clone()
                        } else {
                            None
                        };
                        active_glissandi.insert(
                            gliss.number,
                            GlissandoStartInfo {
                                x: note_x,
                                y: n_y,
                                line_type: gliss.line_type.clone(),
                                text: text_to_store,
                                is_continuation: false,
                            },
                        );
                    } else if gliss.gliss_type == "stop" {
                        if let Some(start_info) = active_glissandi.remove(&gliss.number) {
                            doc = self.draw_glissando(
                                doc,
                                start_info.x,
                                start_info.y,
                                note_x,
                                n_y,
                                &start_info.line_type,
                                &start_info.text,
                                start_info.is_continuation,
                                false,
                            );
                        }
                    }
                }
                if let Notation::Slide(slide) = notation {
                    if slide.slide_type == "start" {
                        active_slides.insert(
                            slide.number,
                            SlideStartInfo {
                                x: note_x,
                                y: n_y,
                                line_type: slide.line_type.clone(),
                                is_continuation: false,
                            },
                        );
                    } else if slide.slide_type == "stop" {
                        if let Some(start_info) = active_slides.remove(&slide.number) {
                            doc = self.draw_slide(
                                doc,
                                start_info.x,
                                start_info.y,
                                note_x,
                                n_y,
                                &start_info.line_type,
                                start_info.is_continuation,
                                false,
                            );
                        }
                    }
                }
                if let Notation::Technical(techs) = notation {
                    for tech in techs {
                        match tech {
                            crate::models::TechnicalMark::HammerOn {
                                number,
                                mark_type,
                                text,
                            } => {
                                if mark_type == "start" {
                                    // Move start point up for TAB to avoid fret number overlap
                                    let adjusted_y = if is_tab { n_y - 5.0 } else { n_y };
                                    active_hammer_ons.insert(
                                        *number,
                                        HammerOnPullOffStartInfo {
                                            x: note_x,
                                            y: adjusted_y,
                                            text: text.clone(),
                                        },
                                    );
                                } else if mark_type == "stop" {
                                    if let Some(start_info) = active_hammer_ons.remove(number) {
                                        let adjusted_y = if is_tab { n_y - 5.0 } else { n_y };
                                        doc = self.draw_hammer_on_pull_off(
                                            doc,
                                            start_info.x,
                                            start_info.y,
                                            note_x,
                                            adjusted_y,
                                            &start_info.text,
                                        );
                                    }
                                }
                            }
                            crate::models::TechnicalMark::PullOff {
                                number,
                                mark_type,
                                text,
                            } => {
                                if mark_type == "start" {
                                    let adjusted_y = if is_tab { n_y - 5.0 } else { n_y };
                                    active_pull_offs.insert(
                                        *number,
                                        HammerOnPullOffStartInfo {
                                            x: note_x,
                                            y: adjusted_y,
                                            text: text.clone(),
                                        },
                                    );
                                } else if mark_type == "stop" {
                                    if let Some(start_info) = active_pull_offs.remove(number) {
                                        let adjusted_y = if is_tab { n_y - 5.0 } else { n_y };
                                        doc = self.draw_hammer_on_pull_off(
                                            doc,
                                            start_info.x,
                                            start_info.y,
                                            note_x,
                                            adjusted_y,
                                            &start_info.text,
                                        );
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        (doc, min_y, max_y, note_right_edge)
    }

    fn draw_hammer_on_pull_off(
        &self,
        doc: Document,
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        text: &str,
    ) -> Document {
        let mid_x = (x1 + x2) / 2.0;
        let mid_y = (y1 + y2) / 2.0 - 15.0; // Place above the slur area
        doc.add(
            Text::new(text)
                .set("x", mid_x)
                .set("y", mid_y)
                .set("font-size", self.staff_line_distance * 1.5)
                .set("text-anchor", "middle")
                .set("dominant-baseline", "central")
                .set("font-family", "serif")
                .set("font-style", "italic")
                .set("fill", "black"),
        )
    }

    fn draw_multi_note_tremolo(
        &self,
        mut doc: Document,
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        bars: i32,
    ) -> Document {
        let dx = x2 - x1;
        let dy = y2 - y1;
        let angle = dy.atan2(dx);
        let dist = (dx * dx + dy * dy).sqrt();

        // Tremolo beams are usually centered and shorter than the actual interval
        let beam_len = (dist * 0.6).min(40.0).max(20.0);
        let mid_x = (x1 + x2) / 2.0;
        let mid_y = (y1 + y2) / 2.0;

        let beam_thickness = 4.0;
        let beam_spacing = 7.0;

        let start_offset = -(bars as f32 - 1.0) * beam_spacing * 0.5;

        for i in 0..bars {
            let offset_y = start_offset + (i as f32 * beam_spacing);

            // Calculate 4 corners of the tilted beam rectangle
            let cos = angle.cos();
            let sin = angle.sin();

            let h_len = beam_len * 0.5;
            let v_thick = beam_thickness * 0.5;

            // Points relative to mid_x, mid_y + offset_y
            // We want the beam to be tilted according to the interval angle
            let p1 = (
                mid_x - h_len * cos,
                mid_y + offset_y - h_len * sin - v_thick,
            );
            let p2 = (
                mid_x + h_len * cos,
                mid_y + offset_y + h_len * sin - v_thick,
            );
            let p3 = (
                mid_x + h_len * cos,
                mid_y + offset_y + h_len * sin + v_thick,
            );
            let p4 = (
                mid_x - h_len * cos,
                mid_y + offset_y - h_len * sin + v_thick,
            );

            let data = Data::new()
                .move_to(p1)
                .line_to(p2)
                .line_to(p3)
                .line_to(p4)
                .close();

            doc = doc.add(Path::new().set("fill", "black").set("d", data));
        }
        doc
    }

    fn draw_wavy_line(&self, doc: Document, x1: f32, y1: f32, x2: f32, _y2: f32) -> Document {
        let dx = x2 - x1;
        let dist = dx.abs();
        if dist < 1.0 {
            return doc;
        }

        let segments = (dist / 6.0).ceil() as i32;
        let mut data = Data::new().move_to((x1, y1));

        for i in 0..segments {
            let ex = x1 + (dx / segments as f32) * (i + 1) as f32;
            let mid_x = (x1 + (dx / segments as f32) * i as f32 + ex) / 2.0;

            // Alternate humps for a more natural wavy look
            let h = if i % 2 == 0 { -3.0 } else { 3.0 };

            data = data.quadratic_curve_to((mid_x, y1 + h, ex, y1));
        }

        doc.add(
            Path::new()
                .set("fill", "none")
                .set("stroke", "black")
                .set("stroke-width", 1.2)
                .set("d", data),
        )
    }
    fn draw_slide(
        &self,
        doc: Document,
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        line_type: &Option<String>,
        is_start_broken: bool,
        is_end_broken: bool,
    ) -> Document {
        let dx = x2 - x1;
        let dy = y2 - y1;
        let dist = (dx * dx + dy * dy).sqrt();
        if dist < 1.0 {
            return doc;
        }

        let angle = dy.atan2(dx);

        // Start/end padding to not touch noteheads
        let padding = 12.0;
        // Ensure padding doesn't exceed total distance for short segments
        let effective_padding = if dist < padding * 2.0 {
            dist * 0.3
        } else {
            padding
        };

        let sx = if is_start_broken {
            x1
        } else {
            x1 + effective_padding * angle.cos()
        };
        let sy = if is_start_broken {
            y1
        } else {
            y1 + effective_padding * angle.sin()
        };
        let ex = if is_end_broken {
            x2
        } else {
            x2 - effective_padding * angle.cos()
        };
        let ey = if is_end_broken {
            y2
        } else {
            y2 - effective_padding * angle.sin()
        };

        // For broken segments that are perfectly horizontal,
        // add a tiny vertical offset and a slight slope to indicate direction
        // and avoid perfectly overlapping with a staff line.
        let (adj_sy, adj_ey) = if (sy - ey).abs() < 0.1 && (is_start_broken || is_end_broken) {
            let offset = -1.0;
            if is_end_broken {
                (sy + offset, ey + offset - 2.0) // Slight downward slope if going to next system
            } else if is_start_broken {
                (sy + offset + 2.0, ey + offset) // Resuming with slope
            } else {
                (sy + offset, ey + offset)
            }
        } else {
            (sy, ey)
        };

        let mut line = Line::new()
            .set("x1", sx)
            .set("y1", adj_sy)
            .set("x2", ex)
            .set("y2", adj_ey)
            .set("stroke", "black")
            .set("stroke-width", 1.0);
        if let Some(lt) = line_type {
            if lt == "dashed" {
                line = line.set("stroke-dasharray", "4,4");
            } else if lt == "dotted" {
                line = line.set("stroke-dasharray", "1,3");
            }
        }
        doc.add(line)
    }

    fn draw_glissando(
        &self,
        mut doc: Document,
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        line_type: &Option<String>,
        text: &Option<String>,
        is_start_broken: bool,
        is_end_broken: bool,
    ) -> Document {
        let dx = x2 - x1;
        let dy = y2 - y1;
        let dist = (dx * dx + dy * dy).sqrt();
        if dist < 1.0 {
            return doc;
        }

        let angle = dy.atan2(dx);
        let angle_deg = angle.to_degrees();

        // Start/end padding to not touch noteheads
        let padding = 12.0;
        // Ensure padding doesn't exceed total distance for short segments
        let effective_padding = if dist < padding * 2.0 {
            dist * 0.3
        } else {
            padding
        };

        let sx = if is_start_broken {
            x1
        } else {
            x1 + effective_padding * angle.cos()
        };
        let sy = if is_start_broken {
            y1
        } else {
            y1 + effective_padding * angle.sin()
        };
        let ex = if is_end_broken {
            x2
        } else {
            x2 - effective_padding * angle.cos()
        };
        let ey = if is_end_broken {
            y2
        } else {
            y2 - effective_padding * angle.sin()
        };

        // For broken segments that are perfectly horizontal,
        // add a tiny vertical offset and a slight slope to indicate direction
        // and avoid perfectly overlapping with a staff line.
        let (adj_sy, adj_ey) = if (sy - ey).abs() < 0.1 && (is_start_broken || is_end_broken) {
            let offset = -1.0;
            if is_end_broken {
                (sy + offset, ey + offset - 2.0) // Slight downward slope
            } else if is_start_broken {
                (sy + offset + 2.0, ey + offset) // Resuming with slope
            } else {
                (sy + offset, ey + offset)
            }
        } else {
            (sy, ey)
        };

        if line_type.as_deref() == Some("wavy") {
            let mut data = Data::new().move_to((sx, adj_sy));
            let actual_dist =
                ((ex - sx) * (ex - sx) + (adj_ey - adj_sy) * (adj_ey - adj_sy)).sqrt();
            let segments = (actual_dist / 6.0) as i32;
            if segments > 0 {
                let step_x = (ex - sx) / segments as f32;
                let step_y = (adj_ey - adj_sy) / segments as f32;
                let amp = 2.5;
                let perp_x = -angle.sin();
                let perp_y = angle.cos();

                for i in 1..=segments {
                    let px = sx + i as f32 * step_x;
                    let py = adj_sy + i as f32 * step_y;
                    let phase = if i % 2 == 0 { amp } else { -amp };
                    data = data.line_to((px + phase * perp_x, py + phase * perp_y));
                }
            } else {
                data = data.line_to((ex, adj_ey));
            }
            doc = doc.add(
                Path::new()
                    .set("fill", "none")
                    .set("stroke", "black")
                    .set("stroke-width", 1.0)
                    .set("d", data),
            );
        } else {
            doc = doc.add(
                Line::new()
                    .set("x1", sx)
                    .set("y1", adj_sy)
                    .set("x2", ex)
                    .set("y2", adj_ey)
                    .set("stroke", "black")
                    .set("stroke-width", 1.0),
            );
        }

        if let Some(t) = text {
            let tx = (sx + ex) / 2.0;
            let ty = (adj_sy + adj_ey) / 2.0 - 5.0;
            let text_el = Text::new(t.as_str())
                .set("x", tx)
                .set("y", ty)
                .set("font-size", 9)
                .set("font-style", "italic")
                .set("font-family", "serif")
                .set("text-anchor", "middle")
                .set("dominant-baseline", "central")
                .set(
                    "transform",
                    format!("rotate({}, {}, {})", angle_deg, tx, ty),
                );
            doc = doc.add(text_el);
        }

        doc
    }

    fn draw_grouping_bracket(
        &self,
        mut doc: Document,
        x1: f32,
        y1: f32,
        x2: f32,
        _y2: f32,
        features: &[crate::models::GroupingFeature],
    ) -> Document {
        let bracket_y = y1 - 10.0;
        let data = Data::new()
            .move_to((x1, bracket_y + 5.0))
            .line_to((x1, bracket_y))
            .line_to((x2, bracket_y))
            .line_to((x2, bracket_y + 5.0));
        let path = Path::new()
            .set("fill", "none")
            .set("stroke", "gray")
            .set("stroke-width", 1.0)
            .set("d", data);
        doc = doc.add(path);

        if let Some(motif) = features.iter().find(|f| f.feature_type == "motif") {
            let text = Text::new(motif.text.as_str())
                .set("x", (x1 + x2) / 2.0)
                .set("y", bracket_y - 5.0)
                .set("font-size", 10)
                .set("font-style", "italic")
                .set("font-family", "serif")
                .set("text-anchor", "middle")
                .set("dominant-baseline", "central")
                .set("fill", "gray");
            doc = doc.add(text);
        }
        doc
    }

    fn draw_lyric(
        &self,
        mut doc: Document,
        lyric: &crate::models::Lyric,
        x: f32,
        _part_start_y: f32,
        staff: i32,
        _max_y: f32,
        _staff_dist: f32,
        active_lyrics: &mut HashMap<i32, LyricStartInfo>,
        baseline_y: f32,
    ) -> (Document, f32) {
        let y = baseline_y;

        if !lyric.text.is_empty() {
            // Recorded so the part below is pushed clear of the lyric line.
            let half_w = lyric.text.chars().count() as f32 * 3.25;
            self.push_gap_item(staff, true, x - half_w, x + half_w, y - 7.0, y + 7.0);
            doc = doc.add(
                Text::new(lyric.text.as_str())
                    .set("x", x)
                    .set("y", y)
                    .set("font-size", 12)
                    .set("text-anchor", "middle")
                    .set("dominant-baseline", "central")
                    .set("font-family", "serif"),
            );
        }

        let verse_num = lyric.number.unwrap_or(1);
        if let Some(ext_type) = &lyric.extend {
            if ext_type == "start" {
                active_lyrics.insert(
                    verse_num,
                    LyricStartInfo {
                        x: x + 10.0,
                        y,
                        is_continuation: false,
                    },
                );
            } else if ext_type == "stop" {
                if let Some(start) = active_lyrics.remove(&verse_num) {
                    let line = Line::new()
                        .set("x1", start.x)
                        .set("y1", start.y)
                        .set("x2", x - 10.0)
                        .set("y2", y)
                        .set("stroke", "black")
                        .set("stroke-width", 1);
                    doc = doc.add(line);
                }
            }
        }

        (doc, y + 15.0)
    }

    fn render_harmony(
        &self,
        mut doc: Document,
        harmony: &crate::models::Harmony,
        x: f32,
        y: f32,
    ) -> Document {
        if let Some(numeral) = &harmony.numeral {
            let root_text = numeral
                .root_text
                .clone()
                .unwrap_or_else(|| numeral.root_value.to_string());

            // Base Roman numeral
            let base_text = Text::new(root_text.as_str())
                .set("x", x)
                .set("y", y)
                .set("font-size", 14)
                .set("text-anchor", "middle")
                .set("dominant-baseline", "central")
                .set("font-family", self.font_family.as_str())
                .set("font-weight", "bold");
            doc = doc.add(base_text);

            let mut final_x_offset = root_text.len() as f32 * 4.5;

            // Render alteration if present
            if let Some(alt) = &numeral.root_alter {
                let alt_sym = self.format_alter(alt.value);
                let is_left = alt.location.as_deref() == Some("left");
                let alt_x = if is_left {
                    x - 14.0
                } else {
                    x + final_x_offset + 6.0
                };
                // Further increased size and shifted up
                let alt_text = Text::new(alt_sym.as_str())
                    .set("x", alt_x)
                    .set("y", y - 2.0) // Shifted up
                    .set("font-size", 18) // Further increased size
                    .set("text-anchor", "middle")
                    .set("dominant-baseline", "central")
                    .set("font-family", "serif")
                    .set("font-weight", "bold");
                doc = doc.add(alt_text);
                if !is_left {
                    final_x_offset += 12.0;
                }
            }

            // Superscript inversion number
            if let Some(inv) = harmony.inversion {
                // Common convention: 1st inversion of triad is '6'
                let inv_display = if inv == 1 {
                    "6".to_string()
                } else {
                    inv.to_string()
                };
                let inv_text = Text::new(inv_display)
                    .set("x", x + final_x_offset + 2.0)
                    .set("y", y - 5.0)
                    .set("font-size", 9)
                    .set("text-anchor", "start")
                    .set("dominant-baseline", "central")
                    .set("font-family", "serif")
                    .set("font-weight", "bold");
                doc = doc.add(inv_text);
            }
            return doc;
        }

        let mut text = format!(
            "{}{}",
            harmony.root_step,
            self.format_alter(harmony.root_alter.unwrap_or(0.0))
        );
        let kind_display = if harmony.use_symbols {
            match harmony.kind.as_str() {
                "major-seventh" => "\u{2206}", // Δ (Delta)
                "minor-seventh" => "-7",
                "augmented" => "+",
                "diminished" => "\u{006F}",      // o
                "half-diminished" => "\u{00F8}", // ø
                "dominant" => "7",
                _ => harmony.kind_text.as_deref().unwrap_or(""),
            }
        } else {
            match harmony.kind.as_str() {
                "major" => "",
                "minor" => "m",
                "augmented" => "aug",
                "diminished" => "dim",
                "dominant" => "7",
                "major-seventh" => "maj7",
                "minor-seventh" => "m7",
                "diminished-seventh" => "dim7",
                "half-diminished" => "m7b5",
                _ => harmony.kind_text.as_deref().unwrap_or(""),
            }
        };
        text.push_str(kind_display);

        if let Some(bass) = &harmony.bass_step {
            if let Some(sep) = &harmony.bass_separator {
                text.push_str(" ");
                text.push_str(sep);
                text.push_str(" ");
            } else {
                text.push_str("/");
            }
            text.push_str(bass);
            text.push_str(&self.format_alter(harmony.bass_alter.unwrap_or(0.0)));
        }

        for degree in &harmony.degrees {
            text.push_str("(");
            if let Some(t) = &degree.type_text {
                text.push_str(t);
            } else if degree.degree_type == "subtract" {
                text.push_str("no");
            }
            let alter_str = self.format_alter(degree.alter);
            text.push_str(&alter_str);
            text.push_str(&degree.value.to_string());
            text.push_str(")");
        }

        let text_el = Text::new(text)
            .set("x", x)
            .set("y", y)
            .set("font-size", 14)
            .set("text-anchor", "middle")
            .set("dominant-baseline", "central")
            .set("font-family", self.font_family.as_str())
            .set("font-weight", "bold");

        doc = doc.add(text_el);

        if let Some(frame) = &harmony.frame {
            // Draw frame BELOW the chord name
            doc = self.draw_frame(doc, frame, y + 25.0, x);
        }

        doc
    }

    fn arrow_to_smufl(&self, direction: &str) -> Option<&'static str> {
        match direction {
            "up" => Some("\u{EB60}"),
            "northeast" => Some("\u{EB61}"),
            "right" => Some("\u{EB62}"),
            "southeast" => Some("\u{EB63}"),
            "down" => Some("\u{EB64}"),
            "southwest" => Some("\u{EB65}"),
            "left" => Some("\u{EB66}"),
            "northwest" => Some("\u{EB67}"),
            _ => None,
        }
    }

    fn arrowhead_to_smufl(&self, direction: &str, filled: bool) -> Option<&'static str> {
        // SMuFL Arrowheads (U+EB70-U+EB7F)
        // 70: Up Filled, 71: Up Open
        // 72: Down Filled, 73: Down Open
        // 74: Left Filled, 75: Left Open
        // 76: Right Filled, 77: Right Open
        // 78: NE Filled, 79: NE Open
        // 7A: SE Filled, 7B: SE Open
        // 7C: SW Filled, 7D: SW Open
        // 7E: NW Filled, 7F: NW Open
        match (direction, filled) {
            ("up", true) => Some("\u{EB70}"),
            ("up", false) => Some("\u{EB71}"),
            ("down", true) => Some("\u{EB7C}"), // Requested U+EB7C for Down Filled
            ("down", false) => Some("\u{EB73}"),
            ("left", true) => Some("\u{EB74}"),
            ("left", false) => Some("\u{EB75}"),
            ("right", true) => Some("\u{EB76}"),
            ("right", false) => Some("\u{EB77}"),
            ("northeast", true) => Some("\u{EB78}"),
            ("northeast", false) => Some("\u{EB79}"),
            ("southeast", true) => Some("\u{EB7A}"),
            ("southeast", false) => Some("\u{EB7B}"),
            ("southwest", true) => Some("\u{EB72}"), // Swapped with previous Down
            ("southwest", false) => Some("\u{EB7D}"),
            ("northwest", true) => Some("\u{EB7E}"),
            ("northwest", false) => Some("\u{EB7F}"),
            _ => None,
        }
    }

    fn process_technical(
        &self,
        mut doc: Document,
        notes: &[&Note],
        x: f32,
        mut min_y: f32,
        mut max_y: f32,
        stem: Option<StemInfo>,
        staff_y_offset: f32,
    ) -> (Document, f32, f32) {
        let mut last_pedal_y: Option<f32> = None;
        for note in notes {
            for notation in &note.notations {
                if let crate::models::Notation::Technical(marks) = notation {
                    for mark in marks {
                        match mark {
                            crate::models::TechnicalMark::Arrow(arrow) => {
                                let smufl = if arrow.has_arrowhead {
                                    let is_filled = arrow.style.as_deref() != Some("open");
                                    self.arrowhead_to_smufl(&arrow.direction, is_filled)
                                } else {
                                    self.arrow_to_smufl(&arrow.direction)
                                };

                                if let Some(sym) = smufl {
                                    let is_above = if let Some(p) = arrow.placement.as_deref() {
                                        p == "above"
                                    } else {
                                        true
                                    };
                                    let mark_y = if is_above { min_y - 10.0 } else { max_y + 10.0 };
                                    doc = doc.add(
                                        Text::new(sym)
                                            .set("x", x)
                                            .set("y", mark_y)
                                            .set("font-size", self.staff_line_distance * 3.5)
                                            .set("text-anchor", "middle")
                                            .set("dominant-baseline", "central")
                                            .set("font-family", self.font_family.as_str()),
                                    );
                                    if is_above {
                                        min_y -= 15.0;
                                    } else {
                                        max_y += 15.0;
                                    }
                                }
                            }
                            crate::models::TechnicalMark::Harmonic(harmonic) => {
                                // 인공 하모닉스(artificial)의 경우 보통 다이아몬드 음표 머리로 표현하며,
                                // 상단에 동그라미 기호(, \u{E610})를 중복해서 표시하지 않는 것이 일반적입니다.
                                if !harmonic.is_artificial {
                                    let symbol = if harmonic.is_natural {
                                        "\u{E614}"
                                    } else {
                                        "\u{E610}"
                                    };
                                    let is_above = true;
                                    let mark_y = if is_above { min_y - 10.0 } else { max_y + 10.0 };
                                    doc = doc.add(
                                        Text::new(symbol)
                                            .set("x", x)
                                            .set("y", mark_y)
                                            .set("font-size", self.staff_line_distance * 3.5)
                                            .set("text-anchor", "middle")
                                            .set("dominant-baseline", "central")
                                            .set("font-family", self.font_family.as_str()),
                                    );
                                    if is_above {
                                        min_y -= 15.0;
                                    } else {
                                        max_y += 15.0;
                                    }
                                }
                            }
                            crate::models::TechnicalMark::BrassBend { placement } => {
                                let is_above = if let Some(p) = placement.as_deref() {
                                    p == "above"
                                } else {
                                    true
                                };
                                let mark_y = if is_above { min_y - 10.0 } else { max_y + 10.0 };
                                doc = doc.add(
                                    Text::new("\u{E5E3}")
                                        .set("x", x)
                                        .set("y", mark_y)
                                        .set("font-size", self.staff_line_distance * 3.5)
                                        .set("text-anchor", "middle")
                                        .set("dominant-baseline", "central")
                                        .set("font-family", self.font_family.as_str()),
                                );
                                if is_above {
                                    min_y -= 15.0;
                                } else {
                                    max_y += 15.0;
                                }
                            }
                            crate::models::TechnicalMark::DoubleTongue { placement } => {
                                let is_above = if let Some(p) = placement.as_deref() {
                                    p == "above"
                                } else {
                                    true
                                };
                                let mark_y = if is_above { min_y - 8.0 } else { max_y + 8.0 };
                                doc = doc.add(
                                    Text::new("\u{E5F0}")
                                        .set("x", x)
                                        .set("y", mark_y)
                                        .set("font-size", self.staff_line_distance * 3.0)
                                        .set("text-anchor", "middle")
                                        .set("dominant-baseline", "central")
                                        .set("font-family", self.font_family.as_str()),
                                );
                                if is_above {
                                    min_y -= 12.0;
                                } else {
                                    max_y += 12.0;
                                }
                            }
                            crate::models::TechnicalMark::TripleTongue { placement } => {
                                let is_above = if let Some(p) = placement.as_deref() {
                                    p == "above"
                                } else {
                                    true
                                };
                                let mark_y = if is_above { min_y - 8.0 } else { max_y + 8.0 };
                                doc = doc.add(
                                    Text::new("\u{E5F2}")
                                        .set("x", x)
                                        .set("y", mark_y)
                                        .set("font-size", self.staff_line_distance * 3.0)
                                        .set("text-anchor", "middle")
                                        .set("dominant-baseline", "central")
                                        .set("font-family", self.font_family.as_str()),
                                );
                                if is_above {
                                    min_y -= 12.0;
                                } else {
                                    max_y += 12.0;
                                }
                            }
                            crate::models::TechnicalMark::DownBow { placement } => {
                                let is_above = if let Some(p) = placement.as_deref() {
                                    p == "above"
                                } else {
                                    true
                                };
                                let mark_y = if is_above { min_y - 10.0 } else { max_y + 10.0 };
                                doc = doc.add(
                                    Text::new("\u{E610}")
                                        .set("x", x)
                                        .set("y", mark_y)
                                        .set("font-size", self.staff_line_distance * 3.5)
                                        .set("text-anchor", "middle")
                                        .set("dominant-baseline", "central")
                                        .set("font-family", self.font_family.as_str()),
                                );
                                if is_above {
                                    min_y -= 15.0;
                                } else {
                                    max_y += 15.0;
                                }
                            }
                            crate::models::TechnicalMark::UpBow { placement } => {
                                let is_above = if let Some(p) = placement.as_deref() {
                                    p == "above"
                                } else {
                                    true
                                };
                                let mark_y = if is_above { min_y - 10.0 } else { max_y + 10.0 };
                                doc = doc.add(
                                    Text::new("\u{E612}")
                                        .set("x", x)
                                        .set("y", mark_y)
                                        .set("font-size", self.staff_line_distance * 3.5)
                                        .set("text-anchor", "middle")
                                        .set("dominant-baseline", "central")
                                        .set("font-family", self.font_family.as_str()),
                                );
                                if is_above {
                                    min_y -= 15.0;
                                } else {
                                    max_y += 15.0;
                                }
                            }
                            crate::models::TechnicalMark::Fingering { text, placement } => {
                                let is_above = if let Some(p) = placement.as_deref() {
                                    p == "above"
                                } else {
                                    true
                                };
                                let mark_y = if is_above { min_y - 8.0 } else { max_y + 8.0 };
                                let node = Text::new(text.as_str())
                                    .set("x", x)
                                    .set("font-size", 10)
                                    .set("text-anchor", "middle")
                                    .set("dominant-baseline", "central")
                                    .set("font-family", "serif")
                                    .set("font-weight", "bold");
                                // On the beam side of a beamed note the beam isn't
                                // drawn yet, and may end up past this stem's tip.
                                match stem.filter(|s| !note.beams.is_empty() && s.is_up == is_above) {
                                    Some(s) => {
                                        let mut marks = self.pending_beam_marks.borrow_mut();
                                        let stacked = marks
                                            .iter()
                                            .filter(|m| (m.stem_x - s.x).abs() < 0.5)
                                            .count() as f32;
                                        marks.push(PendingBeamMark {
                                            x,
                                            stem_x: s.x,
                                            is_above,
                                            gap: 8.0 + stacked * 12.0,
                                            fallback_y: mark_y,
                                            node,
                                        });
                                    }
                                    None => {
                                        self.placed_fingerings.borrow_mut().push((
                                            self.current_part_y.get(),
                                            x,
                                            mark_y,
                                        ));
                                        doc = doc.add(node.set("y", mark_y));
                                    }
                                }
                                if is_above {
                                    min_y -= 12.0;
                                } else {
                                    max_y += 12.0;
                                }
                            }
                            crate::models::TechnicalMark::Fingernails { placement } => {
                                let is_above = if let Some(p) = placement.as_deref() {
                                    p == "above"
                                } else {
                                    true
                                };
                                let mark_y = if is_above { min_y - 10.0 } else { max_y + 10.0 };
                                doc = doc.add(
                                    Text::new("\u{E636}")
                                        .set("x", x)
                                        .set("y", mark_y)
                                        .set("font-size", self.staff_line_distance * 3.5)
                                        .set("text-anchor", "middle")
                                        .set("dominant-baseline", "central")
                                        .set("font-family", self.font_family.as_str()),
                                );
                                if is_above {
                                    min_y -= 15.0;
                                } else {
                                    max_y += 15.0;
                                }
                            }
                            crate::models::TechnicalMark::Flip { placement } => {
                                let is_above = if let Some(p) = placement.as_deref() {
                                    p == "above"
                                } else {
                                    true
                                };
                                let mark_y = if is_above { min_y - 10.0 } else { max_y + 10.0 };
                                doc = doc.add(
                                    Text::new("\u{E5E1}")
                                        .set("x", x)
                                        .set("y", mark_y)
                                        .set("font-size", self.staff_line_distance * 3.5)
                                        .set("text-anchor", "middle")
                                        .set("dominant-baseline", "central")
                                        .set("font-family", self.font_family.as_str()),
                                );
                                if is_above {
                                    min_y -= 15.0;
                                } else {
                                    max_y += 15.0;
                                }
                            }
                            crate::models::TechnicalMark::Golpe { placement } => {
                                let is_above = if let Some(p) = placement.as_deref() {
                                    p == "above"
                                } else {
                                    true
                                };
                                let mark_y = if is_above { min_y - 10.0 } else { max_y + 10.0 };
                                doc = doc.add(
                                    Text::new("\u{E842}")
                                        .set("x", x)
                                        .set("y", mark_y)
                                        .set("font-size", self.staff_line_distance * 3.5)
                                        .set("text-anchor", "middle")
                                        .set("dominant-baseline", "central")
                                        .set("font-family", self.font_family.as_str()),
                                );
                                if is_above {
                                    min_y -= 15.0;
                                } else {
                                    max_y += 15.0;
                                }
                            }
                            crate::models::TechnicalMark::HalfMuted { placement } => {
                                let is_above = if let Some(p) = placement.as_deref() {
                                    p == "above"
                                } else {
                                    true
                                };
                                let mark_y = if is_above { min_y - 10.0 } else { max_y + 10.0 };
                                doc = doc.add(
                                    Text::new("\u{E5E6}")
                                        .set("x", x)
                                        .set("y", mark_y)
                                        .set("font-size", self.staff_line_distance * 3.0)
                                        .set("text-anchor", "middle")
                                        .set("dominant-baseline", "central")
                                        .set("font-family", self.font_family.as_str()),
                                );
                                if is_above {
                                    min_y -= 15.0;
                                } else {
                                    max_y += 15.0;
                                }
                            }
                            crate::models::TechnicalMark::Handbell { value, placement } => {
                                let is_above = if let Some(p) = placement.as_deref() {
                                    p == "above"
                                } else {
                                    true
                                };
                                let mark_y = if is_above { min_y - 10.0 } else { max_y + 10.0 };
                                let smufl = match value.as_str() {
                                    "martellato" => "\u{E810}",
                                    "martellato-lift" => "\u{E821}",
                                    "hand-mute" => "\u{E822}",
                                    "hum" => "\u{E823}",
                                    "echo" => "\u{E824}",
                                    "gyro" => "\u{E825}",
                                    "shake" => "\u{E826}",
                                    "mallet-lift" => "\u{E827}",
                                    "mallet-on-table" => "\u{E828}",
                                    "pluck" => "\u{E829}",
                                    _ => "",
                                };
                                if !smufl.is_empty() {
                                    doc = doc.add(
                                        Text::new(smufl)
                                            .set("x", x)
                                            .set("y", mark_y)
                                            .set("font-size", self.staff_line_distance * 3.5)
                                            .set("text-anchor", "middle")
                                            .set("dominant-baseline", "central")
                                            .set("font-family", self.font_family.as_str()),
                                    );
                                    if is_above {
                                        min_y -= 15.0;
                                    } else {
                                        max_y += 15.0;
                                    }
                                }
                            }
                            crate::models::TechnicalMark::HarmonMute { closed, placement } => {
                                let is_above = if let Some(p) = placement.as_deref() {
                                    p == "above"
                                } else {
                                    true
                                };
                                let mark_y = if is_above { min_y - 10.0 } else { max_y + 10.0 };
                                let smufl = match closed.as_deref() {
                                    Some("yes") => "\u{E5E8}",
                                    Some("no") => "\u{E5E7}",
                                    Some("half") => "\u{E5E9}",
                                    _ => "\u{E5E8}",
                                };
                                doc = doc.add(
                                    Text::new(smufl)
                                        .set("x", x)
                                        .set("y", mark_y)
                                        .set("font-size", self.staff_line_distance * 3.5)
                                        .set("text-anchor", "middle")
                                        .set("dominant-baseline", "central")
                                        .set("font-family", self.font_family.as_str()),
                                );
                                if is_above {
                                    min_y -= 15.0;
                                } else {
                                    max_y += 15.0;
                                }
                            }
                            crate::models::TechnicalMark::Heel {
                                placement,
                                substitution,
                            }
                            | crate::models::TechnicalMark::Toe {
                                placement,
                                substitution,
                            } => {
                                let is_above = if let Some(p) = placement.as_deref() {
                                    p == "above"
                                } else {
                                    true
                                };
                                let is_heel =
                                    matches!(mark, crate::models::TechnicalMark::Heel { .. });
                                let symbol = if is_heel { "\u{E661}" } else { "\u{E665}" };

                                let is_sub = *substitution == Some(true);
                                if is_sub && last_pedal_y.is_some() {
                                    let py = last_pedal_y.unwrap();
                                    // Draw ONLY the substitution curve between previous X and current X
                                    let sub_sym = if is_above { "\u{E674}" } else { "\u{E675}" };
                                    doc = doc.add(
                                        Text::new(sub_sym)
                                            .set("x", x + 10.0)
                                            .set("y", py)
                                            .set("font-size", self.staff_line_distance * 3.5)
                                            .set("text-anchor", "middle")
                                            .set("dominant-baseline", "central")
                                            .set("font-family", self.font_family.as_str()),
                                    );
                                } else {
                                    let mark_y = if is_above { min_y - 10.0 } else { max_y + 10.0 };
                                    last_pedal_y = Some(mark_y);

                                    doc = doc.add(
                                        Text::new(symbol)
                                            .set("x", x)
                                            .set("y", mark_y)
                                            .set("font-size", self.staff_line_distance * 3.5)
                                            .set("text-anchor", "middle")
                                            .set("dominant-baseline", "central")
                                            .set("font-family", self.font_family.as_str()),
                                    );

                                    if is_above {
                                        min_y -= 15.0;
                                    } else {
                                        max_y += 15.0;
                                    }
                                }
                            }
                            crate::models::TechnicalMark::Hole { content, placement } => {
                                let is_above = if let Some(p) = placement.as_deref() {
                                    p == "above"
                                } else {
                                    true
                                };
                                let mark_y = if is_above { min_y - 10.0 } else { max_y + 10.0 };
                                let smufl = match content.as_str() {
                                    "open" => "\u{E5F9}",
                                    "closed" => "\u{E5F4}",
                                    "half" => "\u{E5F8}",
                                    _ => "\u{E5F9}",
                                };
                                doc = doc.add(
                                    Text::new(smufl)
                                        .set("x", x)
                                        .set("y", mark_y)
                                        .set("font-size", self.staff_line_distance * 3.5)
                                        .set("text-anchor", "middle")
                                        .set("dominant-baseline", "central")
                                        .set("font-family", self.font_family.as_str()),
                                );
                                if is_above {
                                    min_y -= 15.0;
                                } else {
                                    max_y += 15.0;
                                }
                            }
                            crate::models::TechnicalMark::Open { placement } => {
                                let is_above = if let Some(p) = placement.as_deref() {
                                    p == "above"
                                } else {
                                    true
                                };
                                let mark_y = if is_above { min_y - 10.0 } else { max_y + 10.0 };
                                doc = doc.add(
                                    Text::new("\u{E614}")
                                        .set("x", x)
                                        .set("y", mark_y)
                                        .set("font-size", self.staff_line_distance * 3.5)
                                        .set("text-anchor", "middle")
                                        .set("dominant-baseline", "central")
                                        .set("font-family", self.font_family.as_str()),
                                );
                                if is_above {
                                    min_y -= 15.0;
                                } else {
                                    max_y += 15.0;
                                }
                            }
                            crate::models::TechnicalMark::OpenString { placement } => {
                                let is_above = if let Some(p) = placement.as_deref() {
                                    p == "above"
                                } else {
                                    true
                                };
                                let mark_y = if is_above { min_y - 10.0 } else { max_y + 10.0 };
                                doc = doc.add(
                                    Text::new("\u{E614}")
                                        .set("x", x)
                                        .set("y", mark_y)
                                        .set("font-size", self.staff_line_distance * 3.5)
                                        .set("text-anchor", "middle")
                                        .set("dominant-baseline", "central")
                                        .set("font-family", self.font_family.as_str()),
                                );
                                if is_above {
                                    min_y -= 15.0;
                                } else {
                                    max_y += 15.0;
                                }
                            }
                            crate::models::TechnicalMark::Pluck {
                                text,
                                placement,
                                default_x,
                                default_y,
                            } => {
                                let is_above = if let Some(p) = placement.as_deref() {
                                    p == "above"
                                } else {
                                    true
                                };
                                let mut mark_x = x;
                                let mut mark_y = if is_above { min_y - 10.0 } else { max_y + 10.0 };

                                if let Some(dx) = default_x {
                                    mark_x = x + (dx - 10.0); // Rough offset
                                }
                                if let Some(dy) = default_y {
                                    // tenths to px mapping
                                    mark_y =
                                        staff_y_offset - dy * (self.staff_line_distance / 10.0);
                                }

                                doc = doc.add(
                                    Text::new(text.as_str())
                                        .set("x", mark_x)
                                        .set("y", mark_y)
                                        .set("font-size", 12)
                                        .set("text-anchor", "middle")
                                        .set("dominant-baseline", "central")
                                        .set("font-family", "serif")
                                        .set("font-style", "italic"),
                                );
                                if is_above {
                                    min_y = min_y.min(mark_y - 10.0);
                                } else {
                                    max_y = max_y.max(mark_y + 10.0);
                                }
                            }
                            crate::models::TechnicalMark::Smear { placement } => {
                                let is_above = if let Some(p) = placement.as_deref() {
                                    p == "above"
                                } else {
                                    true
                                };
                                let mark_y = if is_above { min_y - 2.0 } else { max_y + 2.0 };
                                doc = doc.add(
                                    Text::new("\u{E5E2}")
                                        .set("x", x)
                                        .set("y", mark_y)
                                        .set("font-size", self.staff_line_distance * 3.5)
                                        .set("text-anchor", "middle")
                                        .set("dominant-baseline", "central")
                                        .set("font-family", self.font_family.as_str()),
                                );
                                if is_above {
                                    min_y -= 15.0;
                                } else {
                                    max_y += 15.0;
                                }
                            }
                            crate::models::TechnicalMark::SnapPizzicato { placement } => {
                                let is_above = if let Some(p) = placement.as_deref() {
                                    p == "above"
                                } else {
                                    true
                                };
                                let symbol = if is_above { "\u{E631}" } else { "\u{E630}" };
                                let mark_y = if is_above { min_y - 10.0 } else { max_y + 10.0 };
                                doc = doc.add(
                                    Text::new(symbol)
                                        .set("x", x)
                                        .set("y", mark_y)
                                        .set("font-size", self.staff_line_distance * 3.5)
                                        .set("text-anchor", "middle")
                                        .set("dominant-baseline", "central")
                                        .set("font-family", self.font_family.as_str()),
                                );
                                if is_above {
                                    min_y -= 15.0;
                                } else {
                                    max_y += 15.0;
                                }
                            }
                            crate::models::TechnicalMark::Tap { hand, placement } => {
                                let is_above = if let Some(p) = placement.as_deref() {
                                    p == "above"
                                } else {
                                    true
                                };
                                let symbol = match hand.as_deref() {
                                    Some("right") => "\u{E841}", // guitarRightHandTap
                                    Some("left") => "\u{E840}",  // guitarLeftHandTap
                                    _ => "\u{E841}",
                                };
                                let mark_y = if is_above { min_y - 10.0 } else { max_y + 10.0 };
                                doc = doc.add(
                                    Text::new(symbol)
                                        .set("x", x)
                                        .set("y", mark_y)
                                        .set("font-size", self.staff_line_distance * 3.5)
                                        .set("text-anchor", "middle")
                                        .set("dominant-baseline", "central")
                                        .set("font-family", self.font_family.as_str()),
                                );
                                if is_above {
                                    min_y -= 15.0;
                                } else {
                                    max_y += 15.0;
                                }
                            }
                            crate::models::TechnicalMark::ThumbPosition { placement } => {
                                let is_above = if let Some(p) = placement.as_deref() {
                                    p == "above"
                                } else {
                                    true
                                };
                                let mark_y = if is_above { min_y - 10.0 } else { max_y + 10.0 };
                                doc = doc.add(
                                    Text::new("\u{E624}")
                                        .set("x", x)
                                        .set("y", mark_y)
                                        .set("font-size", self.staff_line_distance * 3.5)
                                        .set("text-anchor", "middle")
                                        .set("dominant-baseline", "central")
                                        .set("font-family", self.font_family.as_str()),
                                );
                                if is_above {
                                    min_y -= 15.0;
                                } else {
                                    max_y += 15.0;
                                }
                            }
                            crate::models::TechnicalMark::Stopped { placement } => {
                                let is_above = if let Some(p) = placement.as_deref() {
                                    p == "above"
                                } else {
                                    true
                                };
                                let mark_y = if is_above { min_y - 8.0 } else { max_y + 8.0 };
                                doc = doc.add(
                                    Text::new("\u{E5F4}")
                                        .set("x", x)
                                        .set("y", mark_y)
                                        .set("font-size", self.staff_line_distance * 3.0)
                                        .set("text-anchor", "middle")
                                        .set("dominant-baseline", "central")
                                        .set("font-family", self.font_family.as_str()),
                                );
                                if is_above {
                                    min_y -= 12.0;
                                } else {
                                    max_y += 12.0;
                                }
                            }
                            crate::models::TechnicalMark::Bend(bends) => {
                                let mut current_x_offset = 0.0;
                                for bend in bends {
                                    let is_above = if let Some(p) = bend.placement.as_deref() {
                                        p == "above"
                                    } else {
                                        true
                                    };
                                    let mark_y = if is_above { min_y - 10.0 } else { max_y + 10.0 };

                                    if let Some(wb) = &bend.with_bar {
                                        let symbol = if wb.value.to_lowercase() == "dip" {
                                            "\u{E831}"
                                        } else {
                                            wb.value.as_str()
                                        };
                                        let font = if wb.value.to_lowercase() == "dip" {
                                            self.font_family.as_str()
                                        } else {
                                            "serif"
                                        };
                                        let font_size = if wb.value.to_lowercase() == "dip" {
                                            self.staff_line_distance * 3.0
                                        } else {
                                            10.0
                                        };

                                        doc = doc.add(
                                            Text::new(symbol)
                                                .set("x", x + current_x_offset)
                                                .set("y", mark_y)
                                                .set("font-size", font_size)
                                                .set("text-anchor", "middle")
                                                .set("dominant-baseline", "central")
                                                .set("font-family", font)
                                                .set("font-style", "italic"),
                                        );
                                        current_x_offset += 20.0;
                                    } else {
                                        let abs_alter = bend.bend_alter.abs();
                                        let bend_text = if abs_alter >= 2.0 {
                                            "full"
                                        } else if abs_alter >= 1.0 {
                                            "1/2"
                                        } else if abs_alter > 0.0 {
                                            "1/4"
                                        } else {
                                            ""
                                        };

                                        if bend.pre_bend {
                                            let line_data = Data::new()
                                                .move_to((x + current_x_offset, mark_y))
                                                .line_to((x + current_x_offset, mark_y - 15.0));
                                            doc = doc.add(
                                                Path::new()
                                                    .set("fill", "none")
                                                    .set("stroke", "black")
                                                    .set("stroke-width", 1.0)
                                                    .set("d", line_data),
                                            );
                                            let arrow_data = Data::new()
                                                .move_to((
                                                    x + current_x_offset - 3.0,
                                                    mark_y - 12.0,
                                                ))
                                                .line_to((x + current_x_offset, mark_y - 15.0))
                                                .line_to((
                                                    x + current_x_offset + 3.0,
                                                    mark_y - 12.0,
                                                ));
                                            doc = doc.add(
                                                Path::new()
                                                    .set("fill", "none")
                                                    .set("stroke", "black")
                                                    .set("stroke-width", 1.0)
                                                    .set("d", arrow_data),
                                            );
                                            if !bend_text.is_empty() {
                                                doc = doc.add(
                                                    Text::new(bend_text)
                                                        .set("x", x + current_x_offset)
                                                        .set("y", mark_y - 20.0)
                                                        .set("font-size", 9)
                                                        .set("text-anchor", "middle")
                                                        .set("font-family", "serif")
                                                        .set("font-style", "italic"),
                                                );
                                            }
                                            current_x_offset += 15.0;
                                        } else if bend.release {
                                            // Release arrow (curved down from top to fret level) - lowered 5px
                                            let curve_data = Data::new()
                                                .move_to((x + current_x_offset, mark_y - 15.0))
                                                .quadratic_curve_to((
                                                    x + current_x_offset + 10.0,
                                                    mark_y - 15.0,
                                                    x + current_x_offset + 10.0,
                                                    mark_y + 5.0,
                                                ));
                                            doc = doc.add(
                                                Path::new()
                                                    .set("fill", "none")
                                                    .set("stroke", "black")
                                                    .set("stroke-width", 1.0)
                                                    .set("d", curve_data),
                                            );
                                            // Downward arrowhead - lowered 5px
                                            let arrow_data = Data::new()
                                                .move_to((x + current_x_offset + 7.0, mark_y + 2.0))
                                                .line_to((
                                                    x + current_x_offset + 10.0,
                                                    mark_y + 5.0,
                                                ))
                                                .line_to((
                                                    x + current_x_offset + 13.0,
                                                    mark_y + 2.0,
                                                ));
                                            doc = doc.add(
                                                Path::new()
                                                    .set("fill", "none")
                                                    .set("stroke", "black")
                                                    .set("stroke-width", 1.0)
                                                    .set("d", arrow_data),
                                            );

                                            // Do NOT render bend_text for release to avoid duplication with the preceding bend
                                            current_x_offset += 15.0;
                                        } else {
                                            // Normal bend - lowered 5px
                                            let curve_data = Data::new()
                                                .move_to((
                                                    x + current_x_offset + 10.0,
                                                    mark_y + 10.0,
                                                ))
                                                .quadratic_curve_to((
                                                    x + current_x_offset + 20.0,
                                                    mark_y + 10.0,
                                                    x + current_x_offset + 20.0,
                                                    mark_y - 5.0,
                                                ));
                                            doc = doc.add(
                                                Path::new()
                                                    .set("fill", "none")
                                                    .set("stroke", "black")
                                                    .set("stroke-width", 1.0)
                                                    .set("d", curve_data),
                                            );
                                            let arrow_data = Data::new()
                                                .move_to((
                                                    x + current_x_offset + 17.0,
                                                    mark_y - 2.0,
                                                ))
                                                .line_to((
                                                    x + current_x_offset + 20.0,
                                                    mark_y - 5.0,
                                                ))
                                                .line_to((
                                                    x + current_x_offset + 23.0,
                                                    mark_y - 2.0,
                                                ));
                                            doc = doc.add(
                                                Path::new()
                                                    .set("fill", "none")
                                                    .set("stroke", "black")
                                                    .set("stroke-width", 1.0)
                                                    .set("d", arrow_data),
                                            );
                                            if !bend_text.is_empty() {
                                                doc = doc.add(
                                                    Text::new(bend_text)
                                                        .set("x", x + current_x_offset + 20.0)
                                                        .set("y", mark_y - 10.0)
                                                        .set("font-size", 9)
                                                        .set("text-anchor", "middle")
                                                        .set("font-family", "serif")
                                                        .set("font-style", "italic"),
                                                );
                                            }
                                            current_x_offset += 25.0;
                                        }
                                    }
                                }
                                let is_above = if let Some(first) = bends.first() {
                                    if let Some(p) = first.placement.as_deref() {
                                        p == "above"
                                    } else {
                                        true
                                    }
                                } else {
                                    true
                                };
                                if is_above {
                                    min_y -= 35.0;
                                } else {
                                    max_y += 35.0;
                                }
                            }
                            crate::models::TechnicalMark::OtherTechnical { text, placement } => {
                                let is_above = if let Some(p) = placement.as_deref() {
                                    p == "above"
                                } else {
                                    true
                                };
                                let mark_y = if is_above { min_y - 10.0 } else { max_y + 10.0 };
                                doc = doc.add(
                                    Text::new(text.as_str())
                                        .set("x", x)
                                        .set("y", mark_y)
                                        .set("font-size", 10)
                                        .set("text-anchor", "middle")
                                        .set("dominant-baseline", "central")
                                        .set("font-family", "serif")
                                        .set("font-style", "italic"),
                                );
                                if is_above {
                                    min_y -= 15.0;
                                } else {
                                    max_y += 15.0;
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        (doc, min_y, max_y)
    }

    fn process_arpeggios(
        &self,
        mut doc: Document,
        chord_group: &[&Note],
        x: f32,
        pitches_y: &[f32],
    ) -> Document {
        if let Some(first_note) = chord_group.first() {
            if first_note
                .notations
                .iter()
                .any(|n| matches!(n, crate::models::Notation::Arpeggiate { .. }))
            {
                if !pitches_y.is_empty() {
                    let mut sorted = pitches_y.to_vec();
                    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
                    let top_y = sorted[0] - self.staff_line_distance * 1.5;
                    let bottom_y = *sorted.last().unwrap();

                    // Shift arpeggio further left if any accidental is present in the chord group
                    let has_accidental = chord_group.iter().any(|n| {
                        n.accidental.is_some()
                            || n.pitch.as_ref().map_or(false, |p| {
                                p.alter.is_some() && p.alter.unwrap().abs() > 0.01
                            })
                    });
                    let arpeggio_offset = if has_accidental { -28.0 } else { -15.0 };

                    doc = self.draw_arpeggio(doc, x + arpeggio_offset, top_y, bottom_y);
                }
            }
        }
        doc
    }

    fn draw_arpeggio(&self, mut doc: Document, x: f32, top_y: f32, bottom_y: f32) -> Document {
        // Use a single arpeggiato symbol and scale it vertically
        // U+E63C is the standard arpeggiato wiggle
        let height = bottom_y - top_y;
        if height <= 0.0 {
            return doc;
        }

        let base_font_size = self.staff_line_distance * 4.0;
        let scale_y = height / (base_font_size * 1.25); // Adjusted scale to be 2.5x smaller than previous ~0.5 factor

        let text = Text::new("\u{E63C}")
            .set("x", x)
            .set("y", top_y)
            .set("font-size", base_font_size)
            .set("text-anchor", "middle")
            .set("dominant-baseline", "hanging")
            .set("font-family", self.font_family.as_str())
            // Stretch about (x, top_y) within the transform itself:
            // `transform-origin` isn't honoured by every SVG renderer
            // (librsvg, Inkscape), which then scale about the page origin
            // and drop the wiggle far down the page.
            .set(
                "transform",
                format!(
                    "translate(0, {}) scale(1, {}) translate(0, {})",
                    top_y,
                    scale_y,
                    -top_y
                ),
            );

        doc = doc.add(text);
        doc
    }

    fn draw_non_arpeggio(&self, mut doc: Document, x: f32, top_y: f32, bottom_y: f32) -> Document {
        // Draw a bracket to indicate notes should be played together (non-arpeggiated)
        let bracket_data = Data::new()
            .move_to((x + 5.0, top_y))
            .line_to((x, top_y))
            .line_to((x, bottom_y))
            .line_to((x + 5.0, bottom_y));
        let bracket = Path::new()
            .set("fill", "none")
            .set("stroke", "black")
            .set("stroke-width", 1.2)
            .set("d", bracket_data);
        doc = doc.add(bracket);
        doc
    }

    fn accidental_to_smufl(&self, value: &str) -> Option<&'static str> {
        match value {
            "sharp" => Some("\u{E262}"),
            "flat" => Some("\u{E260}"),
            "natural" => Some("\u{E261}"),
            "double-sharp" => Some("\u{E263}"),
            "flat-flat" => Some("\u{E264}"),
            _ => None,
        }
    }

    fn render_accidental_mark(
        &self,
        mut doc: Document,
        mark: &crate::models::AccidentalMark,
        x: f32,
        mut min_y: f32,
        mut max_y: f32,
        _stem: Option<StemInfo>,
        is_small: bool,
    ) -> (Document, f32, f32) {
        if let Some(smufl) = self.accidental_to_smufl(&mark.value) {
            let is_above = if let Some(p) = mark.placement.as_deref() {
                p == "above"
            } else {
                true
            };
            let mark_y = if is_above { min_y - 8.0 } else { max_y + 8.0 };
            let font_size = if is_small {
                self.staff_line_distance * 2.0
            } else {
                self.staff_line_distance * 3.0
            };
            doc = doc.add(
                Text::new(smufl)
                    .set("x", x)
                    .set("y", mark_y)
                    .set("font-size", font_size)
                    .set("text-anchor", "middle")
                    .set("dominant-baseline", "central")
                    .set("font-family", self.font_family.as_str()),
            );
            if is_above {
                min_y -= 12.0;
            } else {
                max_y += 12.0;
            }
        }
        (doc, min_y, max_y)
    }

    fn process_accidental_marks(
        &self,
        mut doc: Document,
        notes: &[&Note],
        x: f32,
        mut min_y: f32,
        mut max_y: f32,
        stem: Option<StemInfo>,
    ) -> (Document, f32, f32) {
        for note in notes {
            for notation in &note.notations {
                if let crate::models::Notation::AccidentalMark(mark) = notation {
                    let (new_doc, new_min, new_max) =
                        self.render_accidental_mark(doc, mark, x, min_y, max_y, stem, false);
                    doc = new_doc;
                    min_y = new_min;
                    max_y = new_max;
                }
            }
        }
        (doc, min_y, max_y)
    }

    fn process_ornaments(
        &self,
        mut doc: Document,
        notes: &[&Note],
        x: f32,
        mut min_y: f32,
        mut max_y: f32,
        stem: Option<StemInfo>,
        active_tremolos: &mut HashMap<i32, TremoloStartInfo>,
        active_wavy_lines: &mut HashMap<i32, WavyLineStartInfo>,
        staff_y_offset: f32,
    ) -> (Document, f32, f32) {
        for note in notes {
            for notation in &note.notations {
                if let crate::models::Notation::Ornaments(ornaments) = notation {
                    for ornament in ornaments {
                        match ornament {
                            crate::models::Ornament::TrillMark => {
                                let is_above = true;
                                // Force trill-mark to be at least above the staff
                                let mark_y = if is_above {
                                    (min_y - 12.0).min(staff_y_offset - 12.0)
                                } else {
                                    (max_y + 12.0)
                                        .max(staff_y_offset + 4.0 * self.staff_line_distance + 12.0)
                                };
                                doc = doc.add(
                                    Text::new("\u{E566}")
                                        .set("x", x)
                                        .set("y", mark_y)
                                        .set("font-size", self.staff_line_distance * 3.5)
                                        .set("text-anchor", "middle")
                                        .set("dominant-baseline", "central")
                                        .set("font-family", self.font_family.as_str()),
                                );
                                if is_above {
                                    min_y = mark_y - 10.0;
                                } else {
                                    max_y = mark_y + 10.0;
                                }
                            }
                            crate::models::Ornament::WavyLine {
                                wavy_type,
                                number,
                                relative_x,
                            } => {
                                // Trill wavy lines are placed clearly above the staff
                                // Align with trill-mark's vertical center if present, then move up slightly
                                let mut cur_y = min_y + 10.0 - 0.5 * self.staff_line_distance;
                                // Force wavy line to be at least above the staff
                                cur_y = cur_y.min(staff_y_offset - 15.0);

                                if wavy_type == "start" {
                                    // Check if there's a trill-mark on any note in this chord group to adjust start X
                                    let has_trill = notes.iter().any(|n| {
                                        n.notations.iter().any(|not| match not {
                                            crate::models::Notation::Ornaments(orns) => {
                                                orns.iter().any(|o| {
                                                    matches!(o, crate::models::Ornament::TrillMark)
                                                })
                                            }
                                            _ => false,
                                        })
                                    });
                                    let start_x = if has_trill { x + 12.0 } else { x };
                                    active_wavy_lines.insert(
                                        *number,
                                        WavyLineStartInfo {
                                            x: start_x,
                                            y: cur_y,
                                            is_continuation: false,
                                            _number: *number,
                                        },
                                    );
                                } else if wavy_type == "stop" {
                                    if let Some(start_info) = active_wavy_lines.remove(number) {
                                        let end_x = if let Some(rx) = relative_x {
                                            x + rx
                                        } else {
                                            x + 20.0
                                        };
                                        // User requested: wavy-line must always be horizontal.
                                        doc = self.draw_wavy_line(
                                            doc,
                                            start_info.x,
                                            start_info.y,
                                            end_x,
                                            start_info.y,
                                        );
                                    }
                                }
                            }
                            crate::models::Ornament::Schleifer { placement } => {
                                let is_above = if let Some(p) = placement.as_deref() {
                                    p == "above"
                                } else {
                                    true
                                };
                                let mark_y = if is_above { min_y - 30.0 } else { max_y - 10.0 };
                                doc = doc.add(
                                    Text::new("\u{E587}")
                                        .set("x", x - 35.0) // Moved even more to the left
                                        .set("y", mark_y)
                                        .set("font-size", self.staff_line_distance * 3.5)
                                        .set("text-anchor", "middle")
                                        .set("dominant-baseline", "central")
                                        .set("font-family", self.font_family.as_str()),
                                );
                                if is_above {
                                    min_y -= 35.0;
                                } else {
                                    max_y += 5.0;
                                }
                            }
                            crate::models::Ornament::Shake { placement } => {
                                let is_above = if let Some(p) = placement.as_deref() {
                                    p == "above"
                                } else {
                                    true
                                };
                                let mark_y = if is_above { min_y - 10.0 } else { max_y + 10.0 };
                                doc = doc.add(
                                    Text::new("\u{E56E}")
                                        .set("x", x)
                                        .set("y", mark_y)
                                        .set("font-size", self.staff_line_distance * 3.5)
                                        .set("text-anchor", "middle")
                                        .set("dominant-baseline", "central")
                                        .set("font-family", self.font_family.as_str()),
                                );
                                if is_above {
                                    min_y -= 20.0;
                                } else {
                                    max_y += 20.0;
                                }
                            }
                            crate::models::Ornament::Turn
                            | crate::models::Ornament::DelayedTurn
                            | crate::models::Ornament::InvertedTurn
                            | crate::models::Ornament::DelayedInvertedTurn
                            | crate::models::Ornament::Haydn
                            | crate::models::Ornament::VerticalTurn
                            | crate::models::Ornament::InvertedVerticalTurn
                            | crate::models::Ornament::Mordent { .. }
                            | crate::models::Ornament::InvertedMordent { .. } => {
                                let symbol = match ornament {
                                    crate::models::Ornament::Turn
                                    | crate::models::Ornament::DelayedTurn => "\u{E567}",
                                    crate::models::Ornament::InvertedTurn
                                    | crate::models::Ornament::DelayedInvertedTurn => "\u{E568}",
                                    crate::models::Ornament::Haydn => "\u{E56F}",
                                    crate::models::Ornament::VerticalTurn => "\u{E56A}",
                                    crate::models::Ornament::InvertedVerticalTurn => "\u{E56B}",
                                    crate::models::Ornament::Mordent { long: false } => "\u{E56D}",
                                    crate::models::Ornament::Mordent { long: true } => "\u{E56F}",
                                    crate::models::Ornament::InvertedMordent { long: false } => {
                                        "\u{E56C}"
                                    }
                                    crate::models::Ornament::InvertedMordent { long: true } => {
                                        "\u{E56E}"
                                    }
                                    _ => "",
                                };
                                let is_delayed = matches!(
                                    ornament,
                                    crate::models::Ornament::DelayedTurn
                                        | crate::models::Ornament::DelayedInvertedTurn
                                );
                                let mark_x = if is_delayed { x + 20.0 } else { x };

                                let is_above = true;
                                let mark_y = if is_above { min_y - 10.0 } else { max_y + 10.0 };
                                doc = doc.add(
                                    Text::new(symbol)
                                        .set("x", mark_x)
                                        .set("y", mark_y)
                                        .set("font-size", self.staff_line_distance * 3.5)
                                        .set("text-anchor", "middle")
                                        .set("dominant-baseline", "central")
                                        .set("font-family", self.font_family.as_str()),
                                );
                                if is_above {
                                    min_y -= 20.0;
                                } else {
                                    max_y += 20.0;
                                }
                            }
                            crate::models::Ornament::Tremolo { tremolo_type, bars } => {
                                if tremolo_type == "single" {
                                    let symbol = match bars {
                                        1 => "\u{E220}",
                                        2 => "\u{E221}",
                                        3 => "\u{E222}",
                                        4 => "\u{E223}",
                                        5 => "\u{E224}",
                                        _ => "\u{E222}",
                                    };
                                    // Single tremolo is usually centered on the stem
                                    let mut tx = x;
                                    let mut ty = (min_y + max_y) / 2.0;
                                    if let Some(s) = &stem {
                                        if (s.y_start - s.y_tip).abs() > 0.01 {
                                            tx = s.x;
                                            // ty = (s.y_start + s.y_tip) / 2.0;
                                            if s.is_up {
                                                ty = s.y_tip + 15.0;
                                            } else {
                                                ty = s.y_tip - 15.0;
                                            }
                                        } else {
                                            // Whole note with zero-length stem
                                            let staff_center =
                                                staff_y_offset + 2.0 * self.staff_line_distance;
                                            if ty > staff_center {
                                                ty = min_y - 15.0;
                                                min_y -= 25.0;
                                            } else {
                                                ty = max_y + 15.0;
                                                max_y += 25.0;
                                            }
                                        }
                                    } else {
                                        // For whole notes (no stem), place above or below
                                        // Use staff center (staff_y_offset + 2 * distance) to decide
                                        let staff_center =
                                            staff_y_offset + 2.0 * self.staff_line_distance;
                                        if ty > staff_center {
                                            ty = min_y - 15.0;
                                            min_y -= 25.0;
                                        } else {
                                            ty = max_y + 15.0;
                                            max_y += 25.0;
                                        }
                                    }
                                    tx += self.staff_line_distance * 0.3;
                                    doc = doc.add(
                                        Text::new(symbol)
                                            .set("x", tx)
                                            .set("y", ty)
                                            .set("font-size", self.staff_line_distance * 4.0)
                                            .set("text-anchor", "middle")
                                            .set("dominant-baseline", "central")
                                            .set("font-family", self.font_family.as_str()),
                                    );
                                } else if tremolo_type == "start" {
                                    let (tx, ty) = if let Some(s) = &stem {
                                        (s.x, (s.y_start + s.y_tip) / 2.0)
                                    } else {
                                        (x, (min_y + max_y) / 2.0)
                                    };
                                    active_tremolos.insert(
                                        0,
                                        TremoloStartInfo {
                                            x: tx,
                                            y: ty,
                                            bars: *bars,
                                            is_continuation: false,
                                        },
                                    );
                                } else if tremolo_type == "stop" {
                                    if let Some(start_info) = active_tremolos.remove(&0) {
                                        let (tx, ty) = if let Some(s) = &stem {
                                            (s.x, (s.y_start + s.y_tip) / 2.0)
                                        } else {
                                            (x, (min_y + max_y) / 2.0)
                                        };
                                        doc = self.draw_multi_note_tremolo(
                                            doc,
                                            start_info.x,
                                            start_info.y,
                                            tx,
                                            ty,
                                            start_info.bars,
                                        );
                                    }
                                }
                            }
                            crate::models::Ornament::AccidentalMark(mark) => {
                                let (new_doc, new_min, new_max) = self
                                    .render_accidental_mark(doc, mark, x, min_y, max_y, stem, true);
                                doc = new_doc;
                                min_y = new_min;
                                max_y = new_max;
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        (doc, min_y, max_y)
    }

    fn draw_chord_group_at(
        &self,
        mut doc: Document,
        x: f32,
        notes: &[&Note],
        clef: &Clef,
        y_offset: f32,
        slash_active: bool,
        slash_use_stems: bool,
        slash_dots: i32,
        slash_note_type: Option<&str>,
        line_count: i32,
        measure_total_dur: i32,
        measure_theoretical_dur: i32,
        divisions: i32,
        time_pos: i32,
        voice_info: &MeasureVoiceInfo,
    ) -> (Document, Option<StemInfo>, f32, f32, Vec<f32>, Vec<f32>, Vec<f32>) {
        if notes.is_empty() {
            return (doc, None, 0.0, 0.0, Vec::new(), Vec::new(), Vec::new());
        }
        if notes[0].rest && !slash_active {
            let duration_units = if divisions > 0 {
                notes[0].duration as f32 * 10080.0 / divisions as f32
            } else {
                notes[0].duration as f32
            };
            let is_full_measure_rest = notes[0].rest_measure
                && notes[0].note_type.is_none()
                && measure_theoretical_dur > 0
                && duration_units >= measure_theoretical_dur as f32 - 0.5;
            let rest_type = if notes[0].rest_measure && notes[0].note_type.is_none() {
                if is_full_measure_rest {
                    "whole"
                } else {
                    self.infer_note_type_from_duration(notes[0].duration, divisions)
                }
            } else {
                notes[0].note_type.as_deref().unwrap_or("quarter")
            };
            let rest_y = if is_full_measure_rest {
                if line_count == 1 {
                    y_offset + 2.0 * self.staff_line_distance
                } else {
                    y_offset + 1.0 * self.staff_line_distance
                }
            } else if let Some(unpitched) = &notes[0].unpitched {
                // Same height draw_rest gives a positioned rest.
                let temp_pitch = crate::models::Pitch {
                    step: unpitched.display_step.clone(),
                    octave: unpitched.display_octave,
                    alter: None,
                };
                let y = self.pitch_to_y(&temp_pitch, clef, y_offset);
                self.rest_y_clear_of_voices(y, notes[0], time_pos, divisions, voice_info, clef, y_offset)
            } else {
                let base_shift = match rest_type {
                    "whole" => 1.0,
                    "half" => 2.0,
                    _ => 2.0,
                };
                let voice_shift = Self::rest_voice_shift(notes[0], time_pos, divisions, voice_info);
                let y = y_offset + (base_shift + voice_shift) * self.staff_line_distance;
                self.rest_y_clear_of_voices(y, notes[0], time_pos, divisions, voice_info, clef, y_offset)
            };
            return (
                self.draw_rest(
                    doc,
                    notes[0],
                    y_offset,
                    x,
                    notes[0].dot_count,
                    clef,
                    line_count,
                    measure_total_dur,
                    measure_theoretical_dur,
                    divisions,
                    time_pos,
                    voice_info,
                ),
                None,
                x,
                rest_y,
                Vec::new(),
                Vec::new(),
                Vec::new(),
            );
        }
        let mut pitches_y = Vec::new();
        let mut note_xs = vec![x; notes.len()];
        let mut note_ys = vec![0.0; notes.len()];
        let mut primary_y = 0.0;
        let is_grace = notes.iter().any(|n| n.grace.is_some());
        let scale = if is_grace { 0.7 } else { 1.0 };
        let head_w = self.note_head_width * scale;

        // 1. Calculate Y positions and sort by pitch (Y coordinate)
        let mut sorted_notes: Vec<(usize, f32, &Note)> = Vec::new();
        for (idx, note) in notes.iter().enumerate() {
            let y = if slash_active {
                y_offset + 2.0 * self.staff_line_distance
            } else if let Some(pitch) = &note.pitch {
                self.pitch_to_y(pitch, clef, y_offset)
            } else if let Some(unpitched) = &note.unpitched {
                let temp_pitch = crate::models::Pitch {
                    step: unpitched.display_step.clone(),
                    octave: unpitched.display_octave,
                    alter: None,
                };
                self.pitch_to_y(&temp_pitch, clef, y_offset)
            } else {
                y_offset + 2.0 * self.staff_line_distance
            };
            sorted_notes.push((idx, y, note));
        }
        sorted_notes.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        // 2. Determine stem direction (needed for displacement logic)
        let min_y_chord = sorted_notes[0].1;
        let max_y_chord = sorted_notes.last().unwrap().1;
        let stem_up = if let Some(s) = notes[0].stem.as_deref() {
            s == "up"
        } else {
            (min_y_chord + max_y_chord) / 2.0 > y_offset + 2.0 * self.staff_line_distance
        };

        // 3. Assign Displacement (X Offsets)
        //
        // Seconds within a chord can't share a column, so one notehead of each
        // close pair moves to the OTHER side of the stem, touching it — the
        // standard engraving layout:
        //   stem-up   → stem on the right; displaced notes go RIGHT of the stem
        //   stem-down → stem on the left;  displaced notes go LEFT  of the stem
        // Stemless notes (whole/breve) follow the stem-up convention.
        //
        // The anchor note (the one the stem physically starts from) stays at x.
        // Iterate outward from it, alternating sides across each run of seconds:
        //   stem-up   → anchor = lowest note (largest y)  → iterate bottom-to-top
        //   stem-down → anchor = highest note (smallest y) → iterate top-to-bottom
        //
        // The displacement equals the stem-to-stem distance across a notehead
        // (2 × stem offset), so the displaced head's edge meets the stem line.
        let stem_offset_x = if is_grace { 4.0 } else { 5.8 };
        let is_stemless = matches!(
            notes[0].note_type.as_deref(),
            Some("whole") | Some("breve") | Some("long") | Some("maxima")
        );
        let displace_w = if is_stemless {
            // A whole notehead is ~1.69 staff spaces wide.
            self.staff_line_distance * 1.69 * scale
        } else {
            2.0 * stem_offset_x
        };
        let mut displacements = vec![0.0; notes.len()];
        if !slash_active && clef.sign != "TAB" {
            let n = sorted_notes.len();
            if stem_up || is_stemless {
                let mut current_side = 0.0;
                for i in (0..n.saturating_sub(1)).rev() {
                    let diff = (sorted_notes[i + 1].1 - sorted_notes[i].1).abs();
                    if diff < self.staff_line_distance * 0.95 {
                        current_side = 1.0 - current_side;
                    } else {
                        current_side = 0.0;
                    }
                    displacements[sorted_notes[i].0] = current_side * displace_w;
                }
            } else {
                let mut current_side = 0.0;
                for i in 1..n {
                    let diff = (sorted_notes[i].1 - sorted_notes[i - 1].1).abs();
                    if diff < self.staff_line_distance * 0.95 {
                        current_side = 1.0 - current_side;
                    } else {
                        current_side = 0.0;
                    }
                    displacements[sorted_notes[i].0] = current_side * (-displace_w);
                }
            }
        }
        // Augmentation dots go after the rightmost notehead of the chord.
        let dots_base_x = x + displacements.iter().cloned().fold(0.0_f32, f32::max);

        let chord_left_edge = sorted_notes
            .iter()
            .filter_map(|(idx, _y, note)| {
                if note.print_object == Some(false) {
                    None
                } else {
                    Some(x + displacements[*idx] - head_w * 0.5)
                }
            })
            .fold(x, f32::min);
        let is_cue_chord = notes.iter().any(|n| n.is_cue);
        let accidental_note_gap = if is_cue_chord { 7.0 } else { 9.0 };
        let accidental_column_spacing = if is_cue_chord { 8.0 } else { 10.0 };
        // Accidentals closer than a sixth would overlap, so they go in
        // separate columns.
        let accidental_min_y_gap = self.staff_line_distance * ACCIDENTAL_COLUMN_STEPS as f32 * 0.5;
        // Accidentals already drawn on this staff near this onset (other
        // voices), then this chord's own as they are placed.
        let mut taken: Vec<(f32, f32)> = self
            .placed_accidentals
            .borrow()
            .iter()
            .filter(|&&(staff_y, ax, _)| {
                (staff_y - y_offset).abs() < 0.5 && (ax - chord_left_edge).abs() < 60.0
            })
            .map(|&(_, ax, ay)| (ax, ay))
            .collect();
        let mut accidental_x_by_note = vec![chord_left_edge - accidental_note_gap; notes.len()];
        for (idx, y, note) in &sorted_notes {
            let has_accidental = note.accidental.is_some()
                || note
                    .pitch
                    .as_ref()
                    .and_then(|p| p.alter)
                    .map(|a| a.abs() > 0.01)
                    .unwrap_or(false);
            if !has_accidental || note.print_object == Some(false) {
                continue;
            }
            // Accidentals sit left of the whole chord, including noteheads
            // displaced to the left of the stem (and never between a
            // right-displaced notehead and the stem); each one moves a
            // column further left while it would hit one already placed.
            let desired_x = chord_left_edge - accidental_note_gap;
            let mut col_x = desired_x;
            while taken.iter().any(|&(ax, ay)| {
                (ax - col_x).abs() < accidental_column_spacing * 0.9
                    && (ay - y).abs() < accidental_min_y_gap
            }) {
                col_x -= accidental_column_spacing;
            }
            taken.push((col_x, *y));
            accidental_x_by_note[*idx] = col_x;
            if !slash_active {
                self.placed_accidentals.borrow_mut().push((y_offset, col_x, *y));
            }
        }

        // 4. Render Noteheads, Accidentals, and Dots
        for (idx, y, note) in sorted_notes {
            note_ys[idx] = y;
            if note.print_object == Some(false) {
                continue;
            }
            let x_offset = displacements[idx];
            let note_x = x + x_offset;
            note_xs[idx] = note_x;

            let note_type = if slash_active && slash_note_type.is_some() {
                slash_note_type.unwrap()
            } else {
                note.note_type.as_deref().unwrap_or("quarter")
            };
            let dot_count = if slash_active && slash_dots > 0 {
                slash_dots
            } else {
                note.dot_count
            };

            let mut notehead = note.notehead.as_ref();
            let slash_notehead = crate::models::Notehead {
                value: "slash".to_string(),
                filled: None,
            };
            if slash_active {
                notehead = Some(&slash_notehead);
            }

            let is_cue = note.is_cue;
            let is_tab = clef.sign == "TAB";

            if is_tab {
                let mut string_num = None;
                let mut fret_num = None;
                for notation in &note.notations {
                    if let Notation::Technical(techs) = notation {
                        for tech in techs {
                            match tech {
                                crate::models::TechnicalMark::Fret(f) => fret_num = Some(*f),
                                crate::models::TechnicalMark::String(s) => string_num = Some(*s),
                                _ => {}
                            }
                        }
                    }
                }
                if let Some(s) = string_num {
                    let mut tab_y = y_offset + ((s - 1) as f32 * self.staff_line_distance);
                    tab_y += 0.5 * self.staff_line_distance;
                    pitches_y.push(tab_y);
                    note_ys[idx] = tab_y;
                    // The first note, or the first visible one if it is hidden.
                    if idx == 0 || pitches_y.len() == 1 {
                        primary_y = tab_y;
                    }
                    if let Some(f) = fret_num {
                        doc = doc.add(
                            svg::node::element::Rectangle::new()
                                .set("x", x - 6.0)
                                .set("y", tab_y - 1.0)
                                .set("width", 12.0)
                                .set("height", 2.0)
                                .set("fill", "white"),
                        );
                        doc = doc.add(
                            Text::new(f.to_string().as_str())
                                .set("x", x)
                                .set("y", tab_y)
                                .set("font-size", self.staff_line_distance * 1.5)
                                .set("text-anchor", "middle")
                                .set("dominant-baseline", "central")
                                .set("font-family", self.font_family.as_str())
                                .set("font-weight", "bold")
                                .set("fill", "black"),
                        );
                    }
                }
            } else {
                // The first note, or the first visible one if it is hidden.
                if idx == 0 || pitches_y.is_empty() {
                    primary_y = y;
                }
                pitches_y.push(y);
                if !slash_active {
                    doc = self.draw_ledger_lines(doc, y, y_offset, note_x, line_count);
                }
                doc = self.draw_notehead(doc, note_x, y, note_type, notehead, is_cue || is_grace);
            }

            if !is_tab {
                if let Some(acc) = &note.accidental {
                    if !slash_active {
                        doc = self.draw_accidental(
                            doc,
                            accidental_x_by_note[idx],
                            y,
                            acc,
                            is_cue || is_grace,
                        );
                    }
                } else if let Some(pitch) = &note.pitch {
                    if let Some(alter) = pitch.alter {
                        if alter.abs() > 0.01 && !slash_active {
                            doc = self.draw_microtonal_accidental(
                                doc,
                                accidental_x_by_note[idx],
                                y,
                                alter,
                                is_cue || is_grace,
                            );
                        }
                    }
                }
            }
            // TAB shows fret numbers without rhythm: a dot there would land at
            // the pitch's notation-staff y, far below the TAB lines.
            if note.print_dot != Some(false) && !is_tab {
                for i in 0..dot_count {
                    let dot_x = dots_base_x + head_w + (i as f32 * 6.0 * scale);
                    let dot_y = if slash_active {
                        y - 3.0 * scale
                    } else if (y - y_offset) % self.staff_line_distance == 0.0 {
                        y - 3.0 * scale
                    } else {
                        y
                    };
                    doc = doc.add(
                        svg::node::element::Circle::new()
                            .set("cx", dot_x)
                            .set("cy", dot_y)
                            .set("r", 1.5 * scale)
                            .set("fill", "black"),
                    );
                }
            }
        }
        // 5. Draw Stem and Flag
        let mut stem_info = None;
        if !pitches_y.is_empty() {
            pitches_y.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let min_y_sorted = pitches_y[0];
            let max_y_sorted = *pitches_y.last().unwrap();
            let note_type = if slash_active && slash_note_type.is_some() {
                slash_note_type.unwrap()
            } else {
                notes[0].note_type.as_deref().unwrap_or("quarter")
            };

            let mut draw_stem =
                notes[0].stem.as_deref() != Some("none") && (!slash_active || slash_use_stems);
            let is_tab = clef.sign == "TAB";
            if is_tab
                && (line_count == 6
                    || notes[0].notations.iter().any(|n| match n {
                        Notation::Technical(techs) => techs
                            .iter()
                            .any(|t| matches!(t, crate::models::TechnicalMark::Bend(_))),
                        _ => false,
                    }))
            {
                draw_stem = false;
            }

            let stem_x = if stem_up {
                x + stem_offset_x
            } else {
                x - stem_offset_x
            };
            let stem_y_start = if stem_up { max_y_sorted } else { min_y_sorted };
            let stem_len = if is_grace {
                2.5 * self.staff_line_distance
            } else {
                3.5 * self.staff_line_distance
            };
            let stem_y_end = if stem_up {
                min_y_sorted - stem_len
            } else {
                max_y_sorted + stem_len
            };

            if draw_stem && note_type != "whole" && note_type != "breve" {
                stem_info = Some(StemInfo {
                    x: stem_x,
                    y_start: stem_y_start,
                    y_tip: stem_y_end,
                    is_up: stem_up,
                    y_top: min_y_sorted,
                    y_bottom: max_y_sorted,
                    is_cue: notes[0].is_cue,
                    staff: notes[0].staff.unwrap_or(1),
                });
                if notes[0].beams.is_empty() {
                    doc = doc.add(
                        Line::new()
                            .set("x1", stem_x)
                            .set("y1", stem_y_start)
                            .set("x2", stem_x)
                            .set("y2", stem_y_end)
                            .set("stroke", "black")
                            .set("stroke-width", if is_grace { 0.8 } else { 1.2 }),
                    );
                    doc = self.draw_flag(doc, stem_x, stem_y_end, note_type, stem_up, is_grace);
                }
                if is_grace {
                    if let Some(g) = &notes[0].grace {
                        if g.slash.as_deref() == Some("yes") {
                            let sy = (stem_y_start + stem_y_end) * 0.5;
                            doc = doc.add(
                                Line::new()
                                    .set("x1", stem_x - 5.0 * scale)
                                    .set("y1", sy + 3.0 * scale)
                                    .set("x2", stem_x + 5.0 * scale)
                                    .set("y2", sy - 3.0 * scale)
                                    .set("stroke", "black")
                                    .set("stroke-width", 1.0 * scale),
                            );
                        }
                    }
                }
            }

            if draw_stem && stem_info.is_none() {
                stem_info = Some(StemInfo {
                    x: stem_x,
                    y_start: stem_y_start,
                    y_tip: stem_y_start,
                    is_up: stem_up,
                    y_top: min_y_sorted,
                    y_bottom: max_y_sorted,
                    is_cue: notes[0].is_cue,
                    staff: notes[0].staff.unwrap_or(1),
                });
            }
        }
        (doc, stem_info, x, primary_y, pitches_y, note_xs, note_ys)
    }

    fn process_fermatas(
        &self,
        mut doc: Document,
        notes: &[&Note],
        x: f32,
        mut min_y: f32,
        mut max_y: f32,
        stem: Option<StemInfo>,
        (staff_top, staff_bottom): (f32, f32),
    ) -> (Document, f32, f32) {
        for note in notes {
            for notation in &note.notations {
                if let Notation::Fermata {
                    note_type,
                    placement,
                } = notation
                {
                    // The type names the drawn shape: an upright fermata sits
                    // above the staff, an inverted one below, whatever the stem.
                    let is_above = if note_type.as_deref() == Some("inverted") {
                        false
                    } else if note_type.as_deref() == Some("upright") {
                        true
                    } else if let Some(p) = placement.as_deref() {
                        p == "above"
                    } else if let Some(s) = stem {
                        !s.is_up
                    } else {
                        true
                    };
                    let symbol = if is_above { "\u{E4C0}" } else { "\u{E4C1}" };
                    // Outside the staff even over a note inside it.
                    if is_above {
                        min_y = min_y.min(staff_top - 5.0);
                    } else {
                        max_y = max_y.max(staff_bottom + 5.0);
                    }
                    // The glyph's baseline is its flat side: an upright fermata
                    // is drawn above it, an inverted one below it.
                    let fermata_y = if is_above { min_y - 6.0 } else { max_y + 6.0 };
                    doc = doc.add(
                        Text::new(symbol)
                            .set("x", x)
                            .set("y", fermata_y)
                            .set("font-size", self.staff_line_distance * 4.0)
                            .set("text-anchor", "middle")
                            .set("dominant-baseline", "central")
                            .set("font-family", self.font_family.as_str()),
                    );
                    if is_above {
                        min_y -= 25.0;
                    } else {
                        max_y += 35.0;
                    }
                }
            }
        }
        (doc, min_y, max_y)
    }

    fn articulation_to_smufl(&self, name: &str) -> Option<&'static str> {
        match name {
            "breath-mark" => Some("\u{E4CE}"),
            "caesura" => Some("\u{E4D1}"),
            "detached-legato" => Some("\u{E4B2}"),
            "doit" => Some("\u{E5D2}"),
            "falloff" => Some("\u{E5DE}"),
            "plop" => Some("\u{E5DE}"),
            "scoop" => Some("\u{E5D0}"),
            _ => None,
        }
    }

    fn process_articulations(
        &self,
        mut doc: Document,
        notes: &[&Note],
        x: f32,
        mut min_y: f32,
        mut max_y: f32,
        stem: Option<StemInfo>,
        pitches_y: &[f32],
        staff_y_offset: f32,
    ) -> (Document, f32, f32) {
        for (i, note) in notes.iter().enumerate() {
            let current_note_y = pitches_y.get(i).cloned().unwrap_or((min_y + max_y) / 2.0);
            for notation in &note.notations {
                if let Notation::Articulation {
                    name: art_name,
                    placement,
                    default_x,
                    default_y,
                } = notation
                {
                    // Follow stem direction: UP -> ABOVE, DOWN -> BELOW
                    let mut is_above = if let Some(p) = placement.as_deref() {
                        p == "above"
                    } else {
                        if let Some(s) = stem { s.is_up } else { true }
                    };

                    // User requested staccato and accent to be placed relative to notehead
                    // and consider stem direction. Standard practice is opposite stem.
                    if (*art_name == "staccato" || *art_name == "accent") && placement.is_none() {
                        if let Some(s) = stem {
                            is_above = !s.is_up;
                        }
                    }

                    let smufl = if *art_name == "soft-accent" {
                        Some(if is_above { "\u{ED40}" } else { "\u{ED41}" })
                    } else if *art_name == "accent" {
                        Some(if is_above { "\u{E4A0}" } else { "\u{E4A1}" })
                    } else if *art_name == "staccato" {
                        Some(if is_above { "\u{E4A2}" } else { "\u{E4A3}" })
                    } else if *art_name == "tenuto" {
                        Some(if is_above { "\u{E4A4}" } else { "\u{E4A5}" })
                    } else if *art_name == "staccatissimo" {
                        Some(if is_above { "\u{E4A6}" } else { "\u{E4A7}" })
                    } else if *art_name == "spiccato" {
                        Some(if is_above { "\u{E4A8}" } else { "\u{E4A9}" })
                    } else if *art_name == "marcato" || *art_name == "strong-accent" {
                        Some(if is_above { "\u{E4AC}" } else { "\u{E4AD}" })
                    } else if *art_name == "stress" {
                        Some(if is_above { "\u{E4B6}" } else { "\u{E4B7}" })
                    } else if *art_name == "unstress" {
                        Some(if is_above { "\u{E4B8}" } else { "\u{E4B9}" })
                    } else {
                        self.articulation_to_smufl(art_name)
                    };

                    if let Some(smufl) = smufl {
                        let font_size = if *art_name == "staccato" {
                            self.staff_line_distance * 4.0
                        } else {
                            self.staff_line_distance * 3.0
                        };

                        let mut art_y = if is_above { min_y - 1.0 } else { max_y + 1.0 };

                        // Special placement for staccato and accent based on notehead position
                        if (*art_name == "staccato" || *art_name == "accent")
                            && !pitches_y.is_empty()
                        {
                            let extreme_note_y = if is_above {
                                pitches_y.iter().cloned().fold(f32::MAX, f32::min)
                            } else {
                                pitches_y.iter().cloned().fold(f32::MIN, f32::max)
                            };

                            let on_line = (((extreme_note_y - staff_y_offset)
                                / (self.staff_line_distance * 0.5))
                                .round() as i32)
                                .abs()
                                % 2
                                == 0;
                            let dist = if on_line { 0.7 } else { 1.0 } * self.staff_line_distance;

                            let target_y = if is_above {
                                extreme_note_y - dist
                            } else {
                                extreme_note_y + dist
                            };

                            // Ensure it's at least outside current assembly bounds to avoid collisions with stems or other notes
                            art_y = if is_above {
                                target_y.min(min_y - 1.0)
                            } else {
                                target_y.max(max_y + 1.0)
                            };
                        }

                        let (mut art_x, mut anchor, baseline) = if *art_name == "doit"
                            || *art_name == "falloff"
                            || *art_name == "plop"
                            || *art_name == "scoop"
                        {
                            let b = if *art_name == "falloff"
                                || *art_name == "plop"
                                || *art_name == "scoop"
                            {
                                if *art_name == "plop" {
                                    art_y = if let Some(s) = stem {
                                        s.y_start
                                    } else {
                                        current_note_y
                                    };
                                } else if *art_name == "scoop" {
                                    art_y = current_note_y - 2.5 * self.staff_line_distance; // 2.5 lines above notehead center
                                } else {
                                    art_y = if is_above {
                                        min_y - 1.6 * self.staff_line_distance
                                    } else {
                                        max_y - 0.4 * self.staff_line_distance
                                    };
                                }
                                "central"
                            } else {
                                art_y = if is_above {
                                    min_y - 0.5 * self.staff_line_distance
                                } else {
                                    max_y + 0.5 * self.staff_line_distance
                                };
                                "central"
                            };
                            if *art_name == "plop" {
                                (x - 8.0, "end", b)
                            } else if *art_name == "scoop" {
                                (x - 12.0, "end", b)
                            } else {
                                (x + 6.0, "start", b)
                            }
                        } else {
                            (x, "middle", "central")
                        };

                        // Special case for caesura: top-right of notehead
                        if *art_name == "caesura" {
                            art_x = x + 15.0; // Shift right to edge of notehead
                            art_y = current_note_y - 12.0; // Above notehead
                            anchor = "start";
                        }

                        // Apply default-x/y if present (simplified relative apply)
                        if let Some(dx) = default_x {
                            // default-x in MusicXML is often from notehead start (roughly x-7)
                            // Here we just use it as an offset if it's large, or a relative shift
                            if *dx > 20.0 {
                                art_x = x + (*dx - 10.0); // Rough adjustment
                            }
                        }
                        if let Some(dy) = default_y {
                            // If default-y is present, use it relative to staff top line (staff_y_offset)
                            // Skip for staccato/accent to prioritize our new notehead-relative rules
                            if *art_name != "staccato" && *art_name != "accent" {
                                let adj_dy = if *dy < 0.0 { *dy + 20.0 } else { *dy - 5.0 };
                                art_y = staff_y_offset - (adj_dy * self.staff_line_distance / 10.0);
                            }
                        }

                        // Ensure we are outside the note assembly (Skip for caesura)
                        if *art_name != "caesura" {
                            if is_above {
                                art_y = art_y.min(min_y - 1.0);
                            } else {
                                art_y = art_y.max(max_y + 1.0);
                            }
                        }

                        doc = doc.add(
                            Text::new(smufl)
                                .set("x", art_x)
                                .set("y", art_y)
                                .set("font-size", font_size)
                                .set("text-anchor", anchor)
                                .set("dominant-baseline", baseline)
                                .set("font-family", self.font_family.as_str()),
                        );

                        if is_above && *art_name != "caesura" {
                            min_y = min_y.min(art_y - 6.0);
                        } else if !is_above && *art_name != "caesura" {
                            max_y = max_y.max(art_y + 6.0);
                        }
                    }
                }
            }
        }
        (doc, min_y, max_y)
    }

    fn process_tuplets(
        &self,
        doc: Document,
        notes: &[&Note],
        x: f32,
        mut min_y: f32,
        mut max_y: f32,
        stem: Option<StemInfo>,
        active_tuplets: &mut HashMap<i32, TupletStartInfo>,
    ) -> (Document, f32, f32) {
        if let Some(first_note) = notes.first() {
            for notation in &first_note.notations {
                if let Notation::Tuplet {
                    number,
                    note_type,
                    bracket,
                    show_number,
                    actual_notes,
                    placement,
                    ..
                } = notation
                {
                    let num = number.unwrap_or(1);
                    match note_type.as_str() {
                        "start" => {
                            let is_above = match placement.as_deref() {
                                Some("above") => true,
                                Some("below") => false,
                                _ => stem.map_or(true, |s| s.is_up),
                            };
                            let level = active_tuplets.len() as i32;
                            let tuplet_y = if is_above {
                                min_y - 15.0 - (level as f32 * 10.0)
                            } else {
                                max_y + 15.0 + (level as f32 * 10.0)
                            };

                            let display_number = actual_notes.unwrap_or_else(|| {
                                first_note
                                    .time_modification
                                    .as_ref()
                                    .map(|tm| tm.actual_notes)
                                    .unwrap_or(3)
                            });

                            let tuplet_x = if let Some(s) = stem { s.x } else { x };
                            let is_bracket = bracket.as_deref() != Some("no");
                            let is_show_num = show_number.as_deref() != Some("none");

                            active_tuplets.insert(
                                num,
                                TupletStartInfo {
                                    x: tuplet_x,
                                    is_above,
                                    number: display_number,
                                    level,
                                    bracket: is_bracket,
                                    show_number: is_show_num,
                                    staff: first_note.staff.unwrap_or(1),
                                    top: min_y,
                                    bottom: max_y,
                                },
                            );
                            if is_above {
                                min_y = min_y.min(tuplet_y - 15.0);
                            } else {
                                max_y = max_y.max(tuplet_y + 15.0);
                            }
                        }
                        "stop" => {
                            if let Some(start_info) = active_tuplets.remove(&num) {
                                let is_above = start_info.is_above;
                                let tuplet_end_x = if let Some(s) = stem { s.x } else { x };
                                // Drawn at the end of the measure, once all the
                                // notes under it have their final positions.
                                self.pending_tuplets.borrow_mut().push(PendingTuplet {
                                    start: start_info,
                                    x2: tuplet_end_x,
                                    top2: min_y,
                                    bottom2: max_y,
                                });
                                if is_above {
                                    min_y -= 20.0;
                                } else {
                                    max_y += 20.0;
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        (doc, min_y, max_y)
    }

    fn draw_tuplet(
        &self,
        mut doc: Document,
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        is_above: bool,
        number: i32,
        bracket: bool,
        show_number: bool,
    ) -> Document {
        let midpoint_x = (x1 + x2) / 2.0;
        let midpoint_y = (y1 + y2) / 2.0;

        if bracket {
            let tick_len = if is_above { 8.0 } else { -8.0 };
            let data = Data::new()
                .move_to((x1, y1 + tick_len))
                .line_to((x1, y1))
                .line_to((x2, y2))
                .line_to((x2, y2 + tick_len));
            doc = doc.add(
                Path::new()
                    .set("fill", "none")
                    .set("stroke", "black")
                    .set("stroke-width", 1)
                    .set("d", data),
            );
        }

        if show_number {
            let font_size = 10.0;
            let text_w = 8.0;
            let text_h = 10.0;
            doc = doc.add(
                svg::node::element::Rectangle::new()
                    .set("x", midpoint_x - text_w * 0.5)
                    .set("y", midpoint_y - text_h * 0.5)
                    .set("width", text_w)
                    .set("height", text_h)
                    .set("fill", "white"),
            );
            doc = doc.add(
                Text::new(number.to_string())
                    .set("x", midpoint_x)
                    .set("y", midpoint_y + 3.0)
                    .set("font-size", font_size)
                    .set("text-anchor", "middle")
                    .set("dominant-baseline", "central")
                    .set("font-family", self.font_family.as_str())
                    .set("font-weight", "bold"),
            );
        }
        doc
    }

    fn process_ties(
        &self,
        mut doc: Document,
        notes: &[&Note],
        x: f32,
        y: f32,
        stem: Option<StemInfo>,
        active_ties: &mut HashMap<(String, i32, i32), TieStartInfo>,
        note_xs: &[f32],
        note_ys: &[f32],
    ) -> (Document, f32, f32) {
        let mut min_y = f32::MAX;
        let mut max_y = f32::MIN;
        for (idx, note) in notes.iter().enumerate() {
            if let Some(pitch) = &note.pitch {
                let pitch_key = (pitch.step.clone(), pitch.octave, note.staff.unwrap_or(1));
                let notehead_x = note_xs.get(idx).copied().unwrap_or(x);
                // Each chord note ties from its own notehead, not the chord's.
                let y = note_ys.get(idx).copied().unwrap_or(y);
                let voice = note.voice.unwrap_or(1);
                // A tie always ends on its voice's next note of the same pitch;
                // files sometimes leave that note's stop out (e.g. grace notes
                // of a written-out arpeggio tied into the chord). End it here
                // rather than leave it open, drawn on to every system's end.
                let has_stop = note
                    .notations
                    .iter()
                    .any(|n| matches!(n, Notation::Tied { note_type } if note_type == "stop"));
                if !has_stop
                    && active_ties
                        .get(&pitch_key)
                        .is_some_and(|t| t.voice == voice && t.x < notehead_x - 1.0)
                {
                    let start_info = active_ties.remove(&pitch_key).unwrap();
                    let end_tie_y = if start_info.is_above { y - 3.0 } else { y + 3.0 };
                    doc = self.draw_tie(
                        doc,
                        start_info.x,
                        start_info.y,
                        notehead_x,
                        end_tie_y,
                        start_info.is_above,
                    );
                    min_y = min_y.min(start_info.y.min(end_tie_y) - 12.0);
                    max_y = max_y.max(start_info.y.max(end_tie_y) + 12.0);
                }
                for notation in &note.notations {
                    if let Notation::Tied { note_type } = notation {
                        let is_above = if let Some(s) = stem { !s.is_up } else { true };
                        let tie_y = if is_above { y - 3.0 } else { y + 3.0 };
                        match note_type.as_str() {
                            "start" => {
                                active_ties.insert(
                                    pitch_key.clone(),
                                    TieStartInfo {
                                        x: notehead_x,
                                        y: tie_y,
                                        is_above,
                                        is_continuation: false,
                                        voice,
                                    },
                                );
                                if is_above {
                                    min_y = min_y.min(tie_y - 12.0);
                                } else {
                                    max_y = max_y.max(tie_y + 12.0);
                                }
                            }
                            "stop" => {
                                if let Some(start_info) = active_ties.remove(&pitch_key) {
                                    // Use the orientation (is_above) from the START note to ensure consistency.
                                    // But use the current y coordinate for the end point.
                                    let end_tie_y = if start_info.is_above {
                                        y - 3.0
                                    } else {
                                        y + 3.0
                                    };
                                    doc = self.draw_tie(
                                        doc,
                                        start_info.x,
                                        start_info.y,
                                        notehead_x,
                                        end_tie_y,
                                        start_info.is_above,
                                    );
                                    let tie_top = start_info.y.min(end_tie_y) - 12.0;
                                    let tie_bottom = start_info.y.max(end_tie_y) + 12.0;
                                    min_y = min_y.min(tie_top);
                                    max_y = max_y.max(tie_bottom);
                                }
                            }
                            "let-ring" => {
                                doc = self.draw_let_ring(doc, notehead_x, tie_y, is_above);
                                if is_above {
                                    min_y = min_y.min(tie_y - 12.0);
                                } else {
                                    max_y = max_y.max(tie_y + 12.0);
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        (doc, min_y, max_y)
    }

    fn draw_tie(
        &self,
        doc: Document,
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        is_above: bool,
    ) -> Document {
        let dx = x2 - x1;
        if dx < 2.0 {
            return doc;
        }
        let height = (dx * 0.10).max(2.5).min(7.0);
        let direction = if is_above { -1.0 } else { 1.0 };
        let thickness = 1.5;
        let start_x = x1 + 4.0;
        let end_x = x2 - 4.0;

        if end_x <= start_x {
            return doc;
        }

        let mid_dx = end_x - start_x;
        let cx1 = start_x + mid_dx * 0.25;
        let cy1 = y1 + direction * height;
        let cx2 = start_x + mid_dx * 0.75;
        let cy2 = y2 + direction * height;
        let path = Path::new().set("fill", "black").set(
            "d",
            Data::new()
                .move_to((start_x, y1))
                .cubic_curve_to((cx1, cy1, cx2, cy2, end_x, y2))
                .cubic_curve_to((
                    cx2,
                    cy2 + direction * thickness,
                    cx1,
                    cy1 + direction * thickness,
                    start_x,
                    y1,
                ))
                .close(),
        );
        doc.add(path)
    }

    fn draw_let_ring(&self, doc: Document, x: f32, y: f32, is_above: bool) -> Document {
        let dx = 12.0;
        let height = 3.0;
        let direction = if is_above { -1.0 } else { 1.0 };
        let thickness = 1.2;
        let start_x = x + 4.0;
        let end_x = start_x + dx;
        let cx1 = start_x + dx * 0.3;
        let cy1 = y + direction * height;
        let cx2 = start_x + dx * 0.7;
        let cy2 = y + direction * height;
        let path = Path::new().set("fill", "black").set(
            "d",
            Data::new()
                .move_to((start_x, y))
                .cubic_curve_to((cx1, cy1, cx2, cy2, end_x, y + direction * 1.0))
                .cubic_curve_to((
                    cx2,
                    cy2 + direction * thickness,
                    cx1,
                    cy1 + direction * thickness,
                    start_x,
                    y,
                ))
                .close(),
        );
        doc.add(path)
    }

    fn process_slurs(
        &self,
        doc: Document,
        notes: &[&Note],
        x: f32,
        _y: f32,
        stem: Option<StemInfo>,
        active_slurs: &mut HashMap<i32, SlurStartInfo>,
        is_tab: bool,
        pitches_y: &[f32],
        note_xs: &[f32],
        beam_tips: &[BeamedStemTip],
        voice_sharing: VoiceSharing,
    ) -> (Document, f32, f32) {
        let mut min_y = f32::MAX;
        let mut max_y = f32::MIN;
        if let Some(first_note) = notes.first() {
            // Nothing drawn to attach to (hidden note, rest): this group can't
            // be a slur endpoint. Drop any slur it would close rather than
            // drawing one to a meaningless position.
            if pitches_y.is_empty() {
                for notation in &first_note.notations {
                    if let Notation::Slur { number, note_type, .. } = notation {
                        if note_type == "stop" {
                            active_slurs.remove(number);
                        }
                    }
                }
                return (doc, min_y, max_y);
            }
            let min_pitch_y = pitches_y.iter().cloned().fold(f32::MAX, f32::min);
            let max_pitch_y = pitches_y.iter().cloned().fold(f32::MIN, f32::max);
            min_y = min_y.min(min_pitch_y);
            max_y = max_y.max(max_pitch_y);
            let stem_x = stem.map(|s| s.x).unwrap_or(x);
            let notehead_x = note_xs.first().copied().unwrap_or(x);
            // A wholly hidden group (a written-out trill's notes kept for
            // playback) draws nothing for other marks to clear.
            if notes.iter().any(|n| n.print_object != Some(false)) {
                // Record this note first: when it ends a beam group, the beam
                // just drawn sets its stem too.
                self.placed_notes.borrow_mut().push(PlacedNote {
                    part_y: self.current_part_y.get(),
                    staff: first_note.staff.unwrap_or(1),
                    x: notehead_x,
                    top: min_pitch_y - 5.0,
                    bottom: max_pitch_y + 5.0,
                    stem: stem.map(|s| (s.x, s.y_tip, s.is_up)),
                    cross_beam: false,
                });
                self.apply_beam_tips_to_placed_notes(beam_tips);
            }

            // Slur endpoints always anchor to the plain notehead position (not a
            // stem-tip or tuplet-occupancy-clamped one) — see process_slurs' "start"
            // and "stop" match arms below for why those clamps produced wrong anchors.
            let above_y_raw = if is_tab {
                min_pitch_y - 8.0
            } else {
                min_pitch_y - 7.0
            };
            let below_y_raw = if is_tab {
                max_pitch_y + 8.0
            } else {
                max_pitch_y + 7.0
            };
            let below_x = if stem.map(|s| s.is_up).unwrap_or(false) {
                notehead_x
            } else {
                stem_x
            };

            // Record this note as an obstacle for every active slur in the same
            // staff that it neither starts nor stops, so the curve can be raised
            // (or lowered) to clear intermediate noteheads and stems — other
            // voices' notes included, since they can poke through just the same.
            let note_staff = first_note.staff.unwrap_or(1);
            let own_slur_numbers: Vec<i32> = first_note
                .notations
                .iter()
                .filter_map(|n| match n {
                    Notation::Slur { number, .. } => Some(*number),
                    _ => None,
                })
                .collect();
            for (num, slur) in active_slurs.iter_mut() {
                if slur.pending_stop.is_some()
                    || own_slur_numbers.contains(num)
                    || slur.staff != note_staff
                {
                    continue;
                }
                let stem_up = stem.map(|s| s.is_up).unwrap_or(false);
                if slur.is_above {
                    let mut top = min_pitch_y - 5.0;
                    if let Some(s) = stem.filter(|_| stem_up) {
                        slur.obstacles.push((s.x, s.y_tip));
                        top = top.min(s.y_tip);
                    }
                    slur.obstacles.push((notehead_x, top));
                } else {
                    let mut bottom = max_pitch_y + 5.0;
                    if let Some(s) = stem.filter(|_| !stem_up) {
                        slur.obstacles.push((s.x, s.y_tip));
                        bottom = bottom.max(s.y_tip);
                    }
                    slur.obstacles.push((notehead_x, bottom));
                }
            }

            for notation in &first_note.notations {
                if let Notation::Slur {
                    number,
                    note_type,
                    placement,
                } = notation
                {
                    match note_type.as_str() {
                        "start" => {
                            let is_above = match placement.as_deref() {
                                Some("above") => true,
                                Some("below") => false,
                                // With another voice sharing the staff, the slur goes
                                // away from it (see `VoiceSharing`); otherwise on the
                                // notehead side.
                                _ => match (&stem, voice_sharing) {
                                    (Some(s), VoiceSharing::Now) => s.is_up,
                                    (_, VoiceSharing::Later { upper }) => upper,
                                    (Some(s), VoiceSharing::Alone) => !s.is_up,
                                    (None, _) => true,
                                },
                            };
                            // Attach to the note's outermost element on the slur's side:
                            // above, the stem tip of an up-stem note (else the notehead);
                            // below, the stem tip of a down-stem note (else the notehead).
                            let start_stem_up = stem.map(|s| s.is_up).unwrap_or(false);
                            let use_stem_tip = is_above && start_stem_up;
                            let use_stem_tip_below =
                                !is_above && stem.is_some_and(|s| !s.is_up);
                            let cur_y = if use_stem_tip {
                                stem.unwrap().y_tip - 6.0
                            } else if use_stem_tip_below {
                                stem.unwrap().y_tip + 6.0
                            } else if is_above {
                                above_y_raw
                            } else {
                                below_y_raw
                            };
                            let cur_x = if use_stem_tip || use_stem_tip_below {
                                stem.unwrap().x
                            } else if is_above {
                                notehead_x
                            } else {
                                below_x
                            };
                            // The matching stop may already have been read (it came
                            // earlier in the file but later in time, e.g. on an
                            // upper staff): draw the slur now.
                            let pending_key = PENDING_SLUR_STOP_KEY + *number;
                            let known_key = {
                                let mut key = KNOWN_SLUR_END_KEY + *number * 64;
                                while active_slurs.contains_key(&key) {
                                    key += 1;
                                }
                                key
                            };
                            let pending = active_slurs
                                .get(&pending_key)
                                .and_then(|p| p.pending_stop)
                                .filter(|p| p.above.0.max(p.below.0) > cur_x + 1.0);
                            // Its end is fixed, but the notes it spans on this staff
                            // come later in the file, so keep it open to collect
                            // them and draw it at the end of the measure.
                            if let Some(p) = pending {
                                active_slurs.remove(&pending_key);
                                let (end_x, end_y) = if is_above { p.above } else { p.below };
                                active_slurs.insert(
                                    known_key,
                                    SlurStartInfo {
                                        x: cur_x,
                                        y: cur_y,
                                        is_above,
                                        is_continuation: false,
                                        staff: note_staff,
                                        obstacles: Vec::new(),
                                        stem_anchor: use_stem_tip || use_stem_tip_below,
                                        pending_stop: None,
                                        known_end: Some((end_x, end_y, p.staff, p.top, p.bottom)),
                                        resumed: false,
                                    },
                                );
                                min_y = min_y.min(cur_y.min(end_y) - 20.0);
                                max_y = max_y.max(cur_y.max(end_y) + 20.0);
                                continue;
                            }
                            active_slurs.insert(
                                *number,
                                SlurStartInfo {
                                    x: cur_x,
                                    y: cur_y,
                                    is_above,
                                    is_continuation: false,
                                    staff: note_staff,
                                    obstacles: Vec::new(),
                                    stem_anchor: use_stem_tip || use_stem_tip_below,
                                    pending_stop: None,
                                    known_end: None,

                                    resumed: false,
                                },
                            );
                            if is_above {
                                min_y = min_y.min(cur_y - 20.0);
                            } else {
                                max_y = max_y.max(cur_y + 20.0);
                            }
                        }
                        "stop" => {
                            if !active_slurs.contains_key(number) {
                                // Start not seen yet — remember where this stop is,
                                // for both placements, until the start shows up.
                                let stem_down = stem.is_some_and(|s| !s.is_up);
                                let below = if stem_down {
                                    (stem.unwrap().x, stem.unwrap().y_tip + 6.0)
                                } else {
                                    (notehead_x, below_y_raw)
                                };
                                let above = match stem.filter(|s| s.is_up) {
                                    Some(st) => (st.x, st.y_tip - 6.0),
                                    None => (notehead_x, above_y_raw),
                                };
                                active_slurs.insert(
                                    PENDING_SLUR_STOP_KEY + *number,
                                    SlurStartInfo {
                                        x: notehead_x,
                                        y: above_y_raw,
                                        is_above: true,
                                        is_continuation: false,
                                        staff: note_staff,
                                        obstacles: Vec::new(),
                                        stem_anchor: false,
                                        pending_stop: Some(PendingSlurStop {
                                            above,
                                            below,
                                            staff: note_staff,
                                            top: min_pitch_y,
                                            bottom: max_pitch_y,
                                        }),
                                        known_end: None,

                                        resumed: false,
                                    },
                                );
                            }
                            if let Some(start_info) = active_slurs.remove(number) {
                                // Same outermost-element anchoring as the "start" arm.
                                let end_stem_up = stem.map(|s| s.is_up).unwrap_or(false);
                                let use_stem_tip = start_info.is_above && end_stem_up;
                                let use_stem_tip_below =
                                    !start_info.is_above && stem.is_some_and(|s| !s.is_up);
                                // If this note's beam group was just completed, use its
                                // final (beam-adjusted) stem tip.
                                let beamed = stem.and_then(|st| {
                                    beam_tips.iter().find(|b| (b.orig_x - st.x).abs() < 0.5)
                                });
                                let (cur_x, cur_y) = if let (true, Some(b)) =
                                    (use_stem_tip || use_stem_tip_below, beamed)
                                {
                                    (b.x, Self::slur_anchor_at_beam(b, start_info.is_above))
                                } else if use_stem_tip {
                                    (stem.unwrap().x, stem.unwrap().y_tip - 6.0)
                                } else if use_stem_tip_below {
                                    (stem.unwrap().x, stem.unwrap().y_tip + 6.0)
                                } else if start_info.is_above {
                                    (notehead_x, above_y_raw)
                                } else {
                                    (notehead_x, below_y_raw)
                                };
                                // Draw it at the end of the measure (flush_known_end_slurs):
                                // other voices' notes under the slur may still follow in
                                // the file, and must be cleared too.
                                let obstacles = start_info.obstacles.clone();
                                let mut key = KNOWN_SLUR_END_KEY + *number * 64;
                                while active_slurs.contains_key(&key) {
                                    key += 1;
                                }
                                active_slurs.insert(
                                    key,
                                    SlurStartInfo {
                                        known_end: Some((
                                            cur_x,
                                            cur_y,
                                            note_staff,
                                            min_pitch_y,
                                            max_pitch_y,
                                        )),
                                        ..start_info.clone()
                                    },
                                );
                                let obs_top = obstacles
                                    .iter()
                                    .map(|o| o.1)
                                    .fold(f32::MAX, f32::min);
                                let obs_bottom = obstacles
                                    .iter()
                                    .map(|o| o.1)
                                    .fold(f32::MIN, f32::max);
                                let slur_top = start_info.y.min(cur_y).min(obs_top) - 20.0;
                                let slur_bottom =
                                    start_info.y.max(cur_y).max(obs_bottom) + 20.0;
                                min_y = min_y.min(slur_top);
                                max_y = max_y.max(slur_bottom);
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        (doc, min_y, max_y)
    }

    /// Moves the stems of already placed notes onto the final tips of the beam
    /// groups just drawn.
    /// Draws held beam-side marks whose beam has been drawn, just outside it;
    /// with `all`, also the rest at the height their note gave them.
    fn place_beam_marks(&self, mut doc: Document, beam_tips: &[BeamedStemTip], all: bool) -> Document {
        let mut marks = self.pending_beam_marks.borrow_mut();
        let mut kept = Vec::new();
        for m in marks.drain(..) {
            let tip = beam_tips
                .iter()
                .find(|b| (b.orig_x - m.stem_x).abs() < 0.5 && b.is_up == m.is_above);
            let y = match tip {
                Some(b) if m.is_above => Some(b.tip_y - m.gap),
                Some(b) => Some(b.tip_y + b.beam_thickness + m.gap),
                None if all => Some(m.fallback_y),
                None => None,
            };
            match y {
                Some(y) => {
                    self.placed_fingerings.borrow_mut().push((self.current_part_y.get(), m.x, y));
                    doc = doc.add(m.node.set("y", y));
                }
                None => kept.push(m),
            }
        }
        *marks = kept;
        doc
    }

    fn apply_beam_tips_to_placed_notes(&self, beam_tips: &[BeamedStemTip]) {
        let mut placed = self.placed_notes.borrow_mut();
        // The beams just drawn belong to the current part (another part's
        // staff of the same number may have a stem at the same x).
        let part_y = self.current_part_y.get();
        for b in beam_tips {
            for pn in placed.iter_mut().filter(|pn| pn.part_y == part_y) {
                if let Some((sx, tip, _)) = pn.stem.as_mut() {
                    if pn.staff == b.staff
                        && (*sx - b.orig_x).abs() < 0.5
                        && (*tip - b.tip_y).abs() < 120.0
                    {
                        *sx = b.x;
                        *tip = if b.is_up { b.tip_y } else { b.tip_y + b.beam_thickness };
                        pn.cross_beam = b.cross_staff;
                    }
                }
            }
        }
    }

    /// Draws the tuplets finished in this measure. The bracket (or number)
    /// sits just outside the notes at either end, tilted no more than 15°,
    /// then moved out as far as needed to clear every note in between.
    fn flush_pending_tuplets(&self, mut doc: Document) -> Document {
        let pending: Vec<PendingTuplet> = self.pending_tuplets.borrow_mut().drain(..).collect();
        for t in pending {
            let s = &t.start;
            let (y1, y2) = self.tuplet_line(&t);
            doc = self.draw_tuplet(
                doc, s.x, y1, t.x2, y2, s.is_above, s.number, s.bracket, s.show_number,
            );
            self.note_drawn_extent(y1.min(y2) - 10.0, y1.max(y2) + 10.0);
            let (x1, x2) = (s.x.min(t.x2), s.x.max(t.x2));
            let (top, bottom) = (y1.min(y2) - TUPLET_REACH, y1.max(y2) + TUPLET_REACH);
            self.placed_tuplets.borrow_mut().push((
                self.current_part_y.get(),
                s.staff,
                x1,
                x2,
                top,
                bottom,
            ));
            // A bracket in the gap between staves: the other staff's hairpins
            // and texts there clear it (by widening the gap if need be). Not
            // one over a beam joining both staves: it sits on that beam, which
            // moves with the gap, so widening the gap could never clear it.
            let on_cross_beam = self.placed_notes.borrow().iter().any(|pn| {
                pn.cross_beam
                    && pn.part_y == self.current_part_y.get()
                    && pn.x >= x1 - 8.0
                    && pn.x <= x2 + 8.0
            });
            if !on_cross_beam {
                self.push_gap_item(s.staff, !s.is_above, x1, x2, top, bottom);
            }
        }
        doc
    }

    /// Heights (at its start and stop x) of a finished tuplet's bracket line:
    /// just outside the notes under it on its side, as their stems and beams
    /// are drawn now. Slurs go outside the bracket, not the other way round.
    fn tuplet_line(&self, t: &PendingTuplet) -> (f32, f32) {
        const END_GAP: f32 = 10.0;
        const CLEARANCE: f32 = 10.0;
        let s = &t.start;
        let (x1, x2) = (s.x.min(t.x2), s.x.max(t.x2));
        let dir = if s.is_above { -1.0 } else { 1.0 };
        let outer = |a: f32, b: f32| if s.is_above { a.min(b) } else { a.max(b) };
        let near_top = s.top.min(t.top2) - 120.0;
        let near_bottom = s.bottom.max(t.bottom2) + 120.0;
        // Extreme points of the notes under the tuplet on its side.
        let mut pts: Vec<(f32, f32)> = Vec::new();
        for pn in self.placed_notes.borrow().iter() {
            if pn.part_y != self.current_part_y.get()
                || pn.staff != s.staff
                || pn.x < x1 - 8.0
                || pn.x > x2 + 8.0
                || pn.bottom < near_top
                || pn.top > near_bottom
            {
                continue;
            }
            pts.push((pn.x, if s.is_above { pn.top } else { pn.bottom }));
            if let Some((sx, tip, up)) = pn.stem {
                if up == s.is_above {
                    pts.push((sx, tip));
                }
            }
        }
        // The notes drawn at an end (with stems as their beam finally set
        // them); the extent recorded with the tuplet's start or stop note
        // predates the beam, so it only stands in when none was found.
        let end_extreme = |x: f32, own: f32| {
            pts.iter()
                .filter(|p| (p.0 - x).abs() < 8.0)
                .map(|p| p.1)
                .reduce(outer)
                .unwrap_or(own)
        };
        let own1 = if s.is_above { s.top } else { s.bottom };
        let own2 = if s.is_above { t.top2 } else { t.bottom2 };
        let offset = dir * (END_GAP + s.level as f32 * 10.0);
        let mut y1 = end_extreme(s.x, own1) + offset;
        let mut y2 = end_extreme(t.x2, own2) + offset;
        // Clamp the slope to 15° (like a beam), keeping the outer end.
        let dx = t.x2 - s.x;
        if dx.abs() > 0.1 {
            let max_dy = 0.2679 * dx.abs();
            if (y2 - y1).abs() > max_dy {
                if outer(y1, y2) == y1 {
                    y2 = y1 + (y2 - y1).signum() * max_dy;
                } else {
                    y1 = y2 + (y1 - y2).signum() * max_dy;
                }
            }
            // Move the whole line out until every note clears it.
            let mut shift: f32 = 0.0;
            for &(px, py) in &pts {
                let tt = ((px - s.x) / dx).clamp(0.0, 1.0);
                let line_y = y1 + (y2 - y1) * tt;
                let need = if s.is_above {
                    line_y - (py - CLEARANCE)
                } else {
                    (py + CLEARANCE) - line_y
                };
                shift = shift.max(need);
            }
            y1 += dir * shift;
            y2 += dir * shift;
        }
        (y1, y2)
    }

    /// Draws the slurs whose end was known before their start (see
    /// `SlurStartInfo::known_end`) now that the measure's notes are placed.
    fn flush_known_end_slurs(
        &self,
        mut doc: Document,
        active_slurs: &mut HashMap<i32, SlurStartInfo>,
    ) -> Document {
        let mut keys: Vec<i32> = active_slurs
            .iter()
            .filter(|(_, s)| s.known_end.is_some())
            .map(|(&k, _)| k)
            .collect();
        keys.sort();
        for k in keys {
            let slur = active_slurs.remove(&k).unwrap();
            let (end_x, end_y, end_staff, end_top, end_bottom) = slur.known_end.unwrap();
            let mut obstacles = slur.obstacles.clone();
            if end_staff != slur.staff {
                // A cross-staff slur heads into the gap toward the end staff:
                // start-staff elements well past its start point on the far
                // side (e.g. another voice's stems and beam hanging away from
                // that staff) are not under its path, and would drag the curve
                // right past them. Notes only a little beyond it (a melody
                // rising from the start note) still are.
                const FAR_SIDE: f32 = 30.0;
                let toward_up = end_staff < slur.staff;
                // (Judged from where the segment really starts: a resumed one
                // has moved out over its first notes.)
                let start_y = Self::resumed_slur_start_y(&slur, &obstacles);
                obstacles.retain(|o| {
                    if toward_up {
                        o.1 <= start_y + FAR_SIDE
                    } else {
                        o.1 >= start_y - FAR_SIDE
                    }
                });
                // End-staff notes phrased under a slur of their own that is
                // over before this one reaches that staff (after its last note
                // on the start staff) belong to the other hand's phrase played
                // meanwhile: they lie beyond its path, not under it.
                let handover = slur
                    .obstacles
                    .iter()
                    .map(|o| o.0)
                    .filter(|&ox| ox < end_x)
                    .fold(slur.x, f32::max);
                let part_y = self.current_part_y.get();
                let own_phrases: Vec<(f32, f32)> = self
                    .placed_slurs
                    .borrow()
                    .iter()
                    .filter(|sl| {
                        sl.staff == end_staff
                            && !sl.cross_staff
                            && (sl.part_y - part_y).abs() < 0.5
                            && sl.x_range().1 < handover
                    })
                    .map(|sl| sl.x_range())
                    .collect();
                obstacles.extend(
                    self.cross_staff_slur_obstacles(
                        end_staff, slur.x, end_x, end_top, end_bottom, slur.is_above,
                    )
                    .into_iter()
                    .filter(|o| !own_phrases.iter().any(|&(a, b)| o.0 >= a - 2.0 && o.0 <= b + 2.0)),
                );
            }
            let start_y = Self::resumed_slur_start_y(&slur, &obstacles);
            doc = self.draw_slur(
                doc, slur.x, start_y, end_x, end_y, slur.is_above, &obstacles, slur.staff,
            );
            if end_staff != slur.staff {
                if let Some(placed) = self.placed_slurs.borrow_mut().last_mut() {
                    placed.cross_staff = true;
                }
            }
        }
        doc
    }

    /// Extreme points, on the slur's side, of the notes already drawn on
    /// `staff` strictly between `x1` and `x2`, for a slur that ends on that
    /// staff. Notes are limited to the end note's neighbourhood (`near_top`
    /// .. `near_bottom` ± 120px) so the same staff number of another part is
    /// never picked up. Notes in line with either end (sounding with the
    /// start note on the other staff, or chord-mates of the end note) are not
    /// under the slur, and clearing one right beside an end would bend the
    /// curve far out.
    fn cross_staff_slur_obstacles(
        &self,
        staff: i32,
        x1: f32,
        x2: f32,
        near_top: f32,
        near_bottom: f32,
        is_above: bool,
    ) -> Vec<(f32, f32)> {
        let mut out = Vec::new();
        let part_y = self.current_part_y.get();
        let in_line = self.note_head_width + 2.0;
        for pn in self.placed_notes.borrow().iter() {
            if pn.part_y != part_y
                || pn.staff != staff
                || pn.x <= x1 + in_line
                || pn.x >= x2 - in_line
                || pn.bottom < near_top - 120.0
                || pn.top > near_bottom + 120.0
            {
                continue;
            }
            if is_above {
                let mut top = pn.top;
                if let Some((sx, tip, true)) = pn.stem {
                    out.push((sx, tip));
                    top = top.min(tip);
                }
                out.push((pn.x, top));
            } else {
                let mut bottom = pn.bottom;
                if let Some((sx, tip, false)) = pn.stem {
                    out.push((sx, tip));
                    bottom = bottom.max(tip);
                }
                out.push((pn.x, bottom));
            }
        }
        out
    }

    /// Vertical extent of the `<text>` elements under `children` whose y lies
    /// within `lo..hi`, merged into `acc`. Music-font glyphs (private-use
    /// characters such as noteheads or dots) are far smaller than their font
    /// size, so they only count a fraction of it.
    fn text_extent(
        children: &[Box<dyn svg::Node>],
        lo: f32,
        hi: f32,
        acc: &mut Option<(f32, f32)>,
    ) {
        for child in children {
            if child.get_name() == "text" {
                let attr = |k: &str| {
                    child
                        .get_attributes()
                        .and_then(|a| a.get(k))
                        .and_then(|v| v.to_string().parse::<f32>().ok())
                };
                let Some(y) = attr("y") else { continue };
                if !(y > lo && y < hi) {
                    continue;
                }
                let fs = attr("font-size").unwrap_or(12.0);
                let content: String = child
                    .get_children()
                    .map(|c| c.iter().map(|n| n.to_string()).collect())
                    .unwrap_or_default();
                let is_glyph = content
                    .trim()
                    .chars()
                    .next()
                    .is_some_and(|c| ('\u{E000}'..='\u{F8FF}').contains(&c));
                let central = child
                    .get_attributes()
                    .and_then(|a| a.get("dominant-baseline"))
                    .is_some_and(|v| v.to_string() == "central");
                let (top, bottom) = if is_glyph {
                    (y - fs * 0.2, y + fs * 0.2)
                } else if central {
                    (y - fs * 0.5, y + fs * 0.5)
                } else {
                    (y - fs * 0.8, y + fs * 0.25)
                };
                *acc = Some(match *acc {
                    Some((t, b)) => (t.min(top), b.max(bottom)),
                    None => (top, bottom),
                });
            } else if let Some(c) = child.get_children() {
                Self::text_extent(c, lo, hi, acc);
            }
        }
    }

    /// Widens the current system's late-placed element extent.
    fn note_drawn_extent(&self, top: f32, bottom: f32) {
        if !(top.is_finite() && bottom.is_finite() && top.abs() < 1e6 && bottom.abs() < 1e6) {
            return;
        }
        self.drawn_extent.set(Some(match self.drawn_extent.get() {
            Some((t, b)) => (t.min(top), b.max(bottom)),
            None => (top, bottom),
        }));
    }

    /// Slur endpoint height just outside a beamed stem's tip: 6px beyond the
    /// tip, plus the beam's thickness when the beam lies between the tip and
    /// the slur (down stems with the slur below).
    fn slur_anchor_at_beam(tip: &BeamedStemTip, is_above: bool) -> f32 {
        if is_above {
            tip.tip_y - 6.0
        } else {
            tip.tip_y + tip.beam_thickness + 6.0
        }
    }

    /// Moves open slurs' stem-tip start points, and the stem obstacles they
    /// recorded, onto the final stem tips of freshly drawn beam groups.
    fn reanchor_slurs_to_beams(
        active_slurs: &mut HashMap<i32, SlurStartInfo>,
        beam_tips: &[BeamedStemTip],
    ) {
        if beam_tips.is_empty() {
            return;
        }
        for slur in active_slurs.values_mut() {
            if slur.is_continuation || slur.pending_stop.is_some() {
                continue;
            }
            // Same side as the slur, and actually near it (a stray tip from a
            // differently positioned pass must never yank the slur away).
            let slur_y = slur.y;
            let on_slur_side = |b: &BeamedStemTip| {
                b.staff == slur.staff
                    && b.is_up == slur.is_above
                    && (b.tip_y - slur_y).abs() < 120.0
            };
            if slur.stem_anchor {
                if let Some(b) = beam_tips
                    .iter()
                    .find(|b| (b.orig_x - slur.x).abs() < 0.5 && on_slur_side(b))
                {
                    slur.x = b.x;
                    slur.y = Self::slur_anchor_at_beam(b, slur.is_above);
                }
            }
            for o in slur.obstacles.iter_mut() {
                if let Some(b) = beam_tips
                    .iter()
                    .find(|b| (b.orig_x - o.0).abs() < 0.5 && on_slur_side(b))
                {
                    o.0 = b.x;
                    o.1 = if slur.is_above {
                        b.tip_y
                    } else {
                        b.tip_y + b.beam_thickness
                    };
                }
            }
        }
    }

    /// Start height of a slur segment: for one resumed at a system start,
    /// moved outward to clear the notes it meets first (within 80px), as
    /// nothing anchors it there; otherwise the start point itself.
    fn resumed_slur_start_y(slur: &SlurStartInfo, obstacles: &[(f32, f32)]) -> f32 {
        if !slur.resumed {
            return slur.y;
        }
        obstacles
            .iter()
            .filter(|o| o.0 > slur.x && o.0 <= slur.x + 80.0)
            .fold(slur.y, |y, o| if slur.is_above { y.min(o.1 - 6.0) } else { y.max(o.1 + 6.0) })
    }

    /// Height at which a slur crossing a system break leaves this system: just
    /// outside the extreme point of the last note it spans (same 6px offset as
    /// a stem-tip anchor), or its start height when it spans no other note.
    fn slur_break_y(slur: &SlurStartInfo) -> f32 {
        let Some(last_x) = slur.obstacles.iter().map(|o| o.0).reduce(f32::max) else {
            return slur.y;
        };
        // The last stretch before the break (not just the very last note): an
        // obstacle right next to the break point can't be cleared by bending.
        let window = ((last_x - slur.x) * 0.15).clamp(8.0, 60.0);
        let last_notes = slur.obstacles.iter().filter(|o| last_x - o.0 < window).map(|o| o.1);
        if slur.is_above {
            last_notes.fold(f32::MAX, f32::min) - 6.0
        } else {
            last_notes.fold(f32::MIN, f32::max) + 6.0
        }
    }

    fn draw_slur(
        &self,
        doc: Document,
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        is_above: bool,
        obstacles: &[(f32, f32)],
        staff: i32,
    ) -> Document {
        // Never draw (or grow the system for) a slur with a bogus endpoint.
        if ![x1, y1, x2, y2].iter().all(|v| v.is_finite() && v.abs() < 1e6) {
            return doc;
        }
        // Slurs already drawn inside this one's span on its staff (a short
        // phrase within a long one) are passed over like notes: sample their
        // curves as obstacles, so the two don't cross.
        let nested: Vec<(f32, f32)> = self
            .placed_slurs
            .borrow()
            .iter()
            .filter(|sl| {
                !sl.cross_staff
                    && sl.staff == staff
                    && (sl.part_y - self.current_part_y.get()).abs() < 0.5
                    && sl.x1 >= x1 - 2.0
                    && sl.x2 <= x2 + 2.0
                    && (sl.x1 - x1).abs() + (sl.x2 - x2).abs() > 4.0
            })
            .flat_map(|sl| (1..8).map(move |i| {
                let x = sl.x1 + (sl.x2 - sl.x1) * i as f32 / 8.0;
                (x, sl.y_at(x))
            }))
            .collect();
        // Tuplet brackets under the slur, drawn or still waiting for the
        // measure's end: the slur goes over them (notes, then brackets, then
        // slurs, outward). Each as (x1, x2, edge y at x1, edge y at x2), the
        // edge being the one on the slur's side; a bracket on the other side
        // of the notes lies inside the slur's curve anyway.
        let part_y = self.current_part_y.get();
        let reach = if is_above { -TUPLET_REACH } else { TUPLET_REACH };
        let mut bracket_segs: Vec<(f32, f32, f32, f32)> = Vec::new();
        for &(t_part_y, t_staff, tx1, tx2, top, bottom) in self.placed_tuplets.borrow().iter() {
            if (t_part_y - part_y).abs() < 0.5 && t_staff == staff && tx1 < x2 && tx2 > x1 {
                let edge = if is_above { top } else { bottom };
                bracket_segs.push((tx1, tx2, edge, edge));
            }
        }
        for t in self.pending_tuplets.borrow().iter() {
            let st = &t.start;
            if st.staff != staff || st.x.min(t.x2) >= x2 || st.x.max(t.x2) <= x1 {
                continue;
            }
            let (ty1, ty2) = self.tuplet_line(t);
            bracket_segs.push((st.x, t.x2, ty1 + reach, ty2 + reach));
        }
        let brackets: Vec<(f32, f32)> = bracket_segs
            .iter()
            .flat_map(|&(bx1, bx2, by1, by2)| {
                (0..=4).map(move |i| {
                    let f = i as f32 / 4.0;
                    (bx1 + (bx2 - bx1) * f, by1 + (by2 - by1) * f)
                })
            })
            .collect();
        // An end inside a bracket's span starts beyond the bracket too, so the
        // slur doesn't cut through its hooks.
        let (x1, y1, x2, y2) = {
            let beyond = |x: f32, y: f32| {
                bracket_segs
                    .iter()
                    .filter(|&&(bx1, bx2, _, _)| x >= bx1.min(bx2) - 2.0 && x <= bx1.max(bx2) + 2.0)
                    .map(|&(bx1, bx2, by1, by2)| {
                        let f = if (bx2 - bx1).abs() > 0.1 { ((x - bx1) / (bx2 - bx1)).clamp(0.0, 1.0) } else { 0.0 };
                        by1 + (by2 - by1) * f + if is_above { -3.0 } else { 3.0 }
                    })
                    .fold(y, |acc, e| if is_above { acc.min(e) } else { acc.max(e) })
            };
            (x1, beyond(x1, y1), x2, beyond(x2, y2))
        };
        let merged: Vec<(f32, f32)>;
        let obstacles = if nested.is_empty() && brackets.is_empty() {
            obstacles
        } else {
            merged = obstacles.iter().copied().chain(nested).chain(brackets).collect();
            &merged[..]
        };
        let dx = x2 - x1;
        let dy = y2 - y1;
        let distance = (dx * dx + dy * dy).sqrt();
        // Long slurs stay fairly flat by default; obstacles raise them as needed.
        let height = (distance * 0.15).max(10.0).min(25.0);
        let direction = if is_above { -1.0 } else { 1.0 };
        // What sits at an end's own x and beyond both ends (another voice's
        // note and stem on the end note's beat) is past where the curve ends:
        // clearing it would take an absurdly deep arc, so leave it out.
        let ends_outer = if is_above { y1.min(y2) } else { y1.max(y2) };
        let beside_end: Vec<(f32, f32)> = obstacles
            .iter()
            .copied()
            .filter(|o| {
                let at_end = (o.0 - x1).abs() < 8.0 || (o.0 - x2).abs() < 8.0;
                !(at_end && direction * (o.1 - ends_outer) > 0.0)
            })
            .collect();
        let obstacles = &beside_end[..];

        let outward = |a: f32, b: f32| if is_above { a.min(b) } else { a.max(b) };
        // The control points sit `h` beyond a base height: the outer endpoint's
        // for a gently sloped slur, or the chord line under each control point
        // for a steep one (ends far apart in height), which then arcs along
        // the line instead of rising over the higher end.
        let steep = dy.abs() > 40.0;
        let bases = |y1: f32, y2: f32, cx1: f32, cx2: f32| -> (f32, f32) {
            if steep && dx > 1.0 {
                let line = |x: f32| y1 + (x - x1) / dx * (y2 - y1);
                (line(cx1), line(cx2))
            } else {
                let b = outward(y1, y2);
                (b, b)
            }
        };

        // Offset `h` needed for the curve to clear every intermediate
        // obstacle. With control heights b1 + d·h and b2 + d·h,
        // y(t) = E(t) + 3t(1-t)·d·h where E(t) is the curve with h = 0, and
        // x(t) is monotonic in t, so each obstacle gives a required h.
        const SLUR_CLEARANCE: f32 = 4.0;
        let required_h = |y1: f32, y2: f32, cx1: f32, cx2: f32| -> Option<f32> {
            let mut req: Option<f32> = None;
            if dx <= 1.0 {
                return None;
            }
            let (b1, b2) = bases(y1, y2, cx1, cx2);
            for &(ox, oy) in obstacles {
                if ox <= x1 || ox >= x2 {
                    continue;
                }
                let (mut lo, mut hi) = (0.0_f32, 1.0_f32);
                for _ in 0..24 {
                    let t = 0.5 * (lo + hi);
                    let u = 1.0 - t;
                    let xt = u * u * u * x1
                        + 3.0 * u * u * t * cx1
                        + 3.0 * u * t * t * cx2
                        + t * t * t * x2;
                    if xt < ox {
                        lo = t;
                    } else {
                        hi = t;
                    }
                }
                let t = (0.5 * (lo + hi)).clamp(0.08, 0.92);
                let u = 1.0 - t;
                let e = u * u * u * y1 + 3.0 * u * u * t * b1 + 3.0 * u * t * t * b2 + t * t * t * y2;
                let h = (SLUR_CLEARANCE - direction * (e - oy)) / (3.0 * u * t);
                req = Some(req.map_or(h, |r: f32| r.max(h)));
            }
            req
        };

        let (mut y1, mut y2) = (y1, y2);
        // An obstacle right beside an end and far beyond it (a beam or stem
        // the slur must pass just before landing on a much lower / higher
        // note) can't be cleared by arching: the curve would shoot off the
        // page. Move that end out past it instead.
        {
            const NEAR_END_GAP: f32 = 6.0;
            const FAR_BEYOND: f32 = 20.0;
            let near = (dx * 0.08).max(30.0);
            // Only an obstacle the curve passes on its way down (up) to the
            // lower (higher) end: one beyond both ends (another voice's note
            // and stem out past the slur) is nowhere near its path.
            let outer_end = outward(y1, y2);
            let past = |y: f32, lo: f32, hi: f32| {
                let need = obstacles
                    .iter()
                    .filter(|o| o.0 > lo && o.0 < hi)
                    .filter(|o| direction * (o.1 - outer_end) <= 0.0)
                    .map(|o| o.1 + direction * NEAR_END_GAP)
                    .fold(y, outward);
                if (need - y).abs() > FAR_BEYOND { need } else { y }
            };
            y1 = past(y1, x1, x1 + near);
            y2 = past(y2, x2 - near, x2);
        }
        let mut cx1 = x1 + dx * 0.25;
        let mut cx2 = x2 - dx * 0.25;
        let mut h = height;
        if let Some(req) = required_h(y1, y2, cx1, cx2) {
            if req > h {
                // An obstacle pokes through the default arc. Rather than just
                // pulling the apex up into a narrow spike, flatten the curve
                // (control points closer to the ends) and lift both endpoints a
                // little, then solve for the height again.
                let excess = (req - h) * 0.75;
                let lift = (excess * 0.35).min(8.0);
                y1 += direction * lift;
                y2 += direction * lift;
                cx1 = x1 + dx * 0.12;
                cx2 = x2 - dx * 0.12;
                if let Some(req) = required_h(y1, y2, cx1, cx2) {
                    h = h.max(req);
                }
            }
        }
        let (b1, b2) = bases(y1, y2, cx1, cx2);
        let cy1 = b1 + direction * h;
        let cy2 = b2 + direction * h;
        if std::env::var("SLDBG").is_ok() {
            for &(ox, oy) in obstacles {
                if ox <= x1 || ox >= x2 { continue; }
                let (mut lo, mut hi) = (0.0_f32, 1.0_f32);
                for _ in 0..30 { let t = 0.5*(lo+hi); let u = 1.0-t;
                    let xt = u*u*u*x1 + 3.0*u*u*t*cx1 + 3.0*u*t*t*cx2 + t*t*t*x2;
                    if xt < ox { lo = t } else { hi = t } }
                let t = 0.5*(lo+hi); let u = 1.0-t;
                let yt = u*u*u*y1 + 3.0*u*u*t*cy1 + 3.0*u*t*t*cy2 + t*t*t*y2;
                let pen = if is_above { yt - oy } else { oy - yt };
                if pen > 0.0 { eprintln!("SLDBG pen={:.1} x1={:.0} x2={:.0} ox={:.0} oy={:.0} t={:.2} above={}", pen, x1, x2, ox, oy, t, is_above); }
            }
        }

        // Record the curve's true vertical extent (sampled) so the system
        // slice can be grown to contain it.
        let (mut top, mut bottom) = (y1.min(y2), y1.max(y2));
        for i in 1..16 {
            let t = i as f32 / 16.0;
            let u = 1.0 - t;
            let yt = u * u * u * y1 + 3.0 * u * u * t * cy1 + 3.0 * u * t * t * cy2 + t * t * t * y2;
            top = top.min(yt);
            bottom = bottom.max(yt);
        }
        let half_stroke = 0.75;
        self.note_drawn_extent(top - half_stroke, bottom + half_stroke);
        self.placed_slurs.borrow_mut().push(PlacedSlur {
            x1,
            y1,
            cx1,
            cx2,
            cy1,
            cy2,
            x2,
            y2,
            top: top - half_stroke,
            bottom: bottom + half_stroke,
            cross_staff: false,
            staff,
            part_y: self.current_part_y.get(),
        });

        let path = Path::new()
            .set("fill", "none")
            .set("stroke", "black")
            .set("stroke-width", 1.5)
            .set(
                "d",
                Data::new()
                    .move_to((x1, y1))
                    .cubic_curve_to((cx1, cy1, cx2, cy2, x2, y2)),
            );
        doc.add(path)
    }

    fn draw_beam_group(
        &self,
        mut doc: Document,
        pts: &[(StemInfo, Vec<crate::models::Beam>)],
        is_cue: bool,
    ) -> Document {
        if pts.is_empty() {
            return doc;
        }
        // Determine beam direction by majority vote across the group's own
        // (pre-correction) per-note directions — except for a cross-staff
        // beam (e.g. a piano run crossing from the bass clef staff into the
        // treble clef staff), where each note keeps its own direction based
        // on which staff it's on: the topmost staff (lowest staff number,
        // e.g. treble clef) stems down, any other (lower/bass) staff stems
        // up — so stems from both staves reach toward a shared beam line
        // sitting in the gap between the staves.
        //
        // A single staff's group must never mix directions: majority (not
        // "whichever note happens to be first") matches standard engraving
        // practice, where the notehead farthest from the middle line governs
        // the whole beam; ties keep the first note's direction so a passage
        // centred on the middle line doesn't flip unpredictably.
        let is_cross_staff = pts.iter().any(|(s, _)| s.staff != pts[0].0.staff);
        let top_staff = pts
            .iter()
            .map(|(s, _)| s.staff)
            .min()
            .unwrap_or(pts[0].0.staff);
        let up_votes = pts.iter().filter(|(s, _)| s.is_up).count();
        let down_votes = pts.len() - up_votes;
        let group_is_up = match up_votes.cmp(&down_votes) {
            std::cmp::Ordering::Greater => true,
            std::cmp::Ordering::Less => false,
            std::cmp::Ordering::Equal => pts[0].0.is_up,
        };
        // Stem edge sits 5.8 px from the notehead centre for regular notes.
        // Individual notes are drawn before the beam group direction is resolved,
        // so a note whose own is_up differs from the group's target direction
        // will have its stem_x on the wrong side.  Re-derive the note centre
        // from each stored stem_x and recompute stem_x using that direction.
        let stem_offset = 5.8_f32;
        let orig_xs: Vec<f32> = pts.iter().map(|(s, _)| s.x).collect();
        let corrected_pts: Vec<(StemInfo, Vec<crate::models::Beam>)> = pts
            .iter()
            .map(|(s, b)| {
                let note_is_up = if is_cross_staff {
                    s.staff != top_staff
                } else {
                    group_is_up
                };
                let note_center = if s.is_up {
                    s.x - stem_offset
                } else {
                    s.x + stem_offset
                };
                let cx = if note_is_up {
                    note_center + stem_offset
                } else {
                    note_center - stem_offset
                };
                let correct_y_start = if note_is_up { s.y_bottom } else { s.y_top };
                // y_tip direction must also match note_is_up; derive the original stem
                // length and recompute y_tip for that direction.
                let stem_len = if s.is_up {
                    (s.y_top - s.y_tip).abs()
                } else {
                    (s.y_tip - s.y_bottom).abs()
                };
                let correct_y_tip = if note_is_up {
                    s.y_top - stem_len
                } else {
                    s.y_bottom + stem_len
                };
                (
                    StemInfo {
                        x: cx,
                        y_start: correct_y_start,
                        y_tip: correct_y_tip,
                        is_up: note_is_up,
                        ..*s
                    },
                    b.clone(),
                )
            })
            .collect();
        let pts = corrected_pts.as_slice();

        let first_s = pts[0].0;
        let last_s = pts[pts.len() - 1].0;
        let ref_x1 = first_s.x;
        let ref_x2 = last_s.x;

        let (mut ref_y1, mut ref_y2, mut slope) = if is_cross_staff {
            // The generic "extend each endpoint by a full nominal stem
            // length" logic below assumes one uniform direction and easily
            // overshoots past the other staff when the inter-staff gap is
            // narrower than a standard stem — which it usually is. Instead,
            // place a flat beam line at the midpoint between the two staves'
            // closest-to-the-gap noteheads, so every stem (up from below,
            // down from above) reaches toward it without crossing over.
            let bass_closest = pts
                .iter()
                .filter(|(s, _)| s.staff != top_staff)
                .map(|(s, _)| s.y_start)
                .fold(f32::MAX, f32::min);
            let treble_closest = pts
                .iter()
                .filter(|(s, _)| s.staff == top_staff)
                .map(|(s, _)| s.y_start)
                .fold(f32::MIN, f32::max);
            let mid_y = (bass_closest + treble_closest) * 0.5;
            (mid_y, mid_y, 0.0)
        } else {
            (first_s.y_tip, last_s.y_tip, 0.0)
        };

        // 1. Calculate primary slope and apply min stem length adjustment
        let dx = ref_x2 - ref_x1;
        if !is_cross_staff {
            slope = if dx.abs() > 0.1 {
                (ref_y2 - ref_y1) / dx
            } else {
                0.0
            };
        }

        // Clamp slope to max 15 degrees (tan(15 deg) ≈ 0.2679)
        let max_slope = 0.2679;
        if slope > max_slope {
            slope = max_slope;
            ref_y2 = ref_y1 + slope * dx;
        } else if slope < -max_slope {
            slope = -max_slope;
            ref_y2 = ref_y1 + slope * dx;
        }

        // The min-stem-length enforcement below assumes shifting the whole
        // beam line one direction lengthens every stem — true for a normal
        // same-direction beam, but not for a cross-staff one where shifting
        // toward one staff shortens stems on the other side. Skip it there;
        // the midpoint placement above already keeps stems reasonable.
        let mut max_needed_shift: f32 = 0.0;
        let min_stem_len = 3.0 * self.staff_line_distance;
        let min_near_len = 2.5 * self.staff_line_distance;
        if !is_cross_staff {
            for (s_item, _) in pts.iter() {
                let tip_y = ref_y1 + (s_item.x - ref_x1) * slope;
                // Signed in the group's own direction (not `.abs()`), so a
                // beam that has drifted to the wrong side of a notehead —
                // e.g. a sharply-clamped slope leaving the beam above a note
                // whose group direction is "down" — reads as a large deficit
                // and gets corrected, instead of `.abs()` treating a
                // wrong-direction stem the same as a merely-short one of the
                // same length. Without this, a note whose beam-line tip ends
                // up on the wrong side of its notehead renders with its stem
                // visually pointing the opposite way from the rest of the
                // group, even though every note still has the same internal
                // is_up.
                let current_len = if first_s.is_up {
                    s_item.y_start - tip_y
                } else {
                    tip_y - s_item.y_start
                };
                if current_len < min_stem_len {
                    max_needed_shift = max_needed_shift.max(min_stem_len - current_len);
                }
                // A chord's stem starts at its far notehead; the beam must also
                // clear the notehead nearest to it (e.g. the top of a wide
                // chord under an up-beam) by a reasonable stem length.
                let near_len = if first_s.is_up {
                    s_item.y_top - tip_y
                } else {
                    tip_y - s_item.y_bottom
                };
                if near_len < min_near_len {
                    max_needed_shift = max_needed_shift.max(min_near_len - near_len);
                }
            }
        }
        if max_needed_shift > 0.0 {
            if first_s.is_up {
                ref_y1 -= max_needed_shift;
                ref_y2 -= max_needed_shift;
            } else {
                ref_y1 += max_needed_shift;
                ref_y2 += max_needed_shift;
            }
            slope = if dx.abs() > 0.1 {
                (ref_y2 - ref_y1) / dx
            } else {
                0.0
            };
        }

        // 2. Draw beams for each level (1 to 8)
        // Every beam segment drawn, as (x1, x2, offset from the primary beam),
        // so each stem can reach the farthest beam at its x (see below).
        let mut beam_spans: Vec<(f32, f32, f32)> = Vec::new();
        for beam_num in 1..=8 {
            let mut start_idx: Option<usize> = None;
            for i in 0..pts.len() {
                let (s_info, beams) = &pts[i];
                if let Some(beam) = beams.iter().find(|b| b.number == beam_num) {
                    match beam.value {
                        BeamValue::Begin => start_idx = Some(i),
                        BeamValue::Continue => {
                            if start_idx.is_none() {
                                start_idx = Some(i);
                            }
                        }
                        BeamValue::End => {
                            if let Some(si) = start_idx {
                                let x1 = pts[si].0.x;
                                let x2 = s_info.x;
                                let beam_thickness = if is_cue {
                                    self.staff_line_distance * 0.35
                                } else {
                                    self.staff_line_distance * 0.5
                                };
                                let multi_beam_offset = if is_cue {
                                    (beam_num - 1) as f32 * self.staff_line_distance * 0.5
                                } else {
                                    (beam_num - 1) as f32 * self.staff_line_distance * 0.75
                                };
                                let offset_y = if s_info.is_up {
                                    multi_beam_offset
                                } else {
                                    -multi_beam_offset
                                };

                                let draw_y1 = ref_y1 + (x1 - ref_x1) * slope + offset_y;
                                let draw_y2 = ref_y1 + (x2 - ref_x1) * slope + offset_y;

                                let beam_path = Path::new().set("fill", "black").set(
                                    "d",
                                    Data::new()
                                        .move_to((x1, draw_y1))
                                        .line_to((x2, draw_y2))
                                        .line_to((x2, draw_y2 + beam_thickness))
                                        .line_to((x1, draw_y1 + beam_thickness))
                                        .close(),
                                );
                                beam_spans.push((x1.min(x2), x1.max(x2), offset_y));
                                doc = doc.add(beam_path);
                                start_idx = None;
                            }
                        }
                        BeamValue::ForwardHook | BeamValue::BackwardHook => {
                            let hook_length = if is_cue { 7.0 } else { 10.0 };
                            let x1 = if beam.value == BeamValue::ForwardHook {
                                s_info.x
                            } else {
                                s_info.x - hook_length
                            };
                            let x2 = if beam.value == BeamValue::ForwardHook {
                                s_info.x + hook_length
                            } else {
                                s_info.x
                            };
                            let beam_thickness = if is_cue {
                                self.staff_line_distance * 0.35
                            } else {
                                self.staff_line_distance * 0.5
                            };
                            let multi_beam_offset = if is_cue {
                                (beam_num - 1) as f32 * self.staff_line_distance * 0.5
                            } else {
                                (beam_num - 1) as f32 * self.staff_line_distance * 0.75
                            };
                            let offset_y = if s_info.is_up {
                                multi_beam_offset
                            } else {
                                -multi_beam_offset
                            };

                            let draw_y1 = ref_y1 + (x1 - ref_x1) * slope + offset_y;
                            let draw_y2 = ref_y1 + (x2 - ref_x1) * slope + offset_y;

                            let beam_path = Path::new().set("fill", "black").set(
                                "d",
                                Data::new()
                                    .move_to((x1, draw_y1))
                                    .line_to((x2, draw_y2))
                                    .line_to((x2, draw_y2 + beam_thickness))
                                    .line_to((x1, draw_y1 + beam_thickness))
                                    .close(),
                            );
                            beam_spans.push((x1.min(x2), x1.max(x2), offset_y));
                            doc = doc.add(beam_path);
                        }
                    }
                } else {
                    // Beam level missing for this note, if we were in a segment, end it at PREVIOUS note.
                    if let Some(si) = start_idx {
                        let x1 = pts[si].0.x;
                        let x2 = pts[i - 1].0.x;
                        let beam_thickness = if is_cue {
                            self.staff_line_distance * 0.35
                        } else {
                            self.staff_line_distance * 0.5
                        };
                        let multi_beam_offset = if is_cue {
                            (beam_num - 1) as f32 * self.staff_line_distance * 0.5
                        } else {
                            (beam_num - 1) as f32 * self.staff_line_distance * 0.75
                        };
                        let offset_y = if first_s.is_up {
                            multi_beam_offset
                        } else {
                            -multi_beam_offset
                        };
                        let draw_y1 = ref_y1 + (x1 - ref_x1) * slope + offset_y;
                        let draw_y2 = ref_y1 + (x2 - ref_x1) * slope + offset_y;
                        let beam_path = Path::new().set("fill", "black").set(
                            "d",
                            Data::new()
                                .move_to((x1, draw_y1))
                                .line_to((x2, draw_y2))
                                .line_to((x2, draw_y2 + beam_thickness))
                                .line_to((x1, draw_y1 + beam_thickness))
                                .close(),
                        );
                        beam_spans.push((x1.min(x2), x1.max(x2), offset_y));
                        doc = doc.add(beam_path);
                        start_idx = None;
                    }
                }
            }
            // Handle trailing segment if loop ended
            if let Some(si) = start_idx {
                let x1 = pts[si].0.x;
                let x2 = pts[pts.len() - 1].0.x;
                let beam_thickness = if is_cue {
                    self.staff_line_distance * 0.35
                } else {
                    self.staff_line_distance * 0.5
                };
                let multi_beam_offset = if is_cue {
                    (beam_num - 1) as f32 * self.staff_line_distance * 0.5
                } else {
                    (beam_num - 1) as f32 * self.staff_line_distance * 0.75
                };
                let offset_y = if first_s.is_up {
                    multi_beam_offset
                } else {
                    -multi_beam_offset
                };
                let draw_y1 = ref_y1 + (x1 - ref_x1) * slope + offset_y;
                let draw_y2 = ref_y1 + (x2 - ref_x1) * slope + offset_y;
                let beam_path = Path::new().set("fill", "black").set(
                    "d",
                    Data::new()
                        .move_to((x1, draw_y1))
                        .line_to((x2, draw_y2))
                        .line_to((x2, draw_y2 + beam_thickness))
                        .line_to((x1, draw_y1 + beam_thickness))
                        .close(),
                );
                beam_spans.push((x1.min(x2), x1.max(x2), offset_y));
                doc = doc.add(beam_path);
            }
        }

        // 3. Redraw all stems to meet the primary beam
        let primary_beam_thickness = if is_cue {
            self.staff_line_distance * 0.35
        } else {
            self.staff_line_distance * 0.5
        };
        for (i, (s_item, _)) in pts.iter().enumerate() {
            let stem_tip_y = ref_y1 + (s_item.x - ref_x1) * slope;
            // In a beam mixing stem directions (across staves), the secondary
            // beams lie on the tip side of the stems pointing their way: those
            // stems run on through them to the farthest one.
            let reach = beam_spans
                .iter()
                .filter(|&&(x1, x2, _)| s_item.x >= x1 - 0.5 && s_item.x <= x2 + 0.5)
                .map(|&(_, _, off)| off)
                .fold(0.0_f32, |acc, off| if s_item.is_up { acc.min(off) } else { acc.max(off) });
            let drawn_tip_y = stem_tip_y + reach;
            self.beamed_stem_tips.borrow_mut().push(BeamedStemTip {
                orig_x: orig_xs[i],
                x: s_item.x,
                tip_y: stem_tip_y,
                is_up: s_item.is_up,
                beam_thickness: primary_beam_thickness,
                staff: s_item.staff,
                cross_staff: is_cross_staff,
            });
            let stroke_w = if is_cue { 0.8 } else { 1.2 };
            doc = doc.add(
                Line::new()
                    .set("x1", s_item.x)
                    .set("y1", s_item.y_start)
                    .set("x2", s_item.x)
                    .set("y2", drawn_tip_y)
                    .set("stroke", "black")
                    .set("stroke-width", stroke_w),
            );
        }

        doc
    }

    fn flush_active_beams(
        &self,
        mut doc: Document,
        active_beams: &mut HashMap<(i32, i32), Vec<(StemInfo, Vec<crate::models::Beam>)>>,
    ) -> Document {
        let keys: Vec<(i32, i32)> = active_beams.keys().copied().collect();
        for key in keys {
            if let Some(pts) = active_beams.remove(&key) {
                if !pts.is_empty() {
                    let is_cue = pts[0].0.is_cue;
                    doc = self.draw_beam_group(doc, &pts, is_cue);
                }
            }
        }
        doc
    }

    // Like flush_active_beams, but only closes groups belonging to `voice`.
    // An unbeamed note (or a beam-less rest that really does end a beam) in
    // one voice must not prematurely close a still-open beam in another
    // voice sharing the same staff.
    fn flush_active_beams_for_voice(
        &self,
        mut doc: Document,
        active_beams: &mut HashMap<(i32, i32), Vec<(StemInfo, Vec<crate::models::Beam>)>>,
        voice: i32,
    ) -> Document {
        let keys: Vec<(i32, i32)> = active_beams
            .keys()
            .copied()
            .filter(|(v, _)| *v == voice)
            .collect();
        for key in keys {
            if let Some(pts) = active_beams.remove(&key) {
                if !pts.is_empty() {
                    let is_cue = pts[0].0.is_cue;
                    doc = self.draw_beam_group(doc, &pts, is_cue);
                }
            }
        }
        doc
    }

    fn process_beams(
        &self,
        mut doc: Document,
        notes: &[&Note],
        stem: Option<StemInfo>,
        active_beams: &mut HashMap<(i32, i32), Vec<(StemInfo, Vec<crate::models::Beam>)>>,
    ) -> Document {
        if let (Some(s), Some(first_note)) = (stem, notes.first()) {
            let voice = first_note.voice.unwrap_or(1);
            if first_note.beams.is_empty() {
                return self.flush_active_beams_for_voice(doc, active_beams, voice);
            }

            let group_id = first_note.beams.iter().map(|b| b.number).min().unwrap_or(1);
            let key = (voice, group_id);
            let pts = active_beams.entry(key).or_insert(Vec::new());
            pts.push((s, first_note.beams.clone()));

            // The group is keyed by `group_id` (the lowest beam-level number), so whether
            // the group ends must be decided by that same level's value. A hook on a
            // higher level (e.g. a 16th-note sub-beam) must not end the group early when
            // the primary level is still Begin/Continue.
            let is_end = first_note
                .beams
                .iter()
                .find(|b| b.number == group_id)
                .is_some_and(|b| {
                    b.value == BeamValue::End
                        || b.value == BeamValue::ForwardHook
                        || b.value == BeamValue::BackwardHook
                });

            if is_end
                || (pts.len() == 1
                    && !first_note
                        .beams
                        .iter()
                        .any(|b| b.value == BeamValue::Begin || b.value == BeamValue::Continue))
            {
                doc = self.draw_beam_group(doc, pts, first_note.is_cue);
                active_beams.remove(&key);
            }
        } else if notes.first().is_some_and(|n| n.rest && n.beams.is_empty()) {
            // A rest has no stem, so it always lands here — but a rest with
            // no <beam> tag of its own commonly means "sits silently inside
            // an already-open beam span" (exporters routinely omit beam
            // elements on embedded rests, relying on the surrounding
            // Begin/End to imply the beam bridges over it), not "the beam
            // ends here". Leave any in-progress groups open so the next
            // beamed note can still close them; only a genuine unbeamed
            // note should force a flush.
        } else if let Some(voice) = notes.first().and_then(|n| n.voice) {
            doc = self.flush_active_beams_for_voice(doc, active_beams, voice);
        } else {
            doc = self.flush_active_beams(doc, active_beams);
        }
        doc
    }

    fn draw_flag(
        &self,
        mut doc: Document,
        x: f32,
        y: f32,
        note_type: &str,
        is_up: bool,
        is_cue: bool,
    ) -> Document {
        let symbol = match (note_type, is_up) {
            ("eighth", true) => Some("\u{E240}"),
            ("eighth", false) => Some("\u{E241}"),
            ("16th", true) => Some("\u{E242}"),
            ("16th", false) => Some("\u{E243}"),
            ("32nd", true) => Some("\u{E244}"),
            ("32nd", false) => Some("\u{E245}"),
            ("64th", true) => Some("\u{E246}"),
            ("64th", false) => Some("\u{E247}"),
            _ => None,
        };
        let font_size = if is_cue {
            self.staff_line_distance * 2.8
        } else {
            self.staff_line_distance * 4.0
        };
        if let Some(sym) = symbol {
            doc = doc.add(
                Text::new(sym)
                    .set("x", x)
                    .set("y", y + 0.1 * self.staff_line_distance)
                    .set("font-size", font_size)
                    .set("dominant-baseline", "central")
                    .set("font-family", self.font_family.as_str()),
            );
        }
        doc
    }

    fn draw_accidental(
        &self,
        mut doc: Document,
        x: f32,
        y: f32,
        accidental: &str,
        is_cue: bool,
    ) -> Document {
        let symbol = match accidental {
            "sharp" => Some("\u{E262}"),
            "flat" => Some("\u{E260}"),
            "natural" => Some("\u{E261}"),
            "double-sharp" => Some("\u{E263}"),
            "flat-flat" => Some("\u{E264}"),
            _ => None,
        };
        let font_size = if is_cue {
            self.staff_line_distance * 2.1
        } else {
            self.staff_line_distance * 3.0
        };
        if let Some(sym) = symbol {
            doc = doc.add(
                Text::new(sym)
                    .set("x", x)
                    .set("y", y + 0.1 * self.staff_line_distance)
                    .set("font-size", font_size)
                    .set("text-anchor", "middle")
                    .set("dominant-baseline", "central")
                    .set("font-family", self.font_family.as_str()),
            );
        }
        doc
    }

    fn draw_microtonal_accidental(
        &self,
        mut doc: Document,
        x: f32,
        y: f32,
        alter: f32,
        is_cue: bool,
    ) -> Document {
        let symbol = if (alter - 0.5).abs() < 0.01 {
            Some("\u{E282}") // quarter-tone sharp (Stein-Zimmermann)
        } else if (alter - (-0.5)).abs() < 0.01 {
            Some("\u{E280}") // quarter-tone flat (Stein-Zimmermann)
        } else if (alter - 1.5).abs() < 0.01 {
            Some("\u{E283}") // three-quarter-tone sharp
        } else if (alter - (-1.5)).abs() < 0.01 {
            Some("\u{E281}") // three-quarter-tone flat
        } else {
            None
        };

        let font_size = if is_cue {
            self.staff_line_distance * 2.1
        } else {
            self.staff_line_distance * 3.0
        };
        if let Some(sym) = symbol {
            doc = doc.add(
                Text::new(sym)
                    .set("x", x)
                    .set("y", y + 0.1 * self.staff_line_distance)
                    .set("font-size", font_size)
                    .set("text-anchor", "middle")
                    .set("dominant-baseline", "central")
                    .set("font-family", self.font_family.as_str()),
            );
        }
        doc
    }

    fn draw_notehead(
        &self,
        doc: Document,
        x: f32,
        y: f32,
        note_type: &str,
        notehead: Option<&crate::models::Notehead>,
        is_cue: bool,
    ) -> Document {
        // Diamond glyphs (E0DB/E0DC/E0DD/E0DE) render taller than the oval
        // noteheads at the same font size — verified directly against the
        // Bravura font's own glyph bounds: black diamonds are 369 units tall,
        // white 312, vs. 250 for a normal notehead (1000 units/em) — which
        // made a diamond in a staff space poke through the adjacent ledger
        // line instead of just meeting it. Scale each variant down so its
        // rendered height matches a normal notehead's.
        let mut notehead_size_scale = 1.0_f32;
        let symbol = if let Some(nh) = notehead {
            match nh.value.as_str() {
                "diamond" => {
                    let is_half_or_longer =
                        note_type == "half" || note_type == "whole" || note_type == "breve";
                    let is_wide = note_type == "whole" || note_type == "breve";
                    let filled = nh.filled.unwrap_or(!is_half_or_longer);
                    if filled {
                        notehead_size_scale = 250.0 / 369.0;
                        if is_wide {
                            "\u{E0DC}" // noteheadDiamondBlackWide
                        } else {
                            "\u{E0DB}" // noteheadDiamondBlack
                        }
                    } else {
                        notehead_size_scale = 250.0 / 312.0;
                        if is_wide {
                            "\u{E0DE}" // noteheadDiamondWhiteWide
                        } else {
                            "\u{E0DD}" // noteheadDiamondWhite
                        }
                    }
                }
                "slash" => "\u{E101}",
                "x" => "\u{E0A9}",
                _ => match note_type {
                    "breve" => "\u{E0A1}",
                    "whole" => "\u{E0A2}",
                    "half" => "\u{E0A3}",
                    _ => "\u{E0A4}",
                },
            }
        } else {
            match note_type {
                "breve" => "\u{E0A1}",
                "whole" => "\u{E0A2}",
                "half" => "\u{E0A3}",
                _ => "\u{E0A4}",
            }
        };
        let font_size = (if is_cue {
            self.staff_line_distance * 2.8
        } else {
            self.staff_line_distance * 4.0
        }) * notehead_size_scale;
        let y_adj = if note_type == "half" {
            0.0
        } else {
            0.1 * self.staff_line_distance
        };
        doc.add(
            Text::new(symbol)
                .set("x", x)
                .set("y", y + y_adj)
                .set("font-size", font_size)
                .set("text-anchor", "middle")
                .set("dominant-baseline", "central")
                .set("font-family", self.font_family.as_str()),
        )
    }

    fn draw_staff_lines(
        &self,
        mut doc: Document,
        y_offset: f32,
        width: f32,
        lines: i32,
        margin_left: f32,
    ) -> Document {
        let start_i = (5 - lines) / 2;
        for i in 0..lines {
            let y = y_offset + ((start_i + i) as f32 * self.staff_line_distance);
            let data = Data::new()
                .move_to((margin_left, y))
                .line_to((width - self.margin_right, y));
            doc = doc.add(
                Path::new()
                    .set("fill", "none")
                    .set("stroke", "black")
                    .set("stroke-width", 1)
                    .set("d", data),
            );
        }
        doc
    }

    fn process_barline(
        &self,
        mut doc: Document,
        barline: &Barline,
        part_start_y: f32,
        x: f32,
        num_staves: i32,
        group_ranges: &[(PartGroup, f32, f32)],
        staff_dist: f32,
        staff_lines: &HashMap<i32, i32>,
        _lyric_baseline_y: f32,
    ) -> Document {
        let mut y_bottom_total = part_start_y;
        for s in 0..num_staves {
            let lines = *staff_lines.get(&(s + 1)).unwrap_or(&5);
            let start_i = (5 - lines) / 2;
            let staff_top = part_start_y
                + (s as f32 * staff_dist)
                + (start_i as f32 * self.staff_line_distance);
            let staff_bottom = staff_top + ((lines - 1) as f32 * self.staff_line_distance);
            y_bottom_total = y_bottom_total.max(staff_bottom);
        }

        let y_top_total = part_start_y;

        let draw_line = |doc: Document, lx: f32, ly1: f32, ly2: f32, style: &str| -> Document {
            let mut line = Line::new()
                .set("x1", lx)
                .set("y1", ly1)
                .set("x2", lx)
                .set("y2", ly2)
                .set("stroke", "black");
            match style {
                "heavy" => {
                    line = line.set("stroke-width", 3.0);
                }
                "dashed" => {
                    line = line.set("stroke-width", 1.0).set("stroke-dasharray", "4,4");
                }
                "dotted" => {
                    line = line
                        .set("stroke-width", 2.0)
                        .set("stroke-dasharray", "1,4")
                        .set("stroke-linecap", "round");
                }
                _ => {
                    line = line.set("stroke-width", 1.0);
                }
            }
            doc.add(line)
        };

        match barline.bar_style.as_deref() {
            Some("heavy") => {
                doc = draw_line(doc, x, y_top_total, y_bottom_total, "heavy");
            }
            Some("light-light") => {
                doc = draw_line(doc, x - 2.0, y_top_total, y_bottom_total, "regular");
                doc = draw_line(doc, x + 2.0, y_top_total, y_bottom_total, "regular");
            }
            Some("light-heavy") => {
                doc = draw_line(doc, x - 3.0, y_top_total, y_bottom_total, "regular");
                doc = draw_line(doc, x + 1.0, y_top_total, y_bottom_total, "heavy");
            }
            Some("heavy-light") => {
                doc = draw_line(doc, x - 1.0, y_top_total, y_bottom_total, "heavy");
                doc = draw_line(doc, x + 3.0, y_top_total, y_bottom_total, "regular");
            }
            Some("heavy-heavy") => {
                doc = draw_line(doc, x - 2.0, y_top_total, y_bottom_total, "heavy");
                doc = draw_line(doc, x + 2.0, y_top_total, y_bottom_total, "heavy");
            }
            Some("dashed") => {
                doc = draw_line(doc, x, y_top_total, y_bottom_total, "dashed");
            }
            Some("dotted") => {
                doc = draw_line(doc, x, y_top_total, y_bottom_total, "dotted");
            }
            Some("short") => {
                for s in 0..num_staves {
                    let staff_y = part_start_y + (s as f32 * staff_dist);
                    doc = draw_line(
                        doc,
                        x,
                        staff_y + self.staff_line_distance,
                        staff_y + 3.0 * self.staff_line_distance,
                        "regular",
                    );
                }
            }
            Some("tick") => {
                for s in 0..num_staves {
                    let staff_y = part_start_y + (s as f32 * staff_dist);
                    doc = draw_line(doc, x, staff_y - 5.0, staff_y + 5.0, "regular");
                }
            }
            _ => {
                doc = draw_line(doc, x, y_top_total, y_bottom_total, "regular");
            }
        }
        for s in 0..num_staves {
            let staff_y = part_start_y + (s as f32 * staff_dist);
            if let Some(repeat) = &barline.repeat {
                let dot_radius = 2.0;
                let dot_x = if repeat.direction == "forward" {
                    x + 8.0
                } else {
                    x - 8.0
                };
                let y_mid1 = staff_y + 1.5 * self.staff_line_distance;
                let y_mid2 = staff_y + 2.5 * self.staff_line_distance;
                doc = doc
                    .add(
                        svg::node::element::Circle::new()
                            .set("cx", dot_x)
                            .set("cy", y_mid1)
                            .set("r", dot_radius)
                            .set("fill", "black"),
                    )
                    .add(
                        svg::node::element::Circle::new()
                            .set("cx", dot_x)
                            .set("cy", y_mid2)
                            .set("r", dot_radius)
                            .set("fill", "black"),
                    );
            }
        }
        for (group, g_start_y, g_end_y) in group_ranges {
            if group.barline.as_deref() == Some("yes") && (part_start_y - *g_start_y).abs() < 1.0 {
                match barline.bar_style.as_deref() {
                    Some("heavy") => {
                        doc = doc.add(
                            Line::new()
                                .set("x1", x)
                                .set("y1", *g_start_y)
                                .set("x2", x)
                                .set("y2", *g_end_y)
                                .set("stroke", "black")
                                .set("stroke-width", 3.0),
                        );
                    }
                    Some("light-light") => {
                        doc = doc.add(
                            Line::new()
                                .set("x1", x - 2.0)
                                .set("y1", *g_start_y)
                                .set("x2", x - 2.0)
                                .set("y2", *g_end_y)
                                .set("stroke", "black")
                                .set("stroke-width", 1.0),
                        );
                        doc = doc.add(
                            Line::new()
                                .set("x1", x + 2.0)
                                .set("y1", *g_start_y)
                                .set("x2", x + 2.0)
                                .set("y2", *g_end_y)
                                .set("stroke", "black")
                                .set("stroke-width", 1.0),
                        );
                    }
                    Some("light-heavy") => {
                        doc = doc.add(
                            Line::new()
                                .set("x1", x - 3.0)
                                .set("y1", *g_start_y)
                                .set("x2", x - 3.0)
                                .set("y2", *g_end_y)
                                .set("stroke", "black")
                                .set("stroke-width", 1.0),
                        );
                        doc = doc.add(
                            Line::new()
                                .set("x1", x + 1.0)
                                .set("y1", *g_start_y)
                                .set("x2", x + 1.0)
                                .set("y2", *g_end_y)
                                .set("stroke", "black")
                                .set("stroke-width", 3.0),
                        );
                    }
                    Some("heavy-light") => {
                        doc = doc.add(
                            Line::new()
                                .set("x1", x - 1.0)
                                .set("y1", *g_start_y)
                                .set("x2", x - 1.0)
                                .set("y2", *g_end_y)
                                .set("stroke", "black")
                                .set("stroke-width", 3.0),
                        );
                        doc = doc.add(
                            Line::new()
                                .set("x1", x + 3.0)
                                .set("y1", *g_start_y)
                                .set("x2", x + 3.0)
                                .set("y2", *g_end_y)
                                .set("stroke", "black")
                                .set("stroke-width", 1.0),
                        );
                    }
                    Some("heavy-heavy") => {
                        doc = doc.add(
                            Line::new()
                                .set("x1", x - 2.0)
                                .set("y1", *g_start_y)
                                .set("x2", x - 2.0)
                                .set("y2", *g_end_y)
                                .set("stroke", "black")
                                .set("stroke-width", 3.0),
                        );
                        doc = doc.add(
                            Line::new()
                                .set("x1", x + 2.0)
                                .set("y1", *g_start_y)
                                .set("x2", x + 2.0)
                                .set("y2", *g_end_y)
                                .set("stroke", "black")
                                .set("stroke-width", 3.0),
                        );
                    }
                    _ => {
                        doc = doc.add(
                            Line::new()
                                .set("x1", x)
                                .set("y1", *g_start_y)
                                .set("x2", x)
                                .set("y2", *g_end_y)
                                .set("stroke", "black")
                                .set("stroke-width", 1.0),
                        );
                    }
                }
            }
        }

        if let Some(f_type) = &barline.fermata {
            let is_inverted = f_type == "inverted";
            let symbol = if is_inverted { "\u{E4C1}" } else { "\u{E4C0}" };
            let f_y = if is_inverted {
                y_bottom_total + 10.0
            } else {
                y_top_total - 10.0
            };
            doc = doc.add(
                Text::new(symbol)
                    .set("x", x)
                    .set("y", f_y)
                    .set("font-size", self.staff_line_distance * 4.0)
                    .set("text-anchor", "middle")
                    .set("dominant-baseline", "central")
                    .set("font-family", self.font_family.as_str()),
            );
        }

        doc
    }

    fn draw_attributes(
        &self,
        mut doc: Document,
        attr: &Attributes,
        part_start_y: f32,
        x: f32,
        current_clefs: &HashMap<i32, Clef>,
        num_staves: i32,
        staff_dist: f32,
        staff_lines: &HashMap<i32, i32>,
        has_left_forward_repeat: bool,
    ) -> (Document, f32) {
        let mut current_x = x + 8.0; // Start with 8px offset from the barline
        if has_left_forward_repeat {
            current_x += 12.0; // Add extra padding for forward repeat dots
        }
        let gap = 10.0;

        // 1. Clefs
        if !attr.clefs.is_empty() {
            let mut has_drawn = false;
            for clef in &attr.clefs {
                let staff_idx = (clef.number - 1).max(0);
                if staff_idx >= num_staves {
                    continue;
                }
                let line_count = *staff_lines.get(&clef.number).unwrap_or(&5);
                doc = self.draw_clef(
                    doc,
                    clef,
                    part_start_y + (staff_idx as f32 * staff_dist),
                    current_x + 15.0,
                    line_count,
                );
                has_drawn = true;
            }
            if has_drawn {
                current_x += 30.0 + gap;
            }
        }

        // 2. Key Signature. Skipped visually on TAB staves — a key signature
        // is meaningless for tablature and conventionally omitted there —
        // but the width is still reserved so note content stays horizontally
        // aligned with any other (non-TAB) staff/part in the same measure.
        if let Some(key) = &attr.key {
            let key_width = Self::key_signature_width(key);
            if key_width > 0.0 {
                let mut key_max_width: f32 = 0.0;
                for s in 0..num_staves {
                    let default_clef = Clef {
                        number: s + 1,
                        sign: "G".to_string(),
                        line: Some(2),
                        ..Default::default()
                    };
                    let clef = current_clefs.get(&(s + 1)).unwrap_or(&default_clef);
                    key_max_width = key_max_width.max(key_width);
                    if clef.sign == "TAB" {
                        continue;
                    }
                    doc = self.draw_key_signature(
                        doc,
                        key,
                        clef,
                        part_start_y + (s as f32 * staff_dist),
                        current_x,
                    );
                }
                current_x += key_max_width + gap;
            }
        }

        // 3. Time Signature — same TAB treatment as the key signature above.
        if let Some(time) = &attr.time {
            let time_width = (time.beats.len() as f32 * 10.0).max(22.0);
            for s in 0..num_staves {
                let default_clef = Clef {
                    number: s + 1,
                    sign: "G".to_string(),
                    line: Some(2),
                    ..Default::default()
                };
                let clef = current_clefs.get(&(s + 1)).unwrap_or(&default_clef);
                if clef.sign == "TAB" {
                    continue;
                }
                let line_count = *staff_lines.get(&(s + 1)).unwrap_or(&5);
                doc = self.draw_time_signature(
                    doc,
                    time,
                    part_start_y + (s as f32 * staff_dist),
                    current_x + time_width * 0.5,
                    line_count,
                );
            }
            current_x += time_width + gap;
        }

        (doc, current_x - x)
    }
    fn draw_clef(
        &self,
        mut doc: Document,
        clef: &Clef,
        y_offset: f32,
        x: f32,
        line_count: i32,
    ) -> Document {
        let line = clef.line.unwrap_or(match clef.sign.as_str() {
            "G" => 2,
            "F" => 4,
            "C" => 3,
            "percussion" => 3,
            _ => 3,
        });
        let mut line_y = y_offset + ((5 - line) as f32 * self.staff_line_distance);

        // Lower Bass clef by 0.1 spaces
        if clef.sign == "F" {
            line_y += 0.1 * self.staff_line_distance;
        }

        let symbol = match clef.sign.as_str() {
            "G" => "\u{E050}",
            "F" => "\u{E062}",
            "C" => "\u{E05C}",
            "percussion" => "\u{E069}",
            "TAB" => "\u{E06D}",
            _ => {
                let low = clef.sign.to_lowercase();
                if low == "percussion" {
                    "\u{E069}"
                } else if low == "tab" {
                    "\u{E06D}"
                } else {
                    "?"
                }
            }
        };

        let is_tab_or_perc = symbol == "\u{E069}" || symbol == "\u{E06D}";
        if is_tab_or_perc {
            line_y = y_offset + 2.0 * self.staff_line_distance; // Center of standard 5 lines
            if line_count == 6 {
                line_y += 0.5 * self.staff_line_distance; // Shift down 0.5 for 6-line TAB
            }
        }
        let font_size = if is_tab_or_perc {
            if line_count == 6 {
                self.staff_line_distance * 3.2
            }
            // Slightly smaller for 6-line
            else {
                self.staff_line_distance * 3.5
            }
        } else {
            self.staff_line_distance * 4.0
        };

        doc = doc.add(
            Text::new(symbol)
                .set("x", x)
                .set("y", line_y)
                .set("font-size", font_size)
                .set("text-anchor", "middle")
                .set("dominant-baseline", "central")
                .set("font-family", self.font_family.as_str()),
        );

        // Draw octave shift number '8' manually
        if let Some(shift) = clef.clef_octave_change {
            if shift != 0 {
                let oct_sym = "\u{E088}"; // timeSig8 or similar small 8
                let oct_x = if clef.sign == "F" { x + 8.0 } else { x };
                let oct_y = if shift > 0 {
                    line_y - 2.5 * self.staff_line_distance // Above
                } else {
                    line_y + 3.0 * self.staff_line_distance // Below
                };

                doc = doc.add(
                    Text::new(oct_sym)
                        .set("x", oct_x)
                        .set("y", oct_y)
                        .set("font-size", self.staff_line_distance * 1.5)
                        .set("text-anchor", "middle")
                        .set("dominant-baseline", "central")
                        .set("font-family", self.font_family.as_str()),
                );
            }
        }
        doc
    }

    /// Copy of `attr` whose key carries `cancel` = the previous key's fifths
    /// when the file didn't specify `<cancel>` and the key actually changes,
    /// so the old key's accidentals get cancelled with naturals.
    pub(crate) fn with_key_cancel(
        attr: &crate::models::Attributes,
        prev: Option<&Key>,
    ) -> crate::models::Attributes {
        let mut attr = attr.clone();
        if let (Some(key), Some(prev)) = (attr.key.as_mut(), prev) {
            if key.cancel.is_none()
                && key.key_accidentals.is_empty()
                && prev.key_accidentals.is_empty()
                && prev.fifths != key.fifths
            {
                key.cancel = Some(prev.fifths);
            }
        }
        attr
    }

    /// Indices (into the previous key's accidental order) that need a natural
    /// when changing from `key.cancel` to `key.fifths`, and whether they were
    /// sharps. Switching to C major or between sharps and flats cancels every
    /// old accidental; staying on the same side cancels only the dropped ones.
    fn key_cancel_naturals(key: &Key) -> (bool, std::ops::Range<usize>) {
        let prev = key.cancel.unwrap_or(0);
        if prev == 0 || !key.key_accidentals.is_empty() {
            return (false, 0..0);
        }
        let prev_n = prev.unsigned_abs().min(7) as usize;
        let same_side = key.fifths.signum() == prev.signum();
        let start = if same_side {
            (key.fifths.unsigned_abs() as usize).min(prev_n)
        } else {
            0
        };
        (prev > 0, start..prev_n)
    }

    /// Width a key signature occupies, including any cancelling naturals.
    fn key_signature_width(key: &Key) -> f32 {
        let accidentals = if key.key_accidentals.is_empty() {
            key.fifths.unsigned_abs() as usize
        } else {
            key.key_accidentals.len()
        };
        let naturals = Self::key_cancel_naturals(key).1.len();
        let gap = if naturals > 0 && accidentals > 0 { 4.0 } else { 0.0 };
        (naturals + accidentals) as f32 * 10.0 + gap
    }

    fn draw_key_signature(
        &self,
        mut doc: Document,
        key: &Key,
        clef: &Clef,
        y_offset: f32,
        x: f32,
    ) -> Document {
        if !key.key_accidentals.is_empty() {
            let mut current_x = x;
            for acc in &key.key_accidentals {
                let symbol = if acc.alter > 0.0 {
                    "\u{E262}"
                } else if acc.alter < 0.0 {
                    "\u{E260}"
                } else {
                    "\u{E261}"
                };

                // If octaves are specified, draw at those octaves
                if !acc.octaves.is_empty() {
                    for oct in &acc.octaves {
                        let temp_pitch = crate::models::Pitch {
                            step: acc.step.clone(),
                            octave: oct.value,
                            alter: None,
                        };
                        let acc_y = self.pitch_to_y(&temp_pitch, clef, y_offset);
                        doc = doc.add(
                            Text::new(symbol)
                                .set("x", current_x)
                                .set("y", acc_y)
                                .set("font-size", self.staff_line_distance * 4.0)
                                .set("dominant-baseline", "central")
                                .set("font-family", self.font_family.as_str()),
                        );
                    }
                } else {
                    // Fallback if no octaves: use default treble/bass positions
                    // (Simplified: just draw one for now at a reasonable octave)
                    let default_octave = match clef.sign.as_str() {
                        "F" => 3,
                        _ => 4,
                    };
                    let temp_pitch = crate::models::Pitch {
                        step: acc.step.clone(),
                        octave: default_octave,
                        alter: None,
                    };
                    let acc_y = self.pitch_to_y(&temp_pitch, clef, y_offset);
                    doc = doc.add(
                        Text::new(symbol)
                            .set("x", current_x)
                            .set("y", acc_y)
                            .set("font-size", self.staff_line_distance * 4.0)
                            .set("dominant-baseline", "central")
                            .set("font-family", self.font_family.as_str()),
                    );
                }
                current_x += 10.0;
            }
            return doc;
        }

        let treble_sharps = [0.0, 1.5, -0.5, 1.0, 2.5, 0.5, 2.0];
        let treble_flats = [2.0, 0.5, 2.5, 1.0, 3.0, 1.5, 3.5];
        let bass_sharps = [1.0, 2.5, 0.5, 2.0, 3.5, 1.5, 3.0];
        let bass_flats = [3.0, 1.5, 3.5, 2.0, 4.0, 2.5, 4.5];
        let offsets_for = |is_sharp: bool| match (clef.sign.as_str(), is_sharp) {
            ("F", true) => &bass_sharps,
            ("F", false) => &bass_flats,
            (_, true) => &treble_sharps,
            (_, false) => &treble_flats,
        };

        // Cancelling naturals for the previous key come first, on the lines
        // and spaces where its accidentals stood.
        let mut x = x;
        let (cancel_sharps, cancel_range) = Self::key_cancel_naturals(key);
        if !cancel_range.is_empty() {
            let prev_offsets = offsets_for(cancel_sharps);
            for (col, i) in cancel_range.enumerate() {
                doc = doc.add(
                    Text::new("\u{E261}")
                        .set("x", x + col as f32 * 10.0)
                        .set("y", y_offset + prev_offsets[i] * self.staff_line_distance)
                        .set("font-size", self.staff_line_distance * 4.0)
                        .set("dominant-baseline", "central")
                        .set("font-family", self.font_family.as_str()),
                );
            }
            x += Self::key_cancel_naturals(key).1.len() as f32 * 10.0 + 4.0;
        }

        let fifths = key.fifths;
        if fifths == 0 {
            return doc;
        }
        let is_sharp = fifths > 0;
        let symbol = if is_sharp { "\u{E262}" } else { "\u{E260}" };
        let offsets = offsets_for(is_sharp);
        for i in 0..(fifths.abs() as usize) {
            if i < offsets.len() {
                doc = doc.add(
                    Text::new(symbol)
                        .set("x", x + (i as f32 * 10.0))
                        .set("y", y_offset + offsets[i] * self.staff_line_distance)
                        .set("font-size", self.staff_line_distance * 4.0)
                        .set("dominant-baseline", "central")
                        .set("font-family", self.font_family.as_str()),
                );
            }
        }
        doc
    }

    fn draw_time_signature(
        &self,
        doc: Document,
        time: &Time,
        y_offset: f32,
        x: f32,
        line_count: i32,
    ) -> Document {
        let mut center_y = y_offset + (2.0 * self.staff_line_distance);
        if line_count == 6 {
            center_y += 0.5 * self.staff_line_distance;
        }
        let font_size = if line_count == 6 {
            self.staff_line_distance * 2.8
        } else {
            self.staff_line_distance * 2.5
        };
        doc.add(
            Text::new(time.beats.as_str())
                .set("x", x)
                .set("y", center_y - 0.1 * self.staff_line_distance)
                .set("font-size", font_size)
                .set("text-anchor", "middle")
                .set("dominant-baseline", "central")
                .set("font-family", self.font_family.as_str())
                .set("font-weight", "bold"),
        )
        .add(
            Text::new(time.beat_type.to_string())
                .set("x", x)
                .set("y", center_y + 1.9 * self.staff_line_distance)
                .set("font-size", font_size)
                .set("text-anchor", "middle")
                .set("dominant-baseline", "central")
                .set("font-family", self.font_family.as_str())
                .set("font-weight", "bold"),
        )
    }

    /// Collects which voices appear on each staff of a measure and which
    /// chord groups must be nudged right because their noteheads would overlap
    /// another voice's (unison or a second apart) at the same onset. Time
    /// positions use the same 10080-per-quarter units as the renderer.
    pub(crate) fn analyze_measure_voices(
        measure: &crate::models::Measure,
        mut divisions: i32,
    ) -> MeasureVoiceInfo {
        let mut info = MeasureVoiceInfo::default();
        // (staff, time) -> voice -> diatonic steps of its noteheads
        let mut heads: HashMap<(i32, i32), HashMap<i32, Vec<i32>>> = HashMap::new();
        // (staff, time, voice) -> duration of that voice's chord group
        let mut durations: HashMap<(i32, i32, i32), i32> = HashMap::new();
        // (staff, time, voice) -> explicit stem direction (down?) of that chord group
        let mut stem_down: HashMap<(i32, i32, i32), Option<bool>> = HashMap::new();
        let mut time = 0;
        let mut chord_time = 0;
        let to_units = |d: i32, div: i32| (d as f64 * 10080.0 / div.max(1) as f64).round() as i32;
        for el in &measure.elements {
            match el {
                MeasureElement::Note(n) => {
                    let staff = n.staff.unwrap_or(1);
                    let voice = n.voice.unwrap_or(1);
                    let t = if n.is_chord { chord_time } else { time };
                    if !n.is_chord && n.grace.is_none() && n.print_object != Some(false) {
                        info.voice_spans.entry(staff).or_default().push((
                            voice,
                            t,
                            t + to_units(n.duration, divisions),
                        ));
                        if n.rest {
                            let step = n.unpitched.as_ref().and_then(|u| {
                                "CDEFGAB"
                                    .find(u.display_step.as_str())
                                    .map(|i| u.display_octave * 7 + i as i32)
                            });
                            info.rest_spans.entry(staff).or_default().push((
                                voice,
                                t,
                                t + to_units(n.duration, divisions),
                                step,
                            ));
                        }
                    }
                    // A hidden note draws no notehead, so it can't collide.
                    if !n.rest && n.grace.is_none() && n.print_object != Some(false) {
                        let step = n
                            .pitch
                            .as_ref()
                            .map(|p| (p.step.as_str(), p.octave))
                            .or_else(|| {
                                n.unpitched
                                    .as_ref()
                                    .map(|u| (u.display_step.as_str(), u.display_octave))
                            })
                            .and_then(|(st, oct)| {
                                "CDEFGAB".find(st).map(|i| oct * 7 + i as i32)
                            });
                        if !n.is_chord {
                            durations.insert((staff, t, voice), n.duration);
                            let down = match n.stem.as_deref() {
                                Some("down") => Some(true),
                                Some("up") => Some(false),
                                _ => None,
                            };
                            stem_down.insert((staff, t, voice), down);
                        }
                        if let Some(step) = step {
                            heads
                                .entry((staff, t))
                                .or_default()
                                .entry(voice)
                                .or_default()
                                .push(step);
                            info.head_spans.entry(staff).or_default().push((
                                voice,
                                t,
                                t + to_units(n.duration, divisions),
                                step,
                            ));
                        }
                    }
                    if !n.is_chord && n.grace.is_none() {
                        chord_time = time;
                        time += to_units(n.duration, divisions);
                    }
                }
                MeasureElement::Backup(d) => {
                    time = (time - to_units(*d, divisions)).max(0);
                    chord_time = time;
                }
                MeasureElement::Forward(d) => {
                    time += to_units(*d, divisions);
                    chord_time = time;
                }
                MeasureElement::Attributes(attr) => {
                    if let Some(d) = attr.divisions {
                        divisions = d;
                    }
                }
                _ => {}
            }
        }
        for (&(staff, t), by_voice) in &heads {
            let mut voices: Vec<i32> = by_voice.keys().copied().collect();
            voices.sort_unstable();
            for (i, &voice) in voices.iter().enumerate() {
                let steps = &by_voice[&voice];
                let mut sorted = steps.clone();
                sorted.sort_unstable();
                if sorted.windows(2).any(|w| w[1] - w[0] <= 1) {
                    let down = stem_down.get(&(staff, t, voice)).copied().flatten();
                    info.chord_seconds.push((t, down));
                }
                let dur = durations.get(&(staff, t, voice));
                for &other in &voices[..i] {
                    let overlaps = steps
                        .iter()
                        .any(|a| by_voice[&other].iter().any(|b| (a - b).abs() <= 1));
                    if !overlaps {
                        continue;
                    }
                    if dur.is_some() && dur == durations.get(&(staff, t, other)) {
                        info.offsets.entry((staff, t, other)).or_insert(-0.5);
                        info.offsets.insert((staff, t, voice), 0.5);
                    } else {
                        info.offsets.insert((staff, t, voice), 1.0);
                    }
                    break;
                }
            }
        }
        info
    }

    /// Vertical rest offset in staff spaces. A rest stays centred unless
    /// another voice on the same staff has a note or rest sounding during the
    /// rest's own time span — voice numbers alone don't matter (exporters such
    /// as MuseScore number the second staff's voices 5-8, and a voice may only
    /// cover part of the measure). When voices do overlap, the lowest-numbered
    /// (upper) voice's rest moves up and the others' move down.
    /// How this note's voice shares its staff with other voices, for placing
    /// a slur that starts on it.
    fn voice_sharing(
        note: &Note,
        time_pos: i32,
        divisions: i32,
        voice_info: &MeasureVoiceInfo,
    ) -> VoiceSharing {
        if Self::rest_voice_shift(note, time_pos, divisions, voice_info) != 0.0 {
            return VoiceSharing::Now;
        }
        let voice = note.voice.unwrap_or(1);
        let lowest_later = voice_info.voice_spans.get(&note.staff.unwrap_or(1)).and_then(|spans| {
            spans
                .iter()
                .filter(|&&(v, _, e)| v != voice && e > time_pos)
                .map(|&(v, _, _)| v)
                .min()
        });
        match lowest_later {
            None => VoiceSharing::Alone,
            Some(lowest_other) => VoiceSharing::Later { upper: voice < lowest_other },
        }
    }

    /// Columns needed for accidentals on these diatonic steps: each goes in
    /// the first column whose accidentals are all a sixth or more away.
    pub(crate) fn accidental_column_count(steps: &[i32]) -> usize {
        let mut sorted = steps.to_vec();
        sorted.sort_unstable_by(|a, b| b.cmp(a));
        let mut columns: Vec<Vec<i32>> = Vec::new();
        for s in sorted {
            match columns
                .iter_mut()
                .find(|c| c.iter().all(|o| (o - s).abs() >= ACCIDENTAL_COLUMN_STEPS))
            {
                Some(c) => c.push(s),
                None => columns.push(vec![s]),
            }
        }
        columns.len().max(1)
    }

    /// Moves a rest that shares its time with other voices (`y` already
    /// nudged toward its side by `rest_voice_shift`) further out, by whole
    /// staff spaces, until it clears their noteheads: above them for the upper
    /// voice, below for the others.
    fn rest_y_clear_of_voices(
        &self,
        y: f32,
        note: &Note,
        time_pos: i32,
        divisions: i32,
        voice_info: &MeasureVoiceInfo,
        clef: &Clef,
        y_offset: f32,
    ) -> f32 {
        // Rest glyph half-height plus a notehead's half-height and a gap,
        // in staff spaces.
        const CLEARANCE: f32 = 2.5;
        // Two rest glyphs' half-heights and a gap.
        const REST_CLEARANCE: f32 = 3.0;
        let shift = Self::rest_voice_shift(note, time_pos, divisions, voice_info);
        // On TAB, pitches aren't where the fret numbers sit.
        if shift == 0.0 || clef.sign == "TAB" {
            return y;
        }
        let voice = note.voice.unwrap_or(1);
        let end = time_pos
            + (note.duration as f64 * 10080.0 / divisions.max(1) as f64).round() as i32;
        let Some(heads) = voice_info.head_spans.get(&note.staff.unwrap_or(1)) else {
            return y;
        };
        let step_y = |step: i32| {
            let i = step.rem_euclid(7) as usize;
            let pitch = crate::models::Pitch {
                step: "CDEFGAB"[i..i + 1].to_string(),
                octave: step.div_euclid(7),
                alter: None,
            };
            self.pitch_to_y(&pitch, clef, y_offset)
        };
        let space = self.staff_line_distance;
        // (y, clearance) of what this rest must stay clear of.
        let mut obstacles: Vec<(f32, f32)> = heads
            .iter()
            .filter(|&&(v, s, e, _)| v != voice && s < end && time_pos < e)
            .map(|&(_, _, _, step)| (step_y(step), CLEARANCE))
            .collect();
        // A lower voice's rest also steps under an upper voice's rest at the
        // same time (only one of the two moves, so they don't fly apart).
        if shift > 0.0 {
            if let Some(rests) = voice_info.rest_spans.get(&note.staff.unwrap_or(1)) {
                let middle = y_offset + 2.0 * space;
                obstacles.extend(
                    rests
                        .iter()
                        .filter(|&&(v, s, e, _)| v < voice && s < end && time_pos < e)
                        .map(|&(_, _, _, step)| {
                            (step.map_or(middle - space, step_y), REST_CLEARANCE)
                        }),
                );
            }
        }
        if obstacles.is_empty() {
            return y;
        }
        let mut y = y;
        if shift < 0.0 {
            let limit = obstacles
                .iter()
                .map(|&(oy, c)| oy - c * space)
                .fold(f32::MAX, f32::min);
            while y > limit + 0.01 {
                y -= space;
            }
        } else {
            let limit = obstacles
                .iter()
                .map(|&(oy, c)| oy + c * space)
                .fold(f32::MIN, f32::max);
            while y < limit - 0.01 {
                y += space;
            }
        }
        y
    }

    fn rest_voice_shift(
        note: &Note,
        time_pos: i32,
        divisions: i32,
        voice_info: &MeasureVoiceInfo,
    ) -> f32 {
        let voice = note.voice.unwrap_or(1);
        let end = time_pos
            + (note.duration as f64 * 10080.0 / divisions.max(1) as f64).round() as i32;
        let Some(spans) = voice_info.voice_spans.get(&note.staff.unwrap_or(1)) else {
            return 0.0;
        };
        let overlapping: Vec<i32> = spans
            .iter()
            .filter(|&&(v, s, e)| v != voice && s < end && time_pos < e)
            .map(|&(v, _, _)| v)
            .collect();
        match overlapping.iter().min() {
            None => 0.0,
            Some(&lowest_other) if voice < lowest_other => -1.0,
            Some(_) => 1.0,
        }
    }

    fn draw_rest(
        &self,
        mut doc: Document,
        note: &Note,
        y_offset: f32,
        x: f32,
        dot_count: i32,
        clef: &Clef,
        line_count: i32,
        _measure_total_dur: i32,
        measure_theoretical_dur: i32,
        divisions: i32,
        time_pos: i32,
        voice_info: &MeasureVoiceInfo,
    ) -> Document {
        if note.print_object == Some(false) {
            return doc;
        }
        let duration_units = if divisions > 0 {
            note.duration as f32 * 10080.0 / divisions as f32
        } else {
            note.duration as f32
        };
        let full_measure_units = measure_theoretical_dur.max(0) as f32;
        let is_full_measure_rest = note.rest_measure
            && note.note_type.is_none()
            && full_measure_units > 0.0
            && duration_units >= full_measure_units - 0.5;
        let note_type = if note.rest_measure && note.note_type.is_none() {
            if is_full_measure_rest {
                "whole"
            } else {
                self.infer_note_type_from_duration(note.duration, divisions)
            }
        } else {
            note.note_type.as_deref().unwrap_or("quarter")
        };
        let symbol = match note_type {
            "whole" => "\u{E4E3}",
            "half" => "\u{E4E4}",
            "quarter" => "\u{E4E5}",
            "eighth" => "\u{E4E6}",
            "16th" => "\u{E4E7}",
            "32nd" => "\u{E4E8}",
            "64th" => "\u{E4E9}",
            "128th" => "\u{E4EA}",
            "256th" => "\u{E4EB}",
            "512th" => "\u{E4EC}",
            "1024th" => "\u{E4ED}",
            _ => "\u{E4E5}",
        };

        let y = if is_full_measure_rest {
            if line_count == 1 {
                // For 1-line staff, hang from the only line (index 2)
                y_offset + 2.0 * self.staff_line_distance
            } else {
                // Measure rests hang from the 4th line (second from top)
                y_offset + 1.0 * self.staff_line_distance
            }
        } else if let Some(unpitched) = &note.unpitched {
            let temp_pitch = crate::models::Pitch {
                step: unpitched.display_step.clone(),
                octave: unpitched.display_octave,
                alter: None,
            };
            // The exporter's position may still sit on another voice's note.
            let y = self.pitch_to_y(&temp_pitch, clef, y_offset);
            self.rest_y_clear_of_voices(y, note, time_pos, divisions, voice_info, clef, y_offset)
        } else {
            let base_shift = match note_type {
                "whole" => 1.0, // hangs from line 4
                "half" => 2.0,  // sits on line 3
                _ => 2.0,       // centered on line 3
            };
            let voice_shift = Self::rest_voice_shift(note, time_pos, divisions, voice_info);
            let y = y_offset + (base_shift + voice_shift) * self.staff_line_distance;
            self.rest_y_clear_of_voices(y, note, time_pos, divisions, voice_info, clef, y_offset)
        };

        doc = doc.add(
            Text::new(symbol)
                .set("x", x)
                .set("y", y)
                .set("font-size", self.staff_line_distance * 4.0)
                .set("text-anchor", "middle")
                .set("dominant-baseline", "central")
                .set("font-family", self.font_family.as_str()),
        );
        for i in 0..dot_count {
            doc = doc.add(
                svg::node::element::Circle::new()
                    .set("cx", x + 12.0 + (i as f32 * 6.0))
                    .set("cy", y - 0.5 * self.staff_line_distance)
                    .set("r", 1.5)
                    .set("fill", "black"),
            );
        }
        doc
    }

    fn draw_ledger_lines(
        &self,
        mut doc: Document,
        y: f32,
        y_offset: f32,
        x: f32,
        line_count: i32,
    ) -> Document {
        if line_count == 1 {
            return doc;
        }
        let top_y = y_offset;
        let bottom_y = y_offset + 4.0 * self.staff_line_distance;
        let line_half_width = 10.0;
        if y < top_y - 0.1 {
            let mut cur_y = top_y - self.staff_line_distance;
            while cur_y >= y - 0.1 {
                doc = doc.add(
                    Line::new()
                        .set("x1", x - line_half_width)
                        .set("y1", cur_y)
                        .set("x2", x + line_half_width)
                        .set("y2", cur_y)
                        .set("stroke", "black")
                        .set("stroke-width", 1),
                );
                cur_y -= self.staff_line_distance;
            }
        } else if y > bottom_y + 0.1 {
            let mut cur_y = bottom_y + self.staff_line_distance;
            while cur_y <= y + 0.1 {
                doc = doc.add(
                    Line::new()
                        .set("x1", x - line_half_width)
                        .set("y1", cur_y)
                        .set("x2", x + line_half_width)
                        .set("y2", cur_y)
                        .set("stroke", "black")
                        .set("stroke-width", 1),
                );
                cur_y += self.staff_line_distance;
            }
        }
        doc
    }
}
