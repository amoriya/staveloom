#!/usr/bin/env bash
# Generate a WOFF2 subset of Bravura containing only the SMuFL codepoints
# used by the staveloom-core renderer (~212 glyphs, ~28 KB vs 501 KB full font).
#
# Usage: bash scripts/generate_bravura_subset.sh [path/to/Bravura.otf]
#
# Dependencies: pip install fonttools brotli
# Output: web/fonts/Bravura-subset.woff2

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
OUT_DIR="$ROOT_DIR/web/fonts"

# Default OTF path — override via first argument
BRAVURA_OTF="${1:-/usr/share/fonts/OTF/Bravura.otf}"

if [[ ! -f "$BRAVURA_OTF" ]]; then
    echo "ERROR: Bravura.otf not found at: $BRAVURA_OTF"
    echo "       Download from https://github.com/steinbergmedia/bravura/releases"
    echo "       or install via your package manager, then pass the path as argument:"
    echo "         bash $0 /path/to/Bravura.otf"
    exit 1
fi

# SMuFL codepoints used by crates/staveloom-core/src/renderer/
# Extracted via:
#   grep -rn '\\u{[0-9A-Fa-f]\{4,\}}' crates/staveloom-core/src/renderer/ \
#     | grep -oP '\\u\{([0-9A-Fa-f]+)\}' | grep -oP '[0-9A-Fa-f]+' \
#     | awk '{printf "U+%s,", toupper($0)}' | sort -u
UNICODES="\
U+006F,U+00F8,U+1D1B4,U+2206,U+266D,\
U+E000,U+E002,U+E003,U+E004,U+E042,U+E047,U+E048,\
U+E050,U+E05C,U+E062,U+E069,U+E06D,U+E088,\
U+E0A1,U+E0A2,U+E0A3,U+E0A4,U+E0AC,U+E0AF,U+E0BE,\
U+E0D0,U+E0DA,U+E0DB,U+E0DD,U+E101,U+E120,\
U+E1D1,U+E1D2,U+E1D3,U+E1D5,U+E1D7,U+E1D9,U+E1DB,U+E1DD,\
U+E220,U+E221,U+E222,U+E223,U+E224,\
U+E240,U+E241,U+E242,U+E243,U+E244,U+E245,U+E246,U+E247,\
U+E260,U+E261,U+E262,U+E263,U+E264,\
U+E280,U+E281,U+E282,U+E283,\
U+E4A0,U+E4A1,U+E4A2,U+E4A3,U+E4A4,U+E4A5,U+E4A6,U+E4A7,U+E4A8,U+E4A9,\
U+E4AC,U+E4AD,U+E4B2,U+E4B6,U+E4B7,U+E4B8,U+E4B9,\
U+E4C0,U+E4C1,U+E4CE,U+E4D1,\
U+E4E3,U+E4E4,U+E4E5,U+E4E6,U+E4E7,U+E4E8,U+E4E9,U+E4EA,U+E4EB,U+E4EC,U+E4ED,\
U+E500,U+E501,U+E502,U+E503,U+E504,\
U+E520,U+E521,U+E522,U+E523,U+E524,U+E525,U+E526,U+E527,U+E528,U+E529,\
U+E52A,U+E52B,U+E52C,U+E52D,U+E52E,U+E52F,U+E530,U+E531,U+E532,U+E533,\
U+E534,U+E535,U+E536,U+E537,U+E538,U+E539,U+E53A,U+E53B,U+E53C,U+E53D,\
U+E566,U+E567,U+E568,U+E56A,U+E56B,U+E56C,U+E56D,U+E56E,U+E56F,U+E587,\
U+E5D0,U+E5D2,U+E5DE,U+E5E1,U+E5E2,U+E5E3,U+E5E6,U+E5E7,U+E5E8,U+E5E9,\
U+E5F0,U+E5F2,U+E5F4,U+E5F8,U+E5F9,\
U+E610,U+E612,U+E614,U+E624,U+E630,U+E631,U+E636,U+E639,U+E63C,\
U+E650,U+E651,U+E655,U+E661,U+E665,U+E674,U+E675,\
U+E810,U+E821,U+E822,U+E823,U+E824,U+E825,U+E826,U+E827,U+E828,U+E829,U+E831,\
U+E840,U+E841,U+E842,U+E870,U+E871,U+E873,\
U+EB60,U+EB61,U+EB62,U+EB63,U+EB64,U+EB65,U+EB66,U+EB67,\
U+EB70,U+EB71,U+EB72,U+EB73,U+EB74,U+EB75,U+EB76,U+EB77,U+EB78,U+EB79,\
U+EB7A,U+EB7B,U+EB7C,U+EB7D,U+EB7E,U+EB7F,\
U+ED40,U+ED41"

mkdir -p "$OUT_DIR"
OUT="$OUT_DIR/Bravura-subset.woff2"

echo "Input:  $BRAVURA_OTF ($(du -h "$BRAVURA_OTF" | cut -f1))"
echo "Output: $OUT"
echo "Glyphs: 212 SMuFL codepoints used by staveloom-core renderer"
echo ""

pyftsubset "$BRAVURA_OTF" \
    --unicodes="$UNICODES" \
    --flavor=woff2 \
    --output-file="$OUT"

SIZE=$(wc -c < "$OUT")
echo "Done — $(( SIZE / 1024 )) KB (${SIZE} bytes)"
