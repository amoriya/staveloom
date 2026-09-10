#!/bin/bash
set -e

# 여러 샘플에 대해 CLI SVG와 WASM SVG를 비교
SAMPLES=(
    "tests/samples/test/01a-Pitches-Pitches.xml"
    "tests/samples/test/03aa-Rhythm-Durations.xml"
    "tests/samples/test/21a-Chords.xml"
    "tests/samples/test/31a-Directions.xml"
)

PASS=0
FAIL=0

echo "Building CLI release binary..."
cargo build -p staveloom-cli --release

for sample in "${SAMPLES[@]}"; do
    name=$(basename "$sample" .xml)
    echo "Comparing outputs for $name..."

    # CLI 렌더링
    cargo run -p staveloom-cli --release -- "$sample" \
        --output "/tmp/cli_${name}.svg" \
        --metadata "/tmp/cli_${name}_meta.json" \
        --midi "/tmp/cli_${name}.mid" 2>/dev/null

    # WASM 렌더링
    node tests/scripts/wasm_render.mjs "$sample" \
        "/tmp/wasm_${name}.svg" \
        "/tmp/wasm_${name}_meta.json" \
        "/tmp/wasm_${name}.mid"

    # SVG 비교 (HashMap 순서 비결정성 대응을 위해 정렬 후 비교)
    sort "/tmp/cli_${name}.svg" > "/tmp/cli_${name}_sorted.svg"
    sort "/tmp/wasm_${name}.svg" > "/tmp/wasm_${name}_sorted.svg"
    if diff -q "/tmp/cli_${name}_sorted.svg" "/tmp/wasm_${name}_sorted.svg" > /dev/null 2>&1; then
        echo "  [PASS] SVG matches"
        PASS=$((PASS + 1))
    else
        echo "  [FAIL] SVG mismatch!"
        FAIL=$((FAIL + 1))
    fi

    # Metadata beats 수 비교
    CLI_BEATS=$(python3 -c "import json; d=json.load(open('/tmp/cli_${name}_meta.json')); print(len(d.get('beats',[])))")
    WASM_BEATS=$(python3 -c "import json; d=json.load(open('/tmp/wasm_${name}_meta.json')); print(len(d.get('beats',[])))")
    if [ "$CLI_BEATS" = "$WASM_BEATS" ]; then
        echo "  [PASS] Metadata beats match ($CLI_BEATS)"
        PASS=$((PASS + 1))
    else
        echo "  [FAIL] Metadata beats mismatch! CLI=$CLI_BEATS, WASM=$WASM_BEATS"
        FAIL=$((FAIL + 1))
    fi

    # MIDI 바이트 비교
    if cmp -s "/tmp/cli_${name}.mid" "/tmp/wasm_${name}.mid"; then
        echo "  [PASS] MIDI matches"
        PASS=$((PASS + 1))
    else
        echo "  [FAIL] MIDI mismatch!"
        FAIL=$((FAIL + 1))
    fi
done

echo ""
echo "Comparison Results: $PASS passed, $FAIL failed"
[ "$FAIL" -eq 0 ] && exit 0 || exit 1
