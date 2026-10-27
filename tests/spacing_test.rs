/// Horizontal spacing modes
///
/// - Compact: notes are spaced the same whatever their duration (as tight
///   as collisions allow).
/// - Elastic: onset-to-onset distance follows the duration (duration^0.6),
///   except where flags, accidentals or lyrics need more room.
///
/// Both must keep the spacing of every measure in a system alike after the
/// system is justified (the clef, key and time of a system's first measure
/// don't stretch).
use staveloom_core::parser::parse_in_memory;
use staveloom_core::renderer::{Renderer, SpacingStrategy};

fn note(step: &str, duration: u32, kind: &str, lyric: Option<&str>) -> String {
    let lyric = lyric.map_or(String::new(), |l| {
        format!("<lyric><text>{l}</text></lyric>")
    });
    format!(
        "<note><pitch><step>{step}</step><octave>4</octave></pitch>\
         <duration>{duration}</duration><type>{kind}</type>{lyric}</note>"
    )
}

/// One system of five 4/4 measures (divisions = 4):
/// 1. half, quarter, eighth, eighth
/// 2. four 16ths, quarter, half
/// 3. whole
/// 4. four quarters
/// 5. eighth, eighth (with long lyrics), dotted half
fn test_score() -> String {
    let measures = [
        vec![
            note("C", 8, "half", None),
            note("D", 4, "quarter", None),
            note("E", 2, "eighth", None),
            note("F", 2, "eighth", None),
        ],
        vec![
            note("C", 1, "16th", None),
            note("D", 1, "16th", None),
            note("E", 1, "16th", None),
            note("F", 1, "16th", None),
            note("G", 4, "quarter", None),
            note("A", 8, "half", None),
        ],
        vec![note("C", 16, "whole", None)],
        vec![
            note("C", 4, "quarter", None),
            note("D", 4, "quarter", None),
            note("E", 4, "quarter", None),
            note("F", 4, "quarter", None),
        ],
        vec![
            note("C", 2, "eighth", Some("Wonderfully")),
            note("D", 2, "eighth", Some("long")),
            note("E", 12, "half", None),
        ],
    ];
    let mut body = String::new();
    for (i, notes) in measures.iter().enumerate() {
        let attributes = if i == 0 {
            "<attributes><divisions>4</divisions><key><fifths>0</fifths></key>\
             <time><beats>4</beats><beat-type>4</beat-type></time>\
             <clef><sign>G</sign><line>2</line></clef></attributes>"
        } else {
            ""
        };
        body += &format!(
            "<measure number=\"{}\">{attributes}{}</measure>",
            i + 1,
            notes.concat()
        );
    }
    format!(
        "<?xml version=\"1.0\"?><score-partwise version=\"3.1\"><part-list>\
         <score-part id=\"P1\"><part-name>T</part-name></score-part></part-list>\
         <part id=\"P1\">{body}</part></score-partwise>"
    )
}

/// Notehead x positions in drawing order, per measure.
fn noteheads(strategy: SpacingStrategy) -> Vec<Vec<f32>> {
    let score = parse_in_memory(test_score().as_bytes()).expect("test score parses");
    let mut renderer = Renderer::default();
    renderer.spacing_strategy = strategy;
    let svg = renderer.render(&score);
    // Noteheads are SMuFL glyphs (whole, half, black) in <text x="..">.
    let mut xs = Vec::new();
    for (pos, ch) in svg.char_indices() {
        if !matches!(ch, '\u{E0A2}' | '\u{E0A3}' | '\u{E0A4}') {
            continue;
        }
        let tag_start = svg[..pos].rfind("<text").expect("glyph inside <text>");
        let tag = &svg[tag_start..pos];
        let x = tag.split(" x=\"").nth(1).and_then(|r| r.split('"').next());
        xs.push(
            x.and_then(|v| v.parse::<f32>().ok())
                .expect("notehead has x"),
        );
    }
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let counts = [4, 6, 1, 4, 3];
    assert_eq!(
        xs.len(),
        counts.iter().sum::<usize>(),
        "one notehead per note"
    );
    let mut measures = Vec::new();
    let mut rest = xs.as_slice();
    for n in counts {
        let (m, r) = rest.split_at(n);
        measures.push(m.to_vec());
        rest = r;
    }
    measures
}

fn gaps(xs: &[f32]) -> Vec<f32> {
    xs.windows(2).map(|w| w[1] - w[0]).collect()
}

#[test]
fn compact_spaces_notes_alike_whatever_their_duration() {
    let m = noteheads(SpacingStrategy::Compact);
    let quarter = gaps(&m[3])[0];
    for g in gaps(&m[3]) {
        assert!((g - quarter).abs() < 1.0, "even quarters: {g} vs {quarter}");
    }
    // Half -> quarter and quarter -> eighth (no flag before them) match the
    // quarters of another measure in the same system.
    let m1 = gaps(&m[0]);
    assert!(
        (m1[0] - quarter).abs() < 1.0,
        "half spaced like a quarter: {m1:?} vs {quarter}"
    );
    assert!(
        (m1[1] - quarter).abs() < 1.0,
        "first measure stretched alike: {m1:?} vs {quarter}"
    );
}

#[test]
fn elastic_spaces_notes_by_duration() {
    let m = noteheads(SpacingStrategy::Elastic);
    let quarter = gaps(&m[3])[0];
    for g in gaps(&m[3]) {
        assert!((g - quarter).abs() < 1.0, "even quarters: {g} vs {quarter}");
    }
    let m1 = gaps(&m[0]);
    // The system's first measure (with clef, key and time) is stretched
    // like the others.
    assert!(
        (m1[1] - quarter).abs() < 1.0,
        "quarter in measure 1: {m1:?} vs {quarter}"
    );
    let ratio = m1[0] / quarter;
    assert!(
        (ratio - 2f32.powf(0.6)).abs() < 0.08,
        "half / quarter = {ratio}"
    );
    // Short notes never get more room than a quarter, flags included.
    assert!(
        m1[2] < quarter,
        "eighth narrower than a quarter: {m1:?} vs {quarter}"
    );
    for g in &gaps(&m[1])[..3] {
        assert!(
            *g < quarter,
            "16th narrower than a quarter: {g} vs {quarter}"
        );
    }
    // Long lyrics push their notes apart.
    let m5 = gaps(&m[4]);
    assert!(
        m5[0] > quarter * 1.5,
        "lyrics make room: {m5:?} vs {quarter}"
    );
}
