import init, { parse_and_render, list_parts, list_instruments } from '../../../web/pkg/staveloom_wasm.js';
import fs from 'fs';
import path from 'path';
import { fileURLToPath } from 'url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

let failures = 0;
function assert(cond, msg) {
    if (!cond) {
        console.error(`  FAIL: ${msg}`);
        failures++;
    }
}

async function runTests() {
    console.log('Loading WASM module...');
    const wasmPath = path.resolve(__dirname, '../../../web/pkg/staveloom_wasm_bg.wasm');
    const wasmBytes = fs.readFileSync(wasmPath);
    await init(wasmBytes);
    console.log('WASM module loaded successfully.\n');

    const sampleDir = path.resolve(__dirname, '../../../tests/samples');
    const xmlPath   = path.join(sampleDir, 'test/01a-Pitches-Pitches.xml');
    const xmlBytes  = new Uint8Array(fs.readFileSync(xmlPath));

    // ── Test 1: RenderResult structure ──────────────────────────────────────
    console.log('Test 1: parse_and_render returns correct top-level structure...');
    const result = parse_and_render(xmlBytes, false, false, 1200, '');
    assert(typeof result === 'object' && result !== null, 'result is an object');
    assert(Array.isArray(result.systems),  'result.systems is an array');
    assert(result.systems.length >= 1,     'at least one system returned');
    assert(typeof result.metadata === 'object', 'metadata is an object');
    assert(Array.isArray(result.metadata.beats), 'metadata.beats is an array');
    assert(Array.isArray(result.midi),     'midi is an array');
    assert(result.midi.length > 0,         'midi is non-empty');
    console.log(`  Systems: ${result.systems.length}`);
    console.log('Test 1 passed.\n');

    // ── Test 2: Per-system SVG structure ─────────────────────────────────────
    console.log('Test 2: Each SystemSvg has required fields and valid SVG...');
    for (let i = 0; i < result.systems.length; i++) {
        const sys = result.systems[i];
        assert(sys.index === i,                       `systems[${i}].index === ${i}`);
        assert(typeof sys.svg_content === 'string',   `systems[${i}].svg_content is string`);
        assert(sys.svg_content.startsWith('<svg'),    `systems[${i}].svg_content starts with <svg`);
        assert(sys.svg_content.includes('</svg>'),    `systems[${i}].svg_content ends with </svg>`);
        assert(typeof sys.y_offset === 'number',      `systems[${i}].y_offset is number`);
        assert(typeof sys.height === 'number' && sys.height > 0, `systems[${i}].height > 0`);
        assert(typeof sys.width === 'number' && sys.width > 0,   `systems[${i}].width > 0`);
        assert(typeof sys.measure_start === 'number', `systems[${i}].measure_start is number`);
        assert(typeof sys.measure_end === 'number',   `systems[${i}].measure_end is number`);
        assert(sys.measure_end >= sys.measure_start,  `systems[${i}].measure_end >= measure_start`);
    }
    console.log('Test 2 passed.\n');

    // ── Test 3: system_boundaries in metadata ────────────────────────────────
    console.log('Test 3: metadata.system_boundaries matches systems array...');
    assert(Array.isArray(result.metadata.system_boundaries), 'system_boundaries is an array');
    assert(result.metadata.system_boundaries.length === result.systems.length,
        `system_boundaries.length (${result.metadata.system_boundaries.length}) === systems.length (${result.systems.length})`);
    for (let i = 0; i < result.metadata.system_boundaries.length; i++) {
        const b = result.metadata.system_boundaries[i];
        assert(typeof b.y_start === 'number',      `boundary[${i}].y_start is number`);
        assert(typeof b.height === 'number' && b.height > 0, `boundary[${i}].height > 0`);
        assert(typeof b.measure_start === 'number', `boundary[${i}].measure_start is number`);
        assert(typeof b.measure_end === 'number',   `boundary[${i}].measure_end is number`);
    }
    console.log('Test 3 passed.\n');

    // ── Test 4: system_index in beats ────────────────────────────────────────
    console.log('Test 4: BeatEvent.system_index is present and in range...');
    const n = result.metadata.system_boundaries.length;
    let beatsChecked = 0;
    for (const beat of result.metadata.beats) {
        assert(typeof beat.system_index === 'number', 'beat.system_index is number');
        assert(beat.system_index >= 0 && beat.system_index < n,
            `beat.system_index (${beat.system_index}) in [0, ${n})`);
        beatsChecked++;
    }
    console.log(`  Checked ${beatsChecked} beats`);
    console.log('Test 4 passed.\n');

    // ── Test 5: Multi-system: per-system SVG viewBox is cropped ──────────────
    console.log('Test 5: Per-system SVG viewBox origin matches boundary.y_start...');
    if (result.systems.length > 1) {
        for (let i = 0; i < result.systems.length; i++) {
            const sys = result.systems[i];
            const bound = result.metadata.system_boundaries[i];
            const vbMatch = sys.svg_content.match(/viewBox="([^"]+)"/);
            assert(vbMatch !== null, `systems[${i}] has viewBox attribute`);
            if (vbMatch) {
                const [minX, minY] = vbMatch[1].split(/\s+/).map(Number);
                assert(minX === 0, `systems[${i}] viewBox minX === 0`);
                assert(Math.abs(minY - bound.y_start) < 1,
                    `systems[${i}] viewBox minY (${minY}) ≈ boundary.y_start (${bound.y_start})`);
            }
        }
        console.log('Test 5 passed.\n');
    } else {
        console.log('Test 5 skipped (single system — need longer score for multi-system test).\n');
    }

    // ── Test 6: MXL parsing ──────────────────────────────────────────────────
    console.log('Test 6: MXL zip archive parsed correctly...');
    const mxlPath = path.join(sampleDir, 'xmlsamples/MozartTrio.mxl');
    const mxlBytes = new Uint8Array(fs.readFileSync(mxlPath));
    const mxlResult = parse_and_render(mxlBytes, false, false, 1200, '');
    assert(Array.isArray(mxlResult.systems) && mxlResult.systems.length >= 1, 'MXL returns at least one system');
    assert(mxlResult.systems[0].svg_content.startsWith('<svg'), 'MXL system[0] is valid SVG');
    console.log(`  MXL systems: ${mxlResult.systems.length}`);
    console.log('Test 6 passed.\n');

    // ── Test 7: list_parts ───────────────────────────────────────────────────
    console.log('Test 7: list_parts returns expected structure...');
    const parts = list_parts(xmlBytes);
    assert(Array.isArray(parts), 'parts is an array');
    assert(parts.length > 0, 'parts is non-empty');
    assert(parts[0].id === 'P1', `parts[0].id === 'P1'`);
    assert(typeof parts[0].name === 'string', 'parts[0].name is string');
    console.log('  Parts:', parts);
    console.log('Test 7 passed.\n');

    // ── Test 8: list_instruments ─────────────────────────────────────────────
    console.log('Test 8: list_instruments returns expected structure...');
    const instruments = list_instruments(xmlBytes);
    assert(Array.isArray(instruments), 'instruments is an array');
    assert(instruments.length > 0, 'instruments is non-empty');
    assert(typeof instruments[0].program === 'number', 'instruments[0].program is number');
    assert(instruments[0].program === 0, '01a-Pitches-Pitches instrument is program 0 (Piano)');
    console.log('  Instruments:', instruments);
    console.log('Test 8 passed.\n');

    // ── Test 9: Horizontal mode (single-system path) ─────────────────────────
    console.log('Test 9: Horizontal mode returns a single full-SVG system...');
    const hResult = parse_and_render(xmlBytes, false, true, 0, '');
    assert(hResult.systems.length === 1, 'horizontal mode produces exactly 1 system');
    assert(hResult.systems[0].index === 0, 'horizontal system[0].index === 0');
    assert(hResult.systems[0].y_offset === 0, 'horizontal system[0].y_offset === 0');
    console.log('Test 9 passed.\n');

    // ── Test 10: Part filtering ──────────────────────────────────────────────
    console.log('Test 10: Part filtering works...');
    const filtResult = parse_and_render(xmlBytes, true, false, 1200, 'P1');
    assert(Array.isArray(filtResult.systems) && filtResult.systems.length >= 1, 'filtered result has systems');
    console.log('Test 10 passed.\n');

    // ── Summary ───────────────────────────────────────────────────────────────
    if (failures > 0) {
        console.error(`\n${failures} assertion(s) failed.`);
        process.exit(1);
    } else {
        console.log('All WASM binding tests passed successfully!');
    }
}

runTests().catch(err => {
    console.error('Test failed with error:', err);
    process.exit(1);
});
