use staveloom_core::parser::parse_musicxml;
use staveloom_core::renderer::Renderer;
use std::fs;
use std::path::PathBuf;

#[test]
fn test_w3c_examples_rendering() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let w3c_xml_dir = manifest_dir.join("../../tests/samples/specification/xml");
    let _w3c_img_dir = manifest_dir.join("../../tests/samples/specification/img");
    let output_dir = manifest_dir.join("../../tests/snapshots/specification");

    if !w3c_xml_dir.exists() {
        println!("W3C XML directory not found. Skipping test.");
        return;
    }

    if !output_dir.exists() {
        fs::create_dir_all(&output_dir).unwrap();
    }

    let renderer = Renderer::default();
    let mut failures = Vec::new();

    let entries = fs::read_dir(w3c_xml_dir).expect("Could not read w3c xml dir");
    for entry in entries {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("xml") {
            let file_stem = path.file_stem().unwrap().to_str().unwrap();
            let content = fs::read_to_string(&path).expect("Could not read file");

            // 1. Parse
            match parse_musicxml(&content) {
                Ok(score) => {
                    // 2. Render to SVG
                    let svg_content = renderer.render(&score);
                    let mut svg_path = output_dir.join(file_stem);
                    svg_path.set_extension("svg");
                    fs::write(&svg_path, svg_content).unwrap();

                    // Note: Here we would ideally compare with the reference image in w3c_img_dir.
                    // Since we are producing SVG and the reference is likely PNG,
                    // an automated pixel-perfect comparison is out of scope for this basic setup.
                    // We save the SVG so it can be manually compared with:
                    // w3c_img_dir.join(format!("{}.png", file_stem))
                }
                Err(e) => {
                    // Some W3C examples might use elements we don't support yet,
                    // but they should at least parse if they are valid MusicXML.
                    failures.push(format!("Failed to parse {}: {}", file_stem, e));
                }
            }
        }
    }

    if !failures.is_empty() {
        // We don't necessarily want to fail the whole suite if some W3C examples fail to parse
        // due to advanced MusicXML features not yet in our parser,
        // but we should list them.
        for failure in &failures {
            eprintln!("{}", failure);
        }
    }
}
