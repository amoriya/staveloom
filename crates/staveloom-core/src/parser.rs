use crate::models::*;
use roxmltree::Node;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use thiserror::Error;
use zip::ZipArchive;

#[derive(Error, Debug)]
pub enum ParserError {
    #[error("XML parsing error: {0}")]
    XmlError(#[from] roxmltree::Error),
    #[error("Invalid MusicXML structure: {0}")]
    InvalidStructure(String),
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Zip error: {0}")]
    ZipError(#[from] zip::result::ZipError),
}

/// Decode a byte buffer that may be UTF-8, UTF-16LE, or UTF-16BE into a
/// `String`, sniffing the encoding from a leading byte-order mark the same
/// way conforming XML processors do (a `<?xml ... encoding="UTF-16"?>`
/// declaration is required by the XML spec to be paired with a BOM, since
/// otherwise the endianness would be ambiguous). Falls back to UTF-8 when no
/// BOM is present, which covers the overwhelming majority of MusicXML files.
fn decode_xml_bytes(bytes: &[u8]) -> Result<String, ParserError> {
    let invalid_data = |e: std::string::FromUtf16Error| {
        ParserError::IoError(std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    };
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        // UTF-16, little-endian
        let units: Vec<u16> = rest
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        return String::from_utf16(&units).map_err(invalid_data);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        // UTF-16, big-endian
        let units: Vec<u16> = rest
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        return String::from_utf16(&units).map_err(invalid_data);
    }
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    std::str::from_utf8(bytes)
        .map(|s| s.to_string())
        .map_err(|e| ParserError::IoError(std::io::Error::new(std::io::ErrorKind::InvalidData, e)))
}

/// Read a zip entry's raw bytes and decode it as XML text (see `decode_xml_bytes`).
fn read_zip_entry_as_string(file: &mut zip::read::ZipFile) -> Result<String, ParserError> {
    let mut buf = Vec::new();
    file.read_to_end(&mut buf)?;
    decode_xml_bytes(&buf)
}

pub fn parse_in_memory(bytes: &[u8]) -> Result<Score, ParserError> {
    if bytes.starts_with(b"PK\x03\x04") {
        let cursor = std::io::Cursor::new(bytes);
        let mut archive = ZipArchive::new(cursor)?;
        let mut xml_data = String::new();
        let mut found = false;

        if archive.by_name("META-INF/container.xml").is_ok() {
            let mut container_file = archive.by_name("META-INF/container.xml")?;
            let container_xml = read_zip_entry_as_string(&mut container_file)?;

            let doc = roxmltree::Document::parse(&container_xml)?;
            if let Some(root_files) = doc
                .root_element()
                .children()
                .find(|n| n.has_tag_name("rootfiles"))
            {
                if let Some(root_file) = root_files.children().find(|n| n.has_tag_name("rootfile"))
                {
                    if let Some(path) = root_file.attribute("full-path") {
                        let path = path.to_string();
                        drop(container_file);
                        if let Ok(mut xml_file) = archive.by_name(&path) {
                            xml_data = read_zip_entry_as_string(&mut xml_file)?;
                            found = true;
                        }
                    }
                }
            }
        }

        if !found {
            for i in 0..archive.len() {
                let mut file = archive.by_index(i)?;
                if file.name().ends_with(".xml") || file.name().ends_with(".musicxml") {
                    xml_data = read_zip_entry_as_string(&mut file)?;
                    found = true;
                    break;
                }
            }
        }

        if !found {
            return Err(ParserError::InvalidStructure(
                "No XML file found in .mxl archive".to_string(),
            ));
        }
        parse_musicxml(&xml_data)
    } else {
        let xml_data = decode_xml_bytes(bytes)?;
        parse_musicxml(&xml_data)
    }
}

pub fn load_and_parse(path: &Path) -> Result<Score, ParserError> {
    let extension = path.extension().and_then(|s| s.to_str()).unwrap_or("");
    if extension == "mxl" {
        let file = File::open(path)?;
        let mut archive = ZipArchive::new(file)?;

        let mut xml_data = String::new();

        // Try to find the root file from container.xml
        let mut found = false;
        {
            if archive.by_name("META-INF/container.xml").is_ok() {
                let mut container_file = archive.by_name("META-INF/container.xml")?;
                let container_xml = read_zip_entry_as_string(&mut container_file)?;

                let doc = roxmltree::Document::parse(&container_xml)?;
                if let Some(root_files) = doc
                    .root_element()
                    .children()
                    .find(|n| n.has_tag_name("rootfiles"))
                {
                    if let Some(root_file) =
                        root_files.children().find(|n| n.has_tag_name("rootfile"))
                    {
                        if let Some(path) = root_file.attribute("full-path") {
                            let path = path.to_string(); // Need to clone path string because dropping container_file doesn't kill scope
                            drop(container_file);
                            if let Ok(mut xml_file) = archive.by_name(&path) {
                                xml_data = read_zip_entry_as_string(&mut xml_file)?;
                                found = true;
                            }
                        }
                    }
                }
            }
        }

        // Fallback: search for first .musicxml or .xml file
        if !found {
            for i in 0..archive.len() {
                let mut file = archive.by_index(i)?;
                if file.name().ends_with(".xml") || file.name().ends_with(".musicxml") {
                    xml_data = read_zip_entry_as_string(&mut file)?;
                    found = true;
                    break;
                }
            }
        }

        if !found {
            return Err(ParserError::InvalidStructure(
                "No XML file found in .mxl archive".to_string(),
            ));
        }
        parse_musicxml(&xml_data)
    } else {
        let mut file = File::open(path)?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)?;
        let xml_data = decode_xml_bytes(&buf)?;
        parse_musicxml(&xml_data)
    }
}

pub fn parse_musicxml(xml: &str) -> Result<Score, ParserError> {
    let clean_xml = if let Some(start) = xml.find("<!DOCTYPE") {
        if let Some(end) = xml[start..].find('>') {
            let mut s = xml.to_string();
            s.replace_range(start..start + end + 1, "");
            s
        } else {
            xml.to_string()
        }
    } else {
        xml.to_string()
    };

    let doc = roxmltree::Document::parse(&clean_xml)?;
    let root = doc.root_element();

    if root.tag_name().name() != "score-partwise" {
        return Err(ParserError::InvalidStructure(
            "Root element is not score-partwise".to_string(),
        ));
    }

    let mut score = Score::default();
    score.version = root.attribute("version").map(|s| s.to_string());

    if let Some(work) = root.children().find(|n| n.has_tag_name("work")) {
        score.title = work
            .children()
            .find(|n| n.has_tag_name("work-title"))
            .map(|n| n.text().unwrap_or_default().to_string());
    }

    if let Some(ident) = root.children().find(|n| n.has_tag_name("identification")) {
        score.creator = ident
            .children()
            .find(|n| n.has_tag_name("creator"))
            .map(|n| n.text().unwrap_or_default().to_string());
    }

    if root.children().any(|n| n.has_tag_name("concert-score")) {
        score.concert_score = true;
    }

    if let Some(part_list_node) = root.children().find(|n| n.has_tag_name("part-list")) {
        for child in part_list_node.children() {
            match child.tag_name().name() {
                "score-part" => {
                    let id = child.attribute("id").unwrap_or_default().to_string();
                    let name = child
                        .children()
                        .find(|n| n.has_tag_name("part-name"))
                        .map(|n| n.text().unwrap_or_default().to_string());
                    let abbreviation = child
                        .children()
                        .find(|n| n.has_tag_name("part-abbreviation"))
                        .map(|n| n.text().unwrap_or_default().to_string());

                    let mut part_links = Vec::new();
                    for pl_node in child.children().filter(|n| n.has_tag_name("part-link")) {
                        let mut pl = crate::models::PartLink::default();
                        pl.href = pl_node
                            .attributes()
                            .find(|a| a.name() == "href")
                            .map(|a| a.value().to_string());
                        pl.title = pl_node
                            .attributes()
                            .find(|a| a.name() == "title")
                            .map(|a| a.value().to_string());
                        for il_node in pl_node
                            .children()
                            .filter(|n| n.has_tag_name("instrument-link"))
                        {
                            if let Some(il_id) = il_node.attribute("id") {
                                pl.instrument_links.push(il_id.to_string());
                            }
                        }
                        part_links.push(pl);
                    }

                    let name_display = child
                        .children()
                        .find(|n| n.has_tag_name("part-name-display"))
                        .map(parse_name_display);
                    let abbreviation_display = child
                        .children()
                        .find(|n| n.has_tag_name("part-abbreviation-display"))
                        .map(parse_name_display);

                    let instrument_sound = child
                        .descendants()
                        .find(|n| n.has_tag_name("instrument-sound"))
                        .and_then(|n| n.text())
                        .map(|s| s.to_string());

                    let mut instrument_names = Vec::new();
                    for instr in child
                        .children()
                        .filter(|n| n.has_tag_name("score-instrument"))
                    {
                        if let Some(in_name) = instr
                            .children()
                            .find(|n| n.has_tag_name("instrument-name"))
                            .and_then(|n| n.text())
                        {
                            instrument_names.push(in_name.to_string());
                        }
                    }

                    let mut midi_instruments = Vec::new();
                    for n in child
                        .children()
                        .filter(|n| n.has_tag_name("midi-instrument"))
                    {
                        let id = n.attribute("id").unwrap_or_default().to_string();
                        let channel = n
                            .children()
                            .find(|c| c.has_tag_name("midi-channel"))
                            .and_then(|c| c.text())
                            .and_then(|t| t.parse().ok());
                        let program = n
                            .children()
                            .find(|c| c.has_tag_name("midi-program"))
                            .and_then(|c| c.text())
                            .and_then(|t| t.parse().ok());
                        let volume = n
                            .children()
                            .find(|c| c.has_tag_name("volume"))
                            .and_then(|c| c.text())
                            .and_then(|t| t.parse().ok());
                        let pan = n
                            .children()
                            .find(|c| c.has_tag_name("pan"))
                            .and_then(|c| c.text())
                            .and_then(|t| t.parse().ok());
                        let elevation = n
                            .children()
                            .find(|c| c.has_tag_name("elevation"))
                            .and_then(|c| c.text())
                            .and_then(|t| t.parse().ok());
                        let midi_unpitched = n
                            .children()
                            .find(|c| c.has_tag_name("midi-unpitched"))
                            .and_then(|c| c.text())
                            .and_then(|t| t.parse().ok());
                        midi_instruments.push(crate::models::MidiInstrument {
                            id,
                            channel,
                            program,
                            volume,
                            pan,
                            elevation,
                            midi_unpitched,
                        });
                    }

                    score.part_list.push(PartListItem::Part {
                        id,
                        name,
                        instrument_names,
                        abbreviation,
                        part_links,
                        name_display,
                        abbreviation_display,
                        instrument_sound,
                        midi_instruments,
                    });
                }
                "part-group" => {
                    let number = child
                        .attribute("number")
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(1);
                    let group_type = child.attribute("type").unwrap_or("start").to_string();
                    let mut group = PartGroup {
                        number,
                        group_type,
                        ..Default::default()
                    };

                    if let Some(n) = child.children().find(|n| n.has_tag_name("group-name")) {
                        group.name = n.text().map(|t| t.to_string());
                    }
                    if let Some(n) = child
                        .children()
                        .find(|n| n.has_tag_name("group-abbreviation"))
                    {
                        group.abbreviation = n.text().map(|t| t.to_string());
                    }
                    if let Some(sym_node) =
                        child.children().find(|n| n.has_tag_name("group-symbol"))
                    {
                        group.symbol = match sym_node.text().unwrap_or_default() {
                            "brace" => Some(GroupSymbol::Brace),
                            "bracket" => Some(GroupSymbol::Bracket),
                            "line" => Some(GroupSymbol::Line),
                            "none" => Some(GroupSymbol::None),
                            _ => None,
                        };
                    }
                    if let Some(bl_node) =
                        child.children().find(|n| n.has_tag_name("group-barline"))
                    {
                        group.barline = Some(bl_node.text().unwrap_or_default().to_string());
                    }
                    score.part_list.push(PartListItem::Group(group));
                }
                _ => {}
            }
        }
    }

    for part_node in root.children().filter(|n| n.has_tag_name("part")) {
        let mut part = Part {
            id: part_node.attribute("id").unwrap_or_default().to_string(),
            ..Default::default()
        };

        for measure_node in part_node.children().filter(|n| n.has_tag_name("measure")) {
            let mut measure = Measure {
                number: measure_node
                    .attribute("number")
                    .unwrap_or_default()
                    .to_string(),
                implicit: measure_node.attribute("implicit") == Some("yes"),
                ..Default::default()
            };

            for child in measure_node.children() {
                match child.tag_name().name() {
                    "attributes" => {
                        let attr = parse_attributes(child);
                        if measure.attributes.is_none() {
                            measure.attributes = Some(attr.clone());
                        }
                        measure.elements.push(MeasureElement::Attributes(attr));
                    }
                    "note" => measure
                        .elements
                        .push(MeasureElement::Note(parse_note(child))),
                    "backup" => {
                        let duration = child
                            .children()
                            .find(|n| n.has_tag_name("duration"))
                            .and_then(|n| n.text()?.parse().ok())
                            .unwrap_or(0);
                        measure.elements.push(MeasureElement::Backup(duration));
                    }
                    "forward" => {
                        let duration = child
                            .children()
                            .find(|n| n.has_tag_name("duration"))
                            .and_then(|n| n.text()?.parse().ok())
                            .unwrap_or(0);
                        measure.elements.push(MeasureElement::Forward(duration));
                    }
                    "direction" => measure
                        .elements
                        .push(MeasureElement::Direction(parse_direction(child))),
                    "sound" => {
                        let tempo = child.attribute("tempo").and_then(|v| v.parse().ok());
                        let dalsegno = child.attribute("dalsegno").map(|s| s.to_string());
                        let segno = child.attribute("segno").map(|s| s.to_string());
                        let coda = child.attribute("coda").map(|s| s.to_string());
                        let tocoda = child.attribute("tocoda").map(|s| s.to_string());
                        measure
                            .elements
                            .push(MeasureElement::Sound(crate::models::Sound {
                                tempo,
                                dalsegno,
                                segno,
                                coda,
                                tocoda,
                            }));
                    }
                    "harmony" => measure
                        .elements
                        .push(MeasureElement::Harmony(parse_harmony(child))),
                    "figured-bass" => measure
                        .elements
                        .push(MeasureElement::FiguredBass(parse_figured_bass(child))),
                    "grouping" => {
                        let grouping_type = child.attribute("type").unwrap_or("start").to_string();
                        let number = child.attribute("number").and_then(|v| v.parse().ok());
                        let mut features = Vec::new();
                        for f_node in child.children().filter(|n| n.has_tag_name("feature")) {
                            let feature_type =
                                f_node.attribute("type").unwrap_or_default().to_string();
                            let text = f_node.text().unwrap_or_default().to_string();
                            features.push(crate::models::GroupingFeature { feature_type, text });
                        }
                        measure
                            .elements
                            .push(MeasureElement::Grouping(crate::models::Grouping {
                                grouping_type,
                                number,
                                features,
                            }));
                    }
                    "bookmark" => {
                        let id = child.attribute("id").unwrap_or_default().to_string();
                        measure.elements.push(MeasureElement::Bookmark(id));
                    }
                    "barline" => {
                        let location = child.attribute("location").unwrap_or("right").to_string();
                        let bar_style = child
                            .children()
                            .find(|n| n.has_tag_name("bar-style"))
                            .and_then(|n| n.text())
                            .map(|s| s.to_string());
                        let repeat = child
                            .children()
                            .find(|n| n.has_tag_name("repeat"))
                            .map(|n| {
                                let direction =
                                    n.attribute("direction").unwrap_or("forward").to_string();
                                Repeat { direction }
                            });
                        let ending = child
                            .children()
                            .find(|n| n.has_tag_name("ending"))
                            .map(|n| {
                                let number = n.attribute("number").unwrap_or("1").to_string();
                                let ending_type =
                                    n.attribute("type").unwrap_or("start").to_string();
                                let text = n.text().unwrap_or(&number).to_string();
                                crate::models::Ending {
                                    number,
                                    ending_type,
                                    text,
                                }
                            });
                        let fermata = child
                            .children()
                            .find(|n| n.has_tag_name("fermata"))
                            .map(|n| n.attribute("type").unwrap_or("upright").to_string());
                        measure.elements.push(MeasureElement::Barline(Barline {
                            location,
                            bar_style,
                            repeat,
                            ending,
                            fermata,
                        }));
                    }
                    "frame" => {
                        measure
                            .elements
                            .push(MeasureElement::Frame(parse_frame(child)));
                    }
                    _ => {}
                }
            }

            part.measures.push(measure);
        }

        score.parts.push(part);
    }

    crate::auto_beam::expand_double_dotted_notes(&mut score);
    Ok(score)
}

fn parse_attributes(node: Node) -> Attributes {
    let mut attr = Attributes::default();
    if let Some(n) = node.children().find(|n| n.has_tag_name("divisions")) {
        attr.divisions = n.text().and_then(|t| t.parse().ok());
    }
    if let Some(n) = node.children().find(|n| n.has_tag_name("key")) {
        let fifths = n
            .children()
            .find(|c| c.has_tag_name("fifths"))
            .and_then(|c| c.text()?.parse().ok())
            .unwrap_or(0);
        let mode = n
            .children()
            .find(|c| c.has_tag_name("mode"))
            .map(|c| c.text().unwrap_or_default().to_string());

        let mut key_accidentals = Vec::new();
        let steps: Vec<_> = n
            .children()
            .filter(|c| c.has_tag_name("key-step"))
            .collect();
        let alters: Vec<_> = n
            .children()
            .filter(|c| c.has_tag_name("key-alter"))
            .collect();

        for i in 0..steps.len() {
            let step = steps[i].text().unwrap_or_default().to_string();
            let alter = alters
                .get(i)
                .and_then(|c| c.text()?.parse().ok())
                .unwrap_or(0.0);
            key_accidentals.push(crate::models::KeyAccidental {
                step,
                alter,
                octaves: Vec::new(),
            });
        }

        for ko in n.children().filter(|c| c.has_tag_name("key-octave")) {
            let number = ko
                .attribute("number")
                .and_then(|v| v.parse().ok())
                .unwrap_or(1);
            let value = ko.text().and_then(|t| t.parse().ok()).unwrap_or(4);
            if number > 0 && (number as usize) <= key_accidentals.len() {
                key_accidentals[number as usize - 1]
                    .octaves
                    .push(crate::models::KeyOctave {
                        number: Some(number),
                        value,
                    });
            }
        }

        attr.key = Some(Key {
            fifths,
            mode,
            key_accidentals,
        });
    }
    if let Some(n) = node.children().find(|n| n.has_tag_name("transpose")) {
        let diatonic = n
            .children()
            .find(|c| c.has_tag_name("diatonic"))
            .and_then(|c| c.text()?.parse().ok());
        let chromatic = n
            .children()
            .find(|c| c.has_tag_name("chromatic"))
            .and_then(|c| c.text()?.parse().ok());
        let octave_change = n
            .children()
            .find(|c| c.has_tag_name("octave-change"))
            .and_then(|c| c.text()?.parse().ok());
        attr.transpose = Some(crate::models::Transpose {
            diatonic,
            chromatic,
            octave_change,
        });
    }
    if let Some(n) = node.children().find(|n| n.has_tag_name("time")) {
        let beats = n
            .children()
            .find(|c| c.has_tag_name("beats"))
            .and_then(|c| c.text())
            .unwrap_or("4")
            .to_string();
        let beat_type = n
            .children()
            .find(|c| c.has_tag_name("beat-type"))
            .and_then(|c| c.text()?.parse().ok())
            .unwrap_or(4);
        attr.time = Some(Time { beats, beat_type });
    }
    if let Some(n) = node.children().find(|n| n.has_tag_name("staves")) {
        attr.staves = n.text().and_then(|t| t.parse().ok());
    }
    if let Some(n) = node.children().find(|n| n.has_tag_name("part-symbol")) {
        let symbol = match n.text().unwrap_or("none") {
            "brace" => PartSymbol::Brace,
            "bracket" => PartSymbol::Bracket,
            "line" => PartSymbol::Line,
            "none" => PartSymbol::None,
            _ => PartSymbol::None,
        };
        let top_staff = n.attribute("top-staff").and_then(|v| v.parse().ok());
        let bottom_staff = n.attribute("bottom-staff").and_then(|v| v.parse().ok());
        attr.part_symbol = Some(crate::models::PartSymbolMark {
            symbol,
            top_staff,
            bottom_staff,
        });
    }
    for n in node.children().filter(|n| n.has_tag_name("staff-details")) {
        let number = n
            .attribute("number")
            .and_then(|v| v.parse().ok())
            .unwrap_or(1);
        let staff_lines = n
            .children()
            .find(|c| c.has_tag_name("staff-lines"))
            .and_then(|c| c.text()?.parse().ok());
        let mut staff_tunings = Vec::new();
        for t in n.children().filter(|c| c.has_tag_name("staff-tuning")) {
            let line = t
                .attribute("line")
                .and_then(|v| v.parse().ok())
                .unwrap_or(1);
            let step = t
                .children()
                .find(|c| c.has_tag_name("tuning-step"))
                .and_then(|c| c.text())
                .unwrap_or("E")
                .to_string();
            let alter = t
                .children()
                .find(|c| c.has_tag_name("tuning-alter"))
                .and_then(|c| c.text()?.parse().ok());
            let octave = t
                .children()
                .find(|c| c.has_tag_name("tuning-octave"))
                .and_then(|c| c.text()?.parse().ok())
                .unwrap_or(4);
            staff_tunings.push(crate::models::StaffTuning {
                line,
                step,
                alter,
                octave,
            });
        }
        attr.staff_details.push(crate::models::StaffDetails {
            number,
            staff_lines,
            staff_tunings,
        });
    }
    for n in node.children().filter(|n| n.has_tag_name("clef")) {
        let number = n
            .attribute("number")
            .and_then(|v| v.parse().ok())
            .unwrap_or(1);
        let sign = n
            .children()
            .find(|c| c.has_tag_name("sign"))
            .map(|c| c.text().unwrap_or_default().to_string())
            .unwrap_or_default();
        let line = n
            .children()
            .find(|c| c.has_tag_name("line"))
            .and_then(|c| c.text()?.parse().ok());
        let clef_octave_change = n
            .children()
            .find(|c| c.has_tag_name("clef-octave-change"))
            .and_then(|c| c.text()?.parse().ok());
        attr.clefs.push(Clef {
            number,
            sign,
            line,
            clef_octave_change,
        });
    }
    for ms in node.children().filter(|n| n.has_tag_name("measure-style")) {
        if let Some(mr) = ms.children().find(|n| n.has_tag_name("measure-repeat")) {
            let repeat_type = mr.attribute("type").unwrap_or("start").to_string();
            let count = mr.text().and_then(|t| t.parse().ok()).unwrap_or(1);
            attr.measure_repeat = Some(crate::models::MeasureRepeat { repeat_type, count });
        }
        if let Some(br) = ms.children().find(|n| n.has_tag_name("beat-repeat")) {
            let repeat_type = br.attribute("type").unwrap_or("start").to_string();
            let slashes = br
                .attribute("slashes")
                .and_then(|s| s.parse().ok())
                .unwrap_or(1);
            attr.beat_repeat = Some(crate::models::BeatRepeat {
                repeat_type,
                slashes,
            });
        }
        if let Some(sl) = ms.children().find(|n| n.has_tag_name("slash")) {
            let slash_type = sl.attribute("type").unwrap_or("start").to_string();
            let use_stems = sl.attribute("use-stems").map(|s| s == "yes");
            let note_type = sl
                .children()
                .find(|n| n.has_tag_name("slash-type"))
                .and_then(|n| n.text())
                .map(|s| s.to_string());
            let dots = sl
                .children()
                .filter(|n| n.has_tag_name("slash-dot"))
                .count() as i32;
            attr.slash = Some(crate::models::SlashMark {
                slash_type,
                use_stems,
                note_type,
                dots,
            });
        }
        if let Some(mult) = ms.children().find(|n| n.has_tag_name("multiple-rest")) {
            attr.multiple_rest = mult.text().and_then(|t| t.parse().ok());
        }
    }
    if let Some(capo_node) = node.children().find(|n| n.has_tag_name("capo")) {
        attr.capo = capo_node.text().and_then(|t| t.parse().ok());
    }
    attr
}

fn parse_metronome(node: Node) -> MetronomeMark {
    let mut m_mark = MetronomeMark::default();
    m_mark.parentheses = node.attribute("parentheses") == Some("yes");

    let mut seen_first_unit = false;
    for child in node.children() {
        match child.tag_name().name() {
            "beat-unit" => {
                if !seen_first_unit {
                    m_mark.beat_unit = child.text().unwrap_or("quarter").to_string();
                    seen_first_unit = true;
                } else {
                    m_mark.to_beat_unit = Some(child.text().unwrap_or("quarter").to_string());
                }
            }
            "beat-unit-dot" => {
                if m_mark.to_beat_unit.is_some() {
                    m_mark.to_beat_unit_dot += 1;
                } else {
                    m_mark.beat_unit_dot += 1;
                }
            }
            "per-minute" => {
                m_mark.bpm = child.text().map(|t| t.to_string());
            }
            "beat-unit-tied" => {
                m_mark.tied_unit = Some(Box::new(parse_metronome(child)));
            }
            "metronome-note" => {
                let mut mn = crate::models::MetronomeNote::default();
                mn.beat_unit = child
                    .children()
                    .find(|n| n.has_tag_name("metronome-type"))
                    .and_then(|n| n.text())
                    .unwrap_or("quarter")
                    .to_string();
                mn.dots = child
                    .children()
                    .filter(|n| n.has_tag_name("metronome-dot"))
                    .count() as i32;
                for beam_node in child
                    .children()
                    .filter(|n| n.has_tag_name("metronome-beam"))
                {
                    let number = beam_node
                        .attribute("number")
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(1);
                    let val = match beam_node.text().unwrap_or_default() {
                        "begin" => BeamValue::Begin,
                        "continue" => BeamValue::Continue,
                        "end" => BeamValue::End,
                        _ => BeamValue::Begin,
                    };
                    mn.beams.push(Beam { number, value: val });
                }
                if let Some(tuplet_node) = child
                    .children()
                    .find(|n| n.has_tag_name("metronome-tuplet"))
                {
                    let mut t = crate::models::MetronomeTuplet::default();
                    t.tuplet_type = tuplet_node.attribute("type").unwrap_or("start").to_string();
                    t.bracket = tuplet_node.attribute("bracket").map(|s| s.to_string());
                    t.show_number = tuplet_node.attribute("show-number").map(|s| s.to_string());
                    mn.tuplet = Some(t);
                }
                m_mark.metronome_notes.push(mn);
            }
            "metronome-relation" => {
                m_mark.relation = child.text().map(|s| s.to_string());
            }
            _ => {}
        }
    }
    m_mark
}

fn parse_name_display(node: Node) -> crate::models::NameDisplay {
    let mut nd = crate::models::NameDisplay::default();
    for child in node.children() {
        match child.tag_name().name() {
            "display-text" => {
                if let Some(text) = child.text() {
                    nd.texts
                        .push(crate::models::NameDisplayText::Display(text.to_string()));
                }
            }
            "accidental-text" => {
                if let Some(text) = child.text() {
                    nd.texts
                        .push(crate::models::NameDisplayText::Accidental(text.to_string()));
                }
            }
            _ => {}
        }
    }
    nd
}

fn parse_direction(node: Node) -> Direction {
    let mut dir = Direction {
        placement: node.attribute("placement").map(|s| s.to_string()),
        ..Default::default()
    };
    for child in node.children() {
        if child.has_tag_name("direction-type") {
            for dt_child in child.children() {
                let tag_name = dt_child.tag_name().name();
                match tag_name {
                    "words" => {
                        if let Some(text) = dt_child.text() {
                            dir.types.push(DirectionType::Words(text.to_string()));
                        }
                    }
                    "dynamics" => {
                        let mut dynamics = Vec::new();
                        for dyn_child in dt_child.children() {
                            let tag = dyn_child.tag_name().name();
                            if tag.is_empty() {
                                continue;
                            }
                            if tag == "other-dynamics" {
                                // Store the inner text content, not the tag name
                                let inner =
                                    dyn_child.text().unwrap_or("other-dynamics").to_string();
                                dynamics.push(inner);
                            } else {
                                dynamics.push(tag.to_string());
                            }
                        }
                        if !dynamics.is_empty() {
                            dir.types.push(DirectionType::Dynamics(dynamics));
                        }
                    }
                    "metronome" => {
                        dir.types
                            .push(DirectionType::Metronome(parse_metronome(dt_child)));
                    }
                    "coda" => dir.types.push(DirectionType::Coda),
                    "segno" => dir.types.push(DirectionType::Segno),
                    "rehearsal" => {
                        if let Some(text) = dt_child.text() {
                            dir.types.push(DirectionType::Rehearsal(text.to_string()));
                        }
                    }
                    "bracket" => {
                        let bracket_type =
                            dt_child.attribute("type").unwrap_or("start").to_string();
                        let number = dt_child.attribute("number").and_then(|v| v.parse().ok());
                        let line_end = dt_child.attribute("line-end").map(|s| s.to_string());
                        let line_type = dt_child.attribute("line-type").map(|s| s.to_string());
                        dir.types
                            .push(DirectionType::Bracket(crate::models::BracketMark {
                                bracket_type,
                                number,
                                line_end,
                                line_type,
                            }));
                    }
                    "wedge" => {
                        let wedge_type = dt_child
                            .attribute("type")
                            .unwrap_or("crescendo")
                            .to_string();
                        let number = dt_child.attribute("number").and_then(|v| v.parse().ok());
                        let spread = dt_child.attribute("spread").and_then(|v| v.parse().ok());
                        dir.types
                            .push(DirectionType::Wedge(crate::models::WedgeMark {
                                wedge_type,
                                number,
                                spread,
                            }));
                    }
                    "octave-shift" => {
                        let shift_type = dt_child.attribute("type").unwrap_or("up").to_string();
                        let number = dt_child.attribute("number").and_then(|v| v.parse().ok());
                        let size = dt_child
                            .attribute("size")
                            .and_then(|v| v.parse().ok())
                            .unwrap_or(8);
                        dir.types.push(DirectionType::OctaveShift(
                            crate::models::OctaveShiftMark {
                                shift_type,
                                number,
                                size,
                            },
                        ));
                    }
                    "pedal" => {
                        let pedal_type = dt_child.attribute("type").unwrap_or("start").to_string();
                        let line = dt_child.attribute("line") == Some("yes");
                        let number = dt_child.attribute("number").and_then(|v| v.parse().ok());
                        dir.types
                            .push(DirectionType::Pedal(crate::models::PedalMark {
                                pedal_type,
                                line,
                                number,
                            }));
                    }
                    "damp" => dir.types.push(DirectionType::Damp),
                    "damp-all" => dir.types.push(DirectionType::DampAll),
                    "dashes" => {
                        let dashes_type = dt_child.attribute("type").unwrap_or("start").to_string();
                        let number = dt_child.attribute("number").and_then(|v| v.parse().ok());
                        let dash_length = dt_child
                            .attribute("dash-length")
                            .and_then(|v| v.parse().ok());
                        let space_length = dt_child
                            .attribute("space-length")
                            .and_then(|v| v.parse().ok());
                        dir.types
                            .push(DirectionType::Dashes(crate::models::DashesMark {
                                dashes_type,
                                number,
                                dash_length,
                                space_length,
                            }));
                    }
                    "" => {} // Skip text nodes
                    _ => {
                        dir.types.push(DirectionType::Other(tag_name.to_string()));
                    }
                }
            }
        } else if child.has_tag_name("staff") {
            dir.staff = child.text().and_then(|s| s.parse().ok());
        }
    }
    dir
}

fn parse_frame(node: Node) -> Frame {
    let mut frame = crate::models::Frame::default();
    frame.strings = node
        .children()
        .find(|n| n.has_tag_name("frame-strings"))
        .and_then(|n| n.text()?.parse().ok())
        .unwrap_or(6);
    frame.frets = node
        .children()
        .find(|n| n.has_tag_name("frame-freets") || n.has_tag_name("frame-frets"))
        .and_then(|n| n.text()?.parse().ok())
        .unwrap_or(5);
    if let Some(ff) = node.children().find(|n| n.has_tag_name("first-fret")) {
        frame.first_fret = ff.text().and_then(|t| t.parse().ok());
        frame.first_fret_text = ff.attribute("text").map(|s| s.to_string());
    }
    for fn_node in node.children().filter(|n| n.has_tag_name("frame-note")) {
        let mut frame_note = crate::models::FrameNote::default();
        frame_note.string = fn_node
            .children()
            .find(|n| n.has_tag_name("string"))
            .and_then(|n| n.text()?.parse().ok())
            .unwrap_or(1);
        frame_note.fret = fn_node
            .children()
            .find(|n| n.has_tag_name("fret"))
            .and_then(|n| n.text()?.parse().ok())
            .unwrap_or(0);
        frame_note.fingering = fn_node
            .children()
            .find(|n| n.has_tag_name("fingering"))
            .and_then(|n| n.text())
            .map(|s| s.to_string());
        frame_note.barre = fn_node
            .children()
            .find(|n| n.has_tag_name("barre"))
            .and_then(|n| n.attribute("type"))
            .map(|s| s.to_string());
        frame.notes.push(frame_note);
    }
    frame
}

fn parse_harmony(node: Node) -> Harmony {
    let mut h = Harmony::default();
    if let Some(root) = node.children().find(|n| n.has_tag_name("root")) {
        h.root_step = root
            .children()
            .find(|n| n.has_tag_name("root-step"))
            .and_then(|n| n.text())
            .unwrap_or("C")
            .to_string();
        h.root_alter = root
            .children()
            .find(|n| n.has_tag_name("root-alter"))
            .and_then(|n| n.text()?.parse().ok());
    }
    if let Some(kind) = node.children().find(|n| n.has_tag_name("kind")) {
        h.kind = kind.text().unwrap_or("major").to_string();
        h.kind_text = kind.attribute("text").map(|s| s.to_string());
        h.use_symbols = kind.attribute("use-symbols") == Some("yes");
    }
    if let Some(bass) = node.children().find(|n| n.has_tag_name("bass")) {
        h.bass_step = bass
            .children()
            .find(|n| n.has_tag_name("bass-step"))
            .and_then(|n| n.text())
            .map(|s| s.to_string());
        h.bass_alter = bass
            .children()
            .find(|n| n.has_tag_name("bass-alter"))
            .and_then(|n| n.text()?.parse().ok());
        h.bass_separator = bass
            .children()
            .find(|n| n.has_tag_name("bass-separator"))
            .and_then(|n| n.text())
            .map(|s| s.to_string());
    }
    for d_node in node.children().filter(|n| n.has_tag_name("degree")) {
        let value = d_node
            .children()
            .find(|n| n.has_tag_name("degree-value"))
            .and_then(|n| n.text()?.parse().ok())
            .unwrap_or(0);
        let alter = d_node
            .children()
            .find(|n| n.has_tag_name("degree-alter"))
            .and_then(|n| n.text()?.parse().ok())
            .unwrap_or(0.0);
        let dt_node = d_node.children().find(|n| n.has_tag_name("degree-type"));
        let degree_type = dt_node.and_then(|n| n.text()).unwrap_or("add").to_string();
        let type_text = dt_node
            .and_then(|n| n.attribute("text"))
            .map(|s| s.to_string());
        h.degrees.push(crate::models::Degree {
            value,
            alter,
            degree_type,
            type_text,
        });
    }
    if let Some(frame_node) = node.children().find(|n| n.has_tag_name("frame")) {
        h.frame = Some(parse_frame(frame_node));
    }
    if let Some(numeral_node) = node.children().find(|n| n.has_tag_name("numeral")) {
        let mut numeral = crate::models::Numeral::default();
        if let Some(root) = numeral_node
            .children()
            .find(|n| n.has_tag_name("numeral-root"))
        {
            numeral.root_value = root.text().and_then(|t| t.parse().ok()).unwrap_or(0);
            numeral.root_text = root.attribute("text").map(|s| s.to_string());
        }
        if let Some(alt) = numeral_node
            .children()
            .find(|n| n.has_tag_name("numeral-alter"))
        {
            let value = alt.text().and_then(|t| t.parse().ok()).unwrap_or(0.0);
            let location = alt.attribute("location").map(|s| s.to_string());
            numeral.root_alter = Some(crate::models::NumeralAlter { value, location });
        }
        h.numeral = Some(numeral);
    }
    h.inversion = node
        .children()
        .find(|n| n.has_tag_name("inversion"))
        .and_then(|n| n.text()?.parse().ok());
    h
}

fn parse_figured_bass(node: Node) -> FiguredBass {
    let mut fb = FiguredBass::default();
    fb.default_y = node.attribute("default-y").and_then(|v| v.parse().ok());
    for child in node.children() {
        if child.has_tag_name("figure") {
            let mut figure = crate::models::Figure::default();
            figure.prefix = child
                .children()
                .find(|n| n.has_tag_name("prefix"))
                .and_then(|n| n.text())
                .map(|s| s.to_string());
            figure.number = child
                .children()
                .find(|n| n.has_tag_name("figure-number"))
                .and_then(|n| n.text())
                .map(|s| s.to_string());
            figure.suffix = child
                .children()
                .find(|n| n.has_tag_name("suffix"))
                .and_then(|n| n.text())
                .map(|s| s.to_string());
            figure.extend = child
                .children()
                .find(|n| n.has_tag_name("extend"))
                .and_then(|n| n.attribute("type"))
                .map(|s| s.to_string());
            fb.figures.push(figure);
        }
    }
    fb
}

fn parse_note(node: Node) -> Note {
    let mut pitch = None;
    let mut unpitched = None;
    let mut duration = 0;
    let mut voice = None;
    let mut staff = None;
    let mut stem = None;
    let mut note_type = None;
    let mut notehead = None;
    let mut rest = false;
    let mut rest_measure = false;
    let mut is_chord = false;
    let mut is_cue = false;
    let mut grace = None;
    let mut dot_count = 0;
    let mut lyrics = Vec::new();
    let mut beams = Vec::new();
    let mut notations = Vec::new();
    let mut accidental = None;
    let mut time_modification = None;
    let mut harmonies = Vec::new();
    let mut instrument = None;
    let print_object = node.attribute("print-object").map(|v| v == "yes");
    let print_dot = node.attribute("print-dot").map(|v| v == "yes");
    let note_pizzicato = node
        .attribute("pizzicato")
        .map(|v| v == "yes")
        .unwrap_or(false);

    for child in node.children() {
        match child.tag_name().name() {
            "instrument" => instrument = child.attribute("id").map(|s| s.to_string()),
            "grace" => {
                let slash = child.attribute("slash").map(|v| v.to_string());
                grace = Some(crate::models::Grace { slash });
            }
            "pitch" => {
                let step = child
                    .children()
                    .find(|n| n.has_tag_name("step"))
                    .map(|n| n.text().unwrap_or_default().to_string())
                    .unwrap_or_default();
                let octave = child
                    .children()
                    .find(|n| n.has_tag_name("octave"))
                    .and_then(|n| n.text()?.parse().ok())
                    .unwrap_or(0);
                let alter = child
                    .children()
                    .find(|n| n.has_tag_name("alter"))
                    .and_then(|n| n.text()?.parse().ok());
                pitch = Some(Pitch {
                    step,
                    octave,
                    alter,
                });
            }
            "unpitched" => {
                let step = child
                    .children()
                    .find(|n| n.has_tag_name("display-step"))
                    .map(|n| n.text().unwrap_or_default().to_string())
                    .unwrap_or_default();
                let octave = child
                    .children()
                    .find(|n| n.has_tag_name("display-octave"))
                    .and_then(|n| n.text()?.parse().ok())
                    .unwrap_or(0);
                unpitched = Some(crate::models::Unpitched {
                    display_step: step,
                    display_octave: octave,
                    midi_number: None,
                });
            }
            "duration" => duration = child.text().and_then(|t| t.parse().ok()).unwrap_or(0),
            "voice" => voice = child.text().and_then(|t| t.parse().ok()),
            "staff" => staff = child.text().and_then(|t| t.parse().ok()),
            "stem" => stem = child.text().map(|t| t.to_string()),
            "type" => note_type = child.text().map(|t| t.to_string()),
            "notehead" => {
                let value = child.text().unwrap_or("normal").to_string();
                let filled = child.attribute("filled").map(|v| v == "yes");
                notehead = Some(crate::models::Notehead { value, filled });
            }
            "harmony" => harmonies.push(parse_harmony(child)),
            "time-modification" => {
                let actual = child
                    .children()
                    .find(|n| n.has_tag_name("actual-notes"))
                    .and_then(|n| n.text()?.parse().ok())
                    .unwrap_or(1);
                let normal = child
                    .children()
                    .find(|n| n.has_tag_name("normal-notes"))
                    .and_then(|n| n.text()?.parse().ok())
                    .unwrap_or(1);
                let normal_type = child
                    .children()
                    .find(|n| n.has_tag_name("normal-type"))
                    .map(|n| n.text().unwrap_or_default().to_string());
                let normal_dot_count = child
                    .children()
                    .filter(|n| n.has_tag_name("normal-dot"))
                    .count() as i32;
                time_modification = Some(TimeModification {
                    actual_notes: actual,
                    normal_notes: normal,
                    normal_type,
                    normal_dot_count,
                });
            }
            "rest" => {
                rest = true;
                if child.attribute("measure") == Some("yes") {
                    rest_measure = true;
                }
                let step = child
                    .children()
                    .find(|n| n.has_tag_name("display-step"))
                    .map(|n| n.text().unwrap_or_default().to_string());
                let octave = child
                    .children()
                    .find(|n| n.has_tag_name("display-octave"))
                    .and_then(|n| n.text()?.parse().ok());
                if let (Some(s), Some(o)) = (step, octave) {
                    unpitched = Some(crate::models::Unpitched {
                        display_step: s,
                        display_octave: o,
                        midi_number: None,
                    });
                }
            }
            "chord" => is_chord = true,
            "cue" => is_cue = true,
            "dot" => dot_count += 1,
            "lyric" => {
                let mut l = crate::models::Lyric::default();
                l.number = child.attribute("number").and_then(|v| v.parse().ok());
                if let Some(text_node) = child.children().find(|n| n.has_tag_name("text")) {
                    l.text = text_node.text().unwrap_or_default().to_string();
                }
                l.syllabic = child
                    .children()
                    .find(|n| n.has_tag_name("syllabic"))
                    .and_then(|n| n.text())
                    .map(|s| s.to_string());
                l.extend = child
                    .children()
                    .find(|n| n.has_tag_name("extend"))
                    .and_then(|n| n.attribute("type"))
                    .map(|s| s.to_string());
                l.end_line = child.children().any(|n| n.has_tag_name("end-line"));
                lyrics.push(l);
            }
            "technical" => {
                let mut technical = Vec::new();
                for t_child in child.children() {
                    match t_child.tag_name().name() {
                        "up-bow" => technical.push(crate::models::TechnicalMark::UpBow {
                            placement: t_child.attribute("placement").map(|s| s.to_string()),
                        }),
                        "down-bow" => technical.push(crate::models::TechnicalMark::DownBow {
                            placement: t_child.attribute("placement").map(|s| s.to_string()),
                        }),
                        "harmonic" => {
                            let is_artificial =
                                t_child.children().any(|n| n.has_tag_name("artificial"));
                            let is_natural = t_child.children().any(|n| n.has_tag_name("natural"));
                            let pitch_type = t_child
                                .children()
                                .find(|n| {
                                    n.has_tag_name("base-pitch")
                                        || n.has_tag_name("touch-pitch")
                                        || n.has_tag_name("sounding-pitch")
                                })
                                .map(|n| n.tag_name().name().to_string());
                            technical.push(crate::models::TechnicalMark::Harmonic(
                                crate::models::Harmonic {
                                    is_artificial,
                                    is_natural,
                                    pitch_type,
                                },
                            ));
                        }
                        "arrow" => {
                            let direction = t_child
                                .children()
                                .find(|n| n.has_tag_name("arrow-direction"))
                                .and_then(|n| n.text())
                                .unwrap_or("up")
                                .to_string();
                            let style = t_child
                                .children()
                                .find(|n| n.has_tag_name("arrow-style"))
                                .and_then(|n| n.text())
                                .map(|s| s.to_string());
                            let placement = t_child.attribute("placement").map(|s| s.to_string());
                            let arrowhead = t_child.children().any(|n| n.has_tag_name("arrowhead"));
                            technical.push(crate::models::TechnicalMark::Arrow(
                                crate::models::Arrow {
                                    direction,
                                    style,
                                    placement,
                                    has_arrowhead: arrowhead,
                                },
                            ));
                        }
                        "handbell" => {
                            let value = t_child.text().unwrap_or_default().to_string();
                            let placement = t_child.attribute("placement").map(|s| s.to_string());
                            technical
                                .push(crate::models::TechnicalMark::Handbell { value, placement });
                        }
                        "brass-bend" => {
                            let placement = t_child.attribute("placement").map(|s| s.to_string());
                            technical.push(crate::models::TechnicalMark::BrassBend { placement });
                        }
                        "flip" => {
                            let placement = t_child.attribute("placement").map(|s| s.to_string());
                            technical.push(crate::models::TechnicalMark::Flip { placement });
                        }
                        "golpe" => {
                            let placement = t_child.attribute("placement").map(|s| s.to_string());
                            technical.push(crate::models::TechnicalMark::Golpe { placement });
                        }
                        "half-muted" => {
                            let placement = t_child.attribute("placement").map(|s| s.to_string());
                            technical.push(crate::models::TechnicalMark::HalfMuted { placement });
                        }
                        "harmon-mute" => {
                            let mut closed = None;
                            if let Some(closed_node) =
                                t_child.children().find(|n| n.has_tag_name("harmon-closed"))
                            {
                                closed = Some(closed_node.text().unwrap_or("yes").to_string());
                            }
                            let placement = t_child.attribute("placement").map(|s| s.to_string());
                            technical.push(crate::models::TechnicalMark::HarmonMute {
                                closed,
                                placement,
                            });
                        }
                        "heel" => {
                            let placement = t_child.attribute("placement").map(|s| s.to_string());
                            let substitution =
                                t_child.attribute("substitution").map(|s| s == "yes");
                            technical.push(crate::models::TechnicalMark::Heel {
                                placement,
                                substitution,
                            });
                        }
                        "toe" => {
                            let placement = t_child.attribute("placement").map(|s| s.to_string());
                            let substitution =
                                t_child.attribute("substitution").map(|s| s == "yes");
                            technical.push(crate::models::TechnicalMark::Toe {
                                placement,
                                substitution,
                            });
                        }
                        "hole" => {
                            let placement = t_child.attribute("placement").map(|s| s.to_string());
                            let mut content = "open".to_string();
                            if let Some(closed) =
                                t_child.children().find(|n| n.has_tag_name("hole-closed"))
                            {
                                if closed.text() == Some("yes") {
                                    content = "closed".to_string();
                                } else if closed.text() == Some("half") {
                                    content = "half".to_string();
                                }
                            }
                            if t_child.children().any(|n| n.has_tag_name("hole-open")) {
                                content = "open".to_string();
                            }
                            technical
                                .push(crate::models::TechnicalMark::Hole { content, placement });
                        }
                        "open" => {
                            let placement = t_child.attribute("placement").map(|s| s.to_string());
                            technical.push(crate::models::TechnicalMark::Open { placement });
                        }
                        "open-string" => {
                            let placement = t_child.attribute("placement").map(|s| s.to_string());
                            technical.push(crate::models::TechnicalMark::OpenString { placement });
                        }
                        "pluck" => {
                            let text = t_child.text().unwrap_or_default().to_string();
                            let placement = t_child.attribute("placement").map(|s| s.to_string());
                            let default_x =
                                t_child.attribute("default-x").and_then(|v| v.parse().ok());
                            let default_y =
                                t_child.attribute("default-y").and_then(|v| v.parse().ok());
                            technical.push(crate::models::TechnicalMark::Pluck {
                                text,
                                placement,
                                default_x,
                                default_y,
                            });
                        }
                        "smear" => {
                            let placement = t_child.attribute("placement").map(|s| s.to_string());
                            technical.push(crate::models::TechnicalMark::Smear { placement });
                        }
                        "pizzicato" => {
                            let placement = t_child.attribute("placement").map(|s| s.to_string());
                            technical.push(crate::models::TechnicalMark::Pizzicato { placement });
                        }
                        "snap-pizzicato" => {
                            let placement = t_child.attribute("placement").map(|s| s.to_string());
                            technical
                                .push(crate::models::TechnicalMark::SnapPizzicato { placement });
                        }
                        "fret" => {
                            if let Some(f) = t_child.text().and_then(|s| s.parse().ok()) {
                                technical.push(crate::models::TechnicalMark::Fret(f));
                            }
                        }
                        "string" => {
                            if let Some(s) = t_child.text().and_then(|s| s.parse().ok()) {
                                technical.push(crate::models::TechnicalMark::String(s));
                            }
                        }
                        "hammer-on" => {
                            let number = t_child
                                .attribute("number")
                                .and_then(|v| v.parse().ok())
                                .unwrap_or(1);
                            let mark_type =
                                t_child.attribute("type").unwrap_or("start").to_string();
                            let text = t_child.text().unwrap_or_default().to_string();
                            technical.push(crate::models::TechnicalMark::HammerOn {
                                number,
                                mark_type,
                                text,
                            });
                        }
                        "pull-off" => {
                            let number = t_child
                                .attribute("number")
                                .and_then(|v| v.parse().ok())
                                .unwrap_or(1);
                            let mark_type =
                                t_child.attribute("type").unwrap_or("start").to_string();
                            let text = t_child.text().unwrap_or_default().to_string();
                            technical.push(crate::models::TechnicalMark::PullOff {
                                number,
                                mark_type,
                                text,
                            });
                        }
                        "tap" => {
                            let hand = t_child.attribute("hand").map(|s| s.to_string());
                            let placement = t_child.attribute("placement").map(|s| s.to_string());
                            technical.push(crate::models::TechnicalMark::Tap { hand, placement });
                        }
                        "thumb-position" => {
                            let placement = t_child.attribute("placement").map(|s| s.to_string());
                            technical
                                .push(crate::models::TechnicalMark::ThumbPosition { placement });
                        }
                        "stopped" => {
                            let placement = t_child.attribute("placement").map(|s| s.to_string());
                            technical.push(crate::models::TechnicalMark::Stopped { placement });
                        }
                        "bend" => {
                            let bend_alter = t_child
                                .children()
                                .find(|n| n.has_tag_name("bend-alter"))
                                .and_then(|n| n.text())
                                .and_then(|s| s.parse().ok())
                                .unwrap_or(0.0);
                            let placement = t_child.attribute("placement").map(|s| s.to_string());
                            let pre_bend = t_child.children().any(|n| n.has_tag_name("pre-bend"));
                            let release = t_child.children().any(|n| n.has_tag_name("release"));
                            let with_bar = t_child
                                .children()
                                .find(|n| n.has_tag_name("with-bar"))
                                .map(|n| crate::models::WithBar {
                                    value: n.text().unwrap_or_default().to_string(),
                                    placement: n.attribute("placement").map(|s| s.to_string()),
                                });
                            technical.push(crate::models::TechnicalMark::Bend(vec![
                                crate::models::BendMark {
                                    bend_alter,
                                    pre_bend,
                                    release,
                                    with_bar,
                                    placement,
                                },
                            ]));
                        }
                        "other-technical" => {
                            let text = t_child.text().unwrap_or_default().to_string();
                            let placement = t_child.attribute("placement").map(|s| s.to_string());
                            technical.push(crate::models::TechnicalMark::OtherTechnical {
                                text,
                                placement,
                            });
                        }
                        "notations" | "ornaments" | "articulations" => {
                            // Handle non-standard nested structures by processing their children
                            for sub_child in t_child.children() {
                                if sub_child.has_tag_name("bend") {
                                    let bend_alter = sub_child
                                        .children()
                                        .find(|n| n.has_tag_name("bend-alter"))
                                        .and_then(|n| n.text())
                                        .and_then(|s| s.parse().ok())
                                        .unwrap_or(0.0);
                                    let placement =
                                        sub_child.attribute("placement").map(|s| s.to_string());
                                    let pre_bend =
                                        sub_child.children().any(|n| n.has_tag_name("pre-bend"));
                                    let release =
                                        sub_child.children().any(|n| n.has_tag_name("release"));
                                    let with_bar = sub_child
                                        .children()
                                        .find(|n| n.has_tag_name("with-bar"))
                                        .map(|n| crate::models::WithBar {
                                            value: n.text().unwrap_or_default().to_string(),
                                            placement: n
                                                .attribute("placement")
                                                .map(|s| s.to_string()),
                                        });
                                    let mark = crate::models::BendMark {
                                        bend_alter,
                                        pre_bend,
                                        release,
                                        with_bar,
                                        placement,
                                    };
                                    let mut found = false;
                                    for m in &mut technical {
                                        if let crate::models::TechnicalMark::Bend(v) = m {
                                            v.push(mark.clone());
                                            found = true;
                                            break;
                                        }
                                    }
                                    if !found {
                                        technical
                                            .push(crate::models::TechnicalMark::Bend(vec![mark]));
                                    }
                                }
                            }
                        }
                        name if !name.is_empty() => {
                            technical.push(crate::models::TechnicalMark::Other(name.to_string()))
                        }
                        _ => {}
                    }
                }
                if !technical.is_empty() {
                    notations.push(Notation::Technical(technical));
                }
            }
            "beam" => {
                let number = child
                    .attribute("number")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(1);
                let bt = match child.text().unwrap_or_default() {
                    "begin" => Some(BeamValue::Begin),
                    "continue" => Some(BeamValue::Continue),
                    "end" => Some(BeamValue::End),
                    "forward hook" => Some(BeamValue::ForwardHook),
                    "backward hook" => Some(BeamValue::BackwardHook),
                    _ => None,
                };
                if let Some(val) = bt {
                    beams.push(Beam { number, value: val });
                }
            }
            "accidental" => accidental = child.text().map(|t| t.to_string()),
            "notations" => {
                for n_child in child.children() {
                    match n_child.tag_name().name() {
                        "slur" => {
                            let number = n_child
                                .attribute("number")
                                .and_then(|v| v.parse().ok())
                                .unwrap_or(1);
                            let note_type =
                                n_child.attribute("type").unwrap_or("start").to_string();
                            // `placement` ("above"/"below") and `orientation`
                            // ("over"/"under") are two different MusicXML attributes
                            // that exporters use interchangeably to say the same thing
                            // for a slur; fall back to orientation when placement is
                            // absent so both are honored.
                            let placement = n_child
                                .attribute("placement")
                                .map(|s| s.to_string())
                                .or_else(|| match n_child.attribute("orientation") {
                                    Some("over") => Some("above".to_string()),
                                    Some("under") => Some("below".to_string()),
                                    _ => None,
                                });
                            notations.push(Notation::Slur {
                                number,
                                note_type,
                                placement,
                            });
                        }
                        "tied" => {
                            let note_type =
                                n_child.attribute("type").unwrap_or("start").to_string();
                            notations.push(Notation::Tied { note_type });
                        }
                        "tuplet" => {
                            let number = n_child.attribute("number").and_then(|v| v.parse().ok());
                            let note_type =
                                n_child.attribute("type").unwrap_or("start").to_string();
                            let bracket = n_child.attribute("bracket").map(|s| s.to_string());
                            let placement = n_child.attribute("placement").map(|s| s.to_string());
                            let show_number =
                                n_child.attribute("show-number").map(|s| s.to_string());
                            let actual_notes = n_child
                                .children()
                                .find(|n| n.has_tag_name("tuplet-actual"))
                                .and_then(|n| {
                                    n.children().find(|c| c.has_tag_name("tuplet-number"))
                                })
                                .and_then(|c| c.text()?.parse().ok());
                            let normal_notes = n_child
                                .children()
                                .find(|n| n.has_tag_name("tuplet-normal"))
                                .and_then(|n| {
                                    n.children().find(|c| c.has_tag_name("tuplet-number"))
                                })
                                .and_then(|c| c.text()?.parse().ok());
                            notations.push(Notation::Tuplet {
                                number,
                                note_type,
                                bracket,
                                placement,
                                show_number,
                                actual_notes,
                                normal_notes,
                            });
                        }
                        "fermata" => {
                            let note_type = n_child.attribute("type").map(|s| s.to_string());
                            let placement = n_child.attribute("placement").map(|s| s.to_string());
                            notations.push(Notation::Fermata {
                                note_type,
                                placement,
                            });
                        }
                        "articulations" => {
                            for art in n_child.children() {
                                let name = art.tag_name().name();
                                if !name.is_empty() {
                                    let placement =
                                        art.attribute("placement").map(|s| s.to_string());
                                    let default_x =
                                        art.attribute("default-x").and_then(|v| v.parse().ok());
                                    let default_y =
                                        art.attribute("default-y").and_then(|v| v.parse().ok());
                                    notations.push(Notation::Articulation {
                                        name: name.to_string(),
                                        placement,
                                        default_x,
                                        default_y,
                                    });
                                }
                            }
                        }
                        "arpeggiate" => {
                            let number = n_child.attribute("number").and_then(|v| v.parse().ok());
                            let direction = n_child.attribute("direction").map(|s| s.to_string());
                            notations.push(Notation::Arpeggiate { number, direction });
                        }
                        "non-arpeggiate" => {
                            let number = n_child.attribute("number").and_then(|v| v.parse().ok());
                            let non_arp_type =
                                n_child.attribute("type").unwrap_or("top").to_string();
                            notations.push(Notation::NonArpeggiate {
                                number,
                                non_arp_type,
                            });
                        }
                        "accidental-mark" => {
                            let value = n_child.text().unwrap_or_default().to_string();
                            let placement = n_child.attribute("placement").map(|s| s.to_string());
                            notations.push(Notation::AccidentalMark(
                                crate::models::AccidentalMark { value, placement },
                            ));
                        }
                        "ornaments" => {
                            let mut ornaments = Vec::new();
                            for o_child in n_child.children() {
                                match o_child.tag_name().name() {
                                    "trill-mark" => {
                                        ornaments.push(crate::models::Ornament::TrillMark)
                                    }
                                    "turn" => ornaments.push(crate::models::Ornament::Turn),
                                    "delayed-turn" => {
                                        ornaments.push(crate::models::Ornament::DelayedTurn)
                                    }
                                    "inverted-turn" => {
                                        ornaments.push(crate::models::Ornament::InvertedTurn)
                                    }
                                    "delayed-inverted-turn" => {
                                        ornaments.push(crate::models::Ornament::DelayedInvertedTurn)
                                    }
                                    "vertical-turn" => {
                                        ornaments.push(crate::models::Ornament::VerticalTurn)
                                    }
                                    "inverted-vertical-turn" => ornaments
                                        .push(crate::models::Ornament::InvertedVerticalTurn),
                                    "mordent" => {
                                        let long = o_child.attribute("long") == Some("yes");
                                        ornaments.push(crate::models::Ornament::Mordent { long });
                                    }
                                    "inverted-mordent" => {
                                        let long = o_child.attribute("long") == Some("yes");
                                        ornaments.push(crate::models::Ornament::InvertedMordent {
                                            long,
                                        });
                                    }
                                    "haydn" => ornaments.push(crate::models::Ornament::Haydn),
                                    "schleifer" => {
                                        let placement =
                                            o_child.attribute("placement").map(|s| s.to_string());
                                        ornaments
                                            .push(crate::models::Ornament::Schleifer { placement });
                                    }
                                    "shake" => {
                                        let placement =
                                            o_child.attribute("placement").map(|s| s.to_string());
                                        ornaments
                                            .push(crate::models::Ornament::Shake { placement });
                                    }
                                    "tremolo" => {
                                        let tremolo_type = o_child
                                            .attribute("type")
                                            .unwrap_or("single")
                                            .to_string();
                                        let bars = o_child
                                            .text()
                                            .and_then(|s| s.parse().ok())
                                            .unwrap_or(1);
                                        ornaments.push(crate::models::Ornament::Tremolo {
                                            tremolo_type,
                                            bars,
                                        });
                                    }
                                    "wavy-line" => {
                                        let wavy_type = o_child
                                            .attribute("type")
                                            .unwrap_or("start")
                                            .to_string();
                                        let number = o_child
                                            .attribute("number")
                                            .and_then(|v| v.parse().ok())
                                            .unwrap_or(1);
                                        let relative_x = o_child
                                            .attribute("relative-x")
                                            .and_then(|v| v.parse().ok());
                                        ornaments.push(crate::models::Ornament::WavyLine {
                                            wavy_type,
                                            number,
                                            relative_x,
                                        });
                                    }
                                    "accidental-mark" => {
                                        let value = o_child.text().unwrap_or_default().to_string();
                                        let placement =
                                            o_child.attribute("placement").map(|s| s.to_string());
                                        ornaments.push(crate::models::Ornament::AccidentalMark(
                                            crate::models::AccidentalMark { value, placement },
                                        ));
                                    }
                                    name if !name.is_empty() => ornaments
                                        .push(crate::models::Ornament::Other(name.to_string())),
                                    _ => {}
                                }
                            }
                            if !ornaments.is_empty() {
                                notations.push(Notation::Ornaments(ornaments));
                            }
                        }
                        "technical" => {
                            let mut technical = Vec::new();
                            for t_child in n_child.children() {
                                match t_child.tag_name().name() {
                                    "arrow" => {
                                        let direction = t_child
                                            .children()
                                            .find(|n| n.has_tag_name("arrow-direction"))
                                            .and_then(|n| n.text())
                                            .unwrap_or("up")
                                            .to_string();
                                        let style = t_child
                                            .children()
                                            .find(|n| n.has_tag_name("arrow-style"))
                                            .and_then(|n| n.text())
                                            .map(|s| s.to_string());
                                        let placement =
                                            t_child.attribute("placement").map(|s| s.to_string());
                                        let has_arrowhead =
                                            t_child.children().any(|n| n.has_tag_name("arrowhead"));
                                        technical.push(crate::models::TechnicalMark::Arrow(
                                            crate::models::Arrow {
                                                direction,
                                                style,
                                                placement,
                                                has_arrowhead,
                                            },
                                        ));
                                    }
                                    "harmonic" => {
                                        let is_artificial = t_child
                                            .children()
                                            .any(|n| n.has_tag_name("artificial"));
                                        let is_natural =
                                            t_child.children().any(|n| n.has_tag_name("natural"));
                                        let pitch_type = t_child
                                            .children()
                                            .find(|n| {
                                                matches!(
                                                    n.tag_name().name(),
                                                    "base-pitch"
                                                        | "touching-pitch"
                                                        | "sounding-pitch"
                                                )
                                            })
                                            .map(|n| n.tag_name().name().to_string());
                                        technical.push(crate::models::TechnicalMark::Harmonic(
                                            crate::models::Harmonic {
                                                is_artificial,
                                                is_natural,
                                                pitch_type,
                                            },
                                        ));
                                    }
                                    "brass-bend" => {
                                        let placement =
                                            t_child.attribute("placement").map(|s| s.to_string());
                                        technical.push(crate::models::TechnicalMark::BrassBend {
                                            placement,
                                        });
                                    }
                                    "double-tongue" => {
                                        let placement =
                                            t_child.attribute("placement").map(|s| s.to_string());
                                        technical.push(
                                            crate::models::TechnicalMark::DoubleTongue {
                                                placement,
                                            },
                                        );
                                    }
                                    "triple-tongue" => {
                                        let placement =
                                            t_child.attribute("placement").map(|s| s.to_string());
                                        technical.push(
                                            crate::models::TechnicalMark::TripleTongue {
                                                placement,
                                            },
                                        );
                                    }
                                    "down-bow" => {
                                        let placement =
                                            t_child.attribute("placement").map(|s| s.to_string());
                                        technical.push(crate::models::TechnicalMark::DownBow {
                                            placement,
                                        });
                                    }
                                    "up-bow" => {
                                        let placement =
                                            t_child.attribute("placement").map(|s| s.to_string());
                                        technical.push(crate::models::TechnicalMark::UpBow {
                                            placement,
                                        });
                                    }
                                    "fingering" => {
                                        let text = t_child.text().unwrap_or_default().to_string();
                                        let placement =
                                            t_child.attribute("placement").map(|s| s.to_string());
                                        technical.push(crate::models::TechnicalMark::Fingering {
                                            text,
                                            placement,
                                        });
                                    }
                                    "fingernails" => {
                                        let placement =
                                            t_child.attribute("placement").map(|s| s.to_string());
                                        technical.push(crate::models::TechnicalMark::Fingernails {
                                            placement,
                                        });
                                    }
                                    "flip" => {
                                        let placement =
                                            t_child.attribute("placement").map(|s| s.to_string());
                                        technical
                                            .push(crate::models::TechnicalMark::Flip { placement });
                                    }
                                    "golpe" => {
                                        let placement =
                                            t_child.attribute("placement").map(|s| s.to_string());
                                        technical.push(crate::models::TechnicalMark::Golpe {
                                            placement,
                                        });
                                    }
                                    "half-muted" => {
                                        let placement =
                                            t_child.attribute("placement").map(|s| s.to_string());
                                        technical.push(crate::models::TechnicalMark::HalfMuted {
                                            placement,
                                        });
                                    }
                                    "handbell" => {
                                        let value = t_child.text().unwrap_or_default().to_string();
                                        let placement =
                                            t_child.attribute("placement").map(|s| s.to_string());
                                        technical.push(crate::models::TechnicalMark::Handbell {
                                            value,
                                            placement,
                                        });
                                    }
                                    "harmon-mute" => {
                                        let closed = t_child
                                            .children()
                                            .find(|n| n.has_tag_name("harmon-closed"))
                                            .and_then(|n| n.text())
                                            .map(|s| s.to_string());
                                        let placement =
                                            t_child.attribute("placement").map(|s| s.to_string());
                                        technical.push(crate::models::TechnicalMark::HarmonMute {
                                            closed,
                                            placement,
                                        });
                                    }
                                    "heel" => {
                                        let placement =
                                            t_child.attribute("placement").map(|s| s.to_string());
                                        let substitution =
                                            t_child.attribute("substitution").map(|s| s == "yes");
                                        technical.push(crate::models::TechnicalMark::Heel {
                                            placement,
                                            substitution,
                                        });
                                    }
                                    "toe" => {
                                        let placement =
                                            t_child.attribute("placement").map(|s| s.to_string());
                                        let substitution =
                                            t_child.attribute("substitution").map(|s| s == "yes");
                                        technical.push(crate::models::TechnicalMark::Toe {
                                            placement,
                                            substitution,
                                        });
                                    }
                                    "hole" => {
                                        let placement =
                                            t_child.attribute("placement").map(|s| s.to_string());
                                        let mut content = "open".to_string();
                                        if let Some(closed) = t_child
                                            .children()
                                            .find(|n| n.has_tag_name("hole-closed"))
                                        {
                                            if closed.text() == Some("yes") {
                                                content = "closed".to_string();
                                            } else if closed.text() == Some("half") {
                                                content = "half".to_string();
                                            }
                                        }
                                        if t_child.children().any(|n| n.has_tag_name("hole-open")) {
                                            content = "open".to_string();
                                        }
                                        technical.push(crate::models::TechnicalMark::Hole {
                                            content,
                                            placement,
                                        });
                                    }
                                    "open" => {
                                        let placement =
                                            t_child.attribute("placement").map(|s| s.to_string());
                                        technical
                                            .push(crate::models::TechnicalMark::Open { placement });
                                    }
                                    "open-string" => {
                                        let placement =
                                            t_child.attribute("placement").map(|s| s.to_string());
                                        technical.push(crate::models::TechnicalMark::OpenString {
                                            placement,
                                        });
                                    }
                                    "pluck" => {
                                        let text = t_child.text().unwrap_or_default().to_string();
                                        let placement =
                                            t_child.attribute("placement").map(|s| s.to_string());
                                        let default_x = t_child
                                            .attribute("default-x")
                                            .and_then(|v| v.parse().ok());
                                        let default_y = t_child
                                            .attribute("default-y")
                                            .and_then(|v| v.parse().ok());
                                        technical.push(crate::models::TechnicalMark::Pluck {
                                            text,
                                            placement,
                                            default_x,
                                            default_y,
                                        });
                                    }
                                    "smear" => {
                                        let placement =
                                            t_child.attribute("placement").map(|s| s.to_string());
                                        technical.push(crate::models::TechnicalMark::Smear {
                                            placement,
                                        });
                                    }
                                    "pizzicato" => {
                                        let placement =
                                            t_child.attribute("placement").map(|s| s.to_string());
                                        technical.push(crate::models::TechnicalMark::Pizzicato {
                                            placement,
                                        });
                                    }
                                    "snap-pizzicato" => {
                                        let placement =
                                            t_child.attribute("placement").map(|s| s.to_string());
                                        technical.push(
                                            crate::models::TechnicalMark::SnapPizzicato {
                                                placement,
                                            },
                                        );
                                    }
                                    "fret" => {
                                        if let Some(f) = t_child.text().and_then(|s| s.parse().ok())
                                        {
                                            technical.push(crate::models::TechnicalMark::Fret(f));
                                        }
                                    }
                                    "string" => {
                                        if let Some(s) = t_child.text().and_then(|s| s.parse().ok())
                                        {
                                            technical.push(crate::models::TechnicalMark::String(s));
                                        }
                                    }
                                    "hammer-on" => {
                                        let number = t_child
                                            .attribute("number")
                                            .and_then(|v| v.parse().ok())
                                            .unwrap_or(1);
                                        let mark_type = t_child
                                            .attribute("type")
                                            .unwrap_or("start")
                                            .to_string();
                                        let text = t_child.text().unwrap_or_default().to_string();
                                        technical.push(crate::models::TechnicalMark::HammerOn {
                                            number,
                                            mark_type,
                                            text,
                                        });
                                    }
                                    "pull-off" => {
                                        let number = t_child
                                            .attribute("number")
                                            .and_then(|v| v.parse().ok())
                                            .unwrap_or(1);
                                        let mark_type = t_child
                                            .attribute("type")
                                            .unwrap_or("start")
                                            .to_string();
                                        let text = t_child.text().unwrap_or_default().to_string();
                                        technical.push(crate::models::TechnicalMark::PullOff {
                                            number,
                                            mark_type,
                                            text,
                                        });
                                    }
                                    "tap" => {
                                        let hand = t_child.attribute("hand").map(|s| s.to_string());
                                        let placement =
                                            t_child.attribute("placement").map(|s| s.to_string());
                                        technical.push(crate::models::TechnicalMark::Tap {
                                            hand,
                                            placement,
                                        });
                                    }
                                    "thumb-position" => {
                                        let placement =
                                            t_child.attribute("placement").map(|s| s.to_string());
                                        technical.push(
                                            crate::models::TechnicalMark::ThumbPosition {
                                                placement,
                                            },
                                        );
                                    }
                                    "stopped" => {
                                        let placement =
                                            t_child.attribute("placement").map(|s| s.to_string());
                                        technical.push(crate::models::TechnicalMark::Stopped {
                                            placement,
                                        });
                                    }
                                    "bend" => {
                                        let bend_alter = t_child
                                            .children()
                                            .find(|n| n.has_tag_name("bend-alter"))
                                            .and_then(|n| n.text())
                                            .and_then(|s| s.parse().ok())
                                            .unwrap_or(0.0);
                                        let placement =
                                            t_child.attribute("placement").map(|s| s.to_string());
                                        let pre_bend =
                                            t_child.children().any(|n| n.has_tag_name("pre-bend"));
                                        let release =
                                            t_child.children().any(|n| n.has_tag_name("release"));
                                        let with_bar = t_child
                                            .children()
                                            .find(|n| n.has_tag_name("with-bar"))
                                            .map(|n| crate::models::WithBar {
                                                value: n.text().unwrap_or_default().to_string(),
                                                placement: n
                                                    .attribute("placement")
                                                    .map(|s| s.to_string()),
                                            });
                                        let mark = crate::models::BendMark {
                                            bend_alter,
                                            pre_bend,
                                            release,
                                            with_bar,
                                            placement,
                                        };

                                        // Find existing Bend vec or create new
                                        let mut found = false;
                                        for m in &mut technical {
                                            if let crate::models::TechnicalMark::Bend(v) = m {
                                                v.push(mark.clone());
                                                found = true;
                                                break;
                                            }
                                        }
                                        if !found {
                                            technical.push(crate::models::TechnicalMark::Bend(
                                                vec![mark],
                                            ));
                                        }
                                    }
                                    "other-technical" => {
                                        let text = t_child.text().unwrap_or_default().to_string();
                                        let placement =
                                            t_child.attribute("placement").map(|s| s.to_string());
                                        technical.push(
                                            crate::models::TechnicalMark::OtherTechnical {
                                                text,
                                                placement,
                                            },
                                        );
                                    }
                                    "notations" | "ornaments" | "articulations" => {
                                        // Handle non-standard nested structures by processing their children
                                        for sub_child in t_child.children() {
                                            if sub_child.has_tag_name("bend") {
                                                let bend_alter = sub_child
                                                    .children()
                                                    .find(|n| n.has_tag_name("bend-alter"))
                                                    .and_then(|n| n.text())
                                                    .and_then(|s| s.parse().ok())
                                                    .unwrap_or(0.0);
                                                let placement = sub_child
                                                    .attribute("placement")
                                                    .map(|s| s.to_string());
                                                let pre_bend = sub_child
                                                    .children()
                                                    .any(|n| n.has_tag_name("pre-bend"));
                                                let release = sub_child
                                                    .children()
                                                    .any(|n| n.has_tag_name("release"));
                                                let with_bar = sub_child
                                                    .children()
                                                    .find(|n| n.has_tag_name("with-bar"))
                                                    .map(|n| crate::models::WithBar {
                                                        value: n
                                                            .text()
                                                            .unwrap_or_default()
                                                            .to_string(),
                                                        placement: n
                                                            .attribute("placement")
                                                            .map(|s| s.to_string()),
                                                    });
                                                let mark = crate::models::BendMark {
                                                    bend_alter,
                                                    pre_bend,
                                                    release,
                                                    with_bar,
                                                    placement,
                                                };
                                                let mut found = false;
                                                for m in &mut technical {
                                                    if let crate::models::TechnicalMark::Bend(v) = m
                                                    {
                                                        v.push(mark.clone());
                                                        found = true;
                                                        break;
                                                    }
                                                }
                                                if !found {
                                                    technical.push(
                                                        crate::models::TechnicalMark::Bend(vec![
                                                            mark,
                                                        ]),
                                                    );
                                                }
                                            }
                                        }
                                    }
                                    name if !name.is_empty() => technical.push(
                                        crate::models::TechnicalMark::Other(name.to_string()),
                                    ),
                                    _ => {}
                                }
                            }
                            if !technical.is_empty() {
                                notations.push(Notation::Technical(technical));
                            }
                        }
                        "glissando" => {
                            let gliss_type =
                                n_child.attribute("type").unwrap_or("start").to_string();
                            let number = n_child
                                .attribute("number")
                                .and_then(|v| v.parse().ok())
                                .unwrap_or(1);
                            let line_type = n_child.attribute("line-type").map(|s| s.to_string());
                            let text = n_child.text().map(|s| s.to_string());
                            notations.push(Notation::Glissando(crate::models::GlissandoMark {
                                gliss_type,
                                number,
                                line_type,
                                text,
                            }));
                        }
                        "slide" => {
                            let slide_type =
                                n_child.attribute("type").unwrap_or("start").to_string();
                            let number = n_child
                                .attribute("number")
                                .and_then(|v| v.parse().ok())
                                .unwrap_or(1);
                            let line_type = n_child.attribute("line-type").map(|s| s.to_string());
                            notations.push(Notation::Slide(crate::models::SlideMark {
                                slide_type,
                                number,
                                line_type,
                            }));
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    // <note pizzicato="yes"> injects a Pizzicato technical mark so the MIDI engine
    // can emit a program change to pizzicato strings (GM 45) for these notes.
    if note_pizzicato {
        notations.push(crate::models::Notation::Technical(vec![
            crate::models::TechnicalMark::Pizzicato { placement: None },
        ]));
    }

    Note {
        pitch,
        unpitched,
        duration,
        voice,
        staff,
        stem,
        note_type,
        notehead,
        rest,
        rest_measure,
        is_chord,
        is_cue,
        grace,
        dot_count,
        lyrics,
        beams,
        notations,
        accidental,
        time_modification,
        print_object,
        print_dot,
        harmonies,
        instrument,
    }
}
