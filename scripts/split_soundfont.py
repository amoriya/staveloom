#!/usr/bin/env python3
"""
SF2 SoundFont Splitter — splits FluidR3 GM.sf2 into per-instrument SF2 files.

Usage:
    python3 scripts/split_soundfont.py [input.sf2] [output_dir]

Defaults:
    input:      soundfont/FluidR3 GM.sf2 (not bundled in the repo — supply
                your own copy of FluidR3 GM.sf2, or pass a path explicitly)
    output_dir: web/soundfonts/instruments/

Each GM preset (bank 0, program 0-127) and drum kit (bank 128, program 0)
is extracted into a separate SF2 file with only the sample data it needs.

After running this script, update web/soundfonts/index.json to reference
the individual files instead of full_gm.sf2 for faster per-instrument loading.
"""

import json
import os
import struct
import sys


def riff_chunks(data, offset=0, end=None):
    """Iterate over RIFF sub-chunks: yields (name, content, chunk_start)."""
    if end is None:
        end = len(data)
    while offset + 8 <= end:
        name = data[offset : offset + 4].decode("ascii", errors="replace")
        size = struct.unpack_from("<I", data, offset + 4)[0]
        content_start = offset + 8
        content_end = content_start + size
        yield name, data[content_start:content_end], offset
        offset = content_end + (size & 1)  # word-align


def parse_sf2(data):
    """Parse SF2 and return structured data."""
    assert data[:4] == b"RIFF", "Not a RIFF file"
    assert data[8:12] == b"sfbk", "Not an SF2 file"

    chunks = {}
    for name, content, _ in riff_chunks(data, 12, len(data)):
        if name == "LIST":
            list_id = content[:4].decode("ascii", errors="replace")
            sub = {}
            for sname, scontent, _ in riff_chunks(content, 4):
                sub[sname] = scontent
            chunks[list_id] = sub
        else:
            chunks[name] = content

    info = chunks.get("INFO", {})
    sdta = chunks.get("sdta", {})
    pdta = chunks.get("pdta", {})

    smpl = sdta.get("smpl", b"")
    sm24 = sdta.get("sm24", b"")

    # Parse phdr (38 bytes each)
    phdr_raw = pdta.get("phdr", b"")
    phdr = []
    for i in range(0, len(phdr_raw) - 37, 38):
        name = phdr_raw[i : i + 20].split(b"\x00")[0].decode("latin-1")
        preset, bank, bag_ndx = struct.unpack_from("<HHH", phdr_raw, i + 20)
        phdr.append({"name": name, "preset": preset, "bank": bank, "bag_ndx": bag_ndx})

    # Parse pbag (4 bytes each)
    pbag_raw = pdta.get("pbag", b"")
    pbag = []
    for i in range(0, len(pbag_raw), 4):
        gen_ndx, mod_ndx = struct.unpack_from("<HH", pbag_raw, i)
        pbag.append({"gen_ndx": gen_ndx, "mod_ndx": mod_ndx})

    # Parse pmod (10 bytes each)
    pmod_raw = pdta.get("pmod", b"")
    pmod = (
        list(struct.iter_unpack("<HHHH", pmod_raw[:-2])) if len(pmod_raw) > 10 else []
    )

    # Parse pgen (4 bytes each)
    pgen_raw = pdta.get("pgen", b"")
    pgen = []
    for i in range(0, len(pgen_raw), 4):
        oper, amount = struct.unpack_from("<HH", pgen_raw, i)
        pgen.append({"oper": oper, "amount": amount})

    # Parse inst (22 bytes each)
    inst_raw = pdta.get("inst", b"")
    inst = []
    for i in range(0, len(inst_raw), 22):
        name = inst_raw[i : i + 20].split(b"\x00")[0].decode("latin-1")
        bag_ndx = struct.unpack_from("<H", inst_raw, i + 20)[0]
        inst.append({"name": name, "bag_ndx": bag_ndx})

    # Parse ibag (4 bytes each)
    ibag_raw = pdta.get("ibag", b"")
    ibag = []
    for i in range(0, len(ibag_raw), 4):
        gen_ndx, mod_ndx = struct.unpack_from("<HH", ibag_raw, i)
        ibag.append({"gen_ndx": gen_ndx, "mod_ndx": mod_ndx})

    # Parse imod (10 bytes each)
    imod_raw = pdta.get("imod", b"")

    # Parse igen (4 bytes each)
    igen_raw = pdta.get("igen", b"")
    igen = []
    for i in range(0, len(igen_raw), 4):
        oper, amount = struct.unpack_from("<HH", igen_raw, i)
        igen.append({"oper": oper, "amount": amount, "raw": igen_raw[i : i + 4]})

    # Parse shdr (46 bytes each)
    shdr_raw = pdta.get("shdr", b"")
    shdr = []
    for i in range(0, len(shdr_raw), 46):
        name = shdr_raw[i : i + 20].split(b"\x00")[0].decode("latin-1")
        start, end, loop_start, loop_end, rate = struct.unpack_from(
            "<IIIII", shdr_raw, i + 20
        )
        pitch, correction = struct.unpack_from("<Bb", shdr_raw, i + 40)
        link, stype = struct.unpack_from("<HH", shdr_raw, i + 42)
        shdr.append(
            {
                "name": name,
                "start": start,
                "end": end,
                "loop_start": loop_start,
                "loop_end": loop_end,
                "rate": rate,
                "pitch": pitch,
                "correction": correction,
                "link": link,
                "type": stype,
                "raw": shdr_raw[i : i + 46],
            }
        )

    return {
        "info": info,
        "smpl": smpl,
        "sm24": sm24,
        "phdr": phdr,
        "pbag": pbag,
        "pmod_raw": pmod_raw,
        "pgen": pgen,
        "inst": inst,
        "ibag": ibag,
        "imod_raw": imod_raw,
        "igen": igen,
        "shdr": shdr,
        "pdta": pdta,
    }


def get_preset_samples(sf, bank, program):
    """Return set of sample indices used by a preset (bank, program)."""
    phdr = sf["phdr"]
    pbag = sf["pbag"]
    pgen = sf["pgen"]
    inst = sf["inst"]
    ibag = sf["ibag"]
    igen = sf["igen"]

    # Find the preset
    preset_entry = None
    for i, p in enumerate(phdr[:-1]):
        if p["bank"] == bank and p["preset"] == program:
            preset_entry = (p, phdr[i + 1])
            break
    if not preset_entry:
        return set()

    p, p_next = preset_entry
    sample_ids = set()
    inst_ids = set()

    # Collect instrument references from preset generators
    for bi in range(p["bag_ndx"], p_next["bag_ndx"]):
        g_start = pbag[bi]["gen_ndx"]
        g_end = pbag[bi + 1]["gen_ndx"] if bi + 1 < len(pbag) else len(pgen)
        for gi in range(g_start, g_end):
            if pgen[gi]["oper"] == 41:  # instrument
                inst_ids.add(pgen[gi]["amount"])

    # Collect sample references from instrument generators
    for iid in inst_ids:
        if iid >= len(inst) - 1:
            continue
        i_start = inst[iid]["bag_ndx"]
        i_end = inst[iid + 1]["bag_ndx"]
        for bi in range(i_start, i_end):
            g_start = ibag[bi]["gen_ndx"]
            g_end = ibag[bi + 1]["gen_ndx"] if bi + 1 < len(ibag) else len(igen)
            for gi in range(g_start, g_end):
                if igen[gi]["oper"] == 53:  # sampleID
                    sample_ids.add(igen[gi]["amount"])

    return sample_ids, inst_ids


def make_sf2_name(s, length):
    """Encode a string into a null-padded fixed-length bytes field."""
    b = s.encode("latin-1", errors="replace")[: length - 1]
    return b.ljust(length, b"\x00")


def build_sf2(sf, bank, program):
    """Build a new SF2 bytes for a single preset.

    SF2 spec field sizes (all little-endian):
      phdr entry : 38 bytes  (name[20] + wPreset[2] + wBank[2] + wPresetBagNdx[2] + lib[4] + genre[4] + morph[4])
      pbag entry :  4 bytes  (wPresetGenNdx[2] + wPresetModNdx[2])
      pmod entry : 10 bytes  (terminal only)
      pgen entry :  4 bytes  (sfGenOper[2] + genAmount[2])
      inst entry : 22 bytes  (name[20] + wInstBagNdx[2])
      ibag entry :  4 bytes  (wInstGenNdx[2] + wInstModNdx[2])
      imod entry : 10 bytes  (sfModSrcOper[2] + sfGenDest[2] + modAmount[2] + sfModAmtSrcOper[2] + sfModTransOper[2])
      igen entry :  4 bytes  (sfGenOper[2] + genAmount[2])
      shdr entry : 46 bytes  (name[20] + start[4] + end[4] + loopStart[4] + loopEnd[4] + rate[4] + pitch[1] + correction[1] + link[2] + type[2])
    """
    result = get_preset_samples(sf, bank, program)
    if not result:
        return None
    sample_ids, inst_ids = result

    shdr = sf["shdr"]
    smpl = sf["smpl"]
    igen = sf["igen"]
    ibag = sf["ibag"]
    inst = sf["inst"]
    imod_raw = sf["imod_raw"]  # raw bytes, 10 bytes per entry
    pgen = sf["pgen"]

    # ── Samples ──────────────────────────────────────────────────────────────
    all_sample_ids = set(sample_ids)
    for sid in sample_ids:
        if sid < len(shdr) and shdr[sid]["link"] not in (0, sid):
            all_sample_ids.add(shdr[sid]["link"])
    all_sample_ids = sorted(all_sample_ids)

    new_smpl = bytearray()
    new_shdr = bytearray()
    old_to_new_sample = {}

    for new_id, old_id in enumerate(all_sample_ids):
        s = shdr[old_id]
        old_to_new_sample[old_id] = new_id
        new_start = len(new_smpl) // 2
        new_smpl.extend(smpl[s["start"] * 2 : s["end"] * 2])
        if len(new_smpl) % 2:
            new_smpl.append(0)
        new_end = len(new_smpl) // 2

        entry = bytearray(46)
        entry[:20] = make_sf2_name(s["name"], 20)
        struct.pack_into(
            "<IIIII",
            entry,
            20,
            new_start,
            new_end,
            new_start + (s["loop_start"] - s["start"]),
            new_start + (s["loop_end"] - s["start"]),
            s["rate"],
        )
        struct.pack_into("<Bb", entry, 40, s["pitch"], s["correction"])
        struct.pack_into(
            "<HH", entry, 42, old_to_new_sample.get(s["link"], 0), s["type"]
        )
        new_shdr.extend(entry)

    # EOS terminal
    eos = bytearray(46)
    eos[:4] = b"EOS\x00"
    new_shdr.extend(eos)

    # ── Instruments (inst / ibag / imod / igen) ───────────────────────────────
    inst_ids_sorted = sorted(inst_ids)
    old_to_new_inst = {old: new for new, old in enumerate(inst_ids_sorted)}

    new_inst = bytearray()
    new_ibag = bytearray()
    new_imod = bytearray()
    new_igen = bytearray()

    ibag_ndx = 0  # index into new_ibag (per zone)
    imod_ndx = 0  # index into new_imod (per zone)
    igen_ndx = 0  # index into new_igen (per zone)

    imod_entries = len(imod_raw) // 10  # total modulator entries in original

    for old_iid in inst_ids_sorted:
        inst_entry = inst[old_iid]
        i_start = inst_entry["bag_ndx"]
        i_end = inst[old_iid + 1]["bag_ndx"] if old_iid + 1 < len(inst) else len(ibag)

        # inst entry: name[20] + wInstBagNdx[2] = 22 bytes
        ie = bytearray(22)
        ie[:20] = make_sf2_name(inst_entry["name"], 20)
        struct.pack_into("<H", ie, 20, ibag_ndx)
        new_inst.extend(ie)

        for bi in range(i_start, i_end):
            m_start = ibag[bi]["mod_ndx"]
            m_end = ibag[bi + 1]["mod_ndx"] if bi + 1 < len(ibag) else imod_entries
            g_start = ibag[bi]["gen_ndx"]
            g_end = ibag[bi + 1]["gen_ndx"] if bi + 1 < len(ibag) else len(igen)

            # ibag entry: wInstGenNdx[2] + wInstModNdx[2] = 4 bytes
            struct.pack_into = struct.pack_into  # no-op, just clarity
            new_ibag.extend(struct.pack("<HH", igen_ndx, imod_ndx))
            ibag_ndx += 1

            # imod entries for this zone (preserve original modulators)
            for mi in range(m_start, m_end):
                new_imod.extend(imod_raw[mi * 10 : (mi + 1) * 10])
                imod_ndx += 1

            # igen entries for this zone
            for gi in range(g_start, g_end):
                gen = igen[gi]
                if gen["oper"] == 53:  # sampleID — remap
                    new_igen.extend(
                        struct.pack("<HH", 53, old_to_new_sample.get(gen["amount"], 0))
                    )
                else:
                    new_igen.extend(gen["raw"])
                igen_ndx += 1

    # Terminal ibag: points past last igen and imod
    new_ibag.extend(struct.pack("<HH", igen_ndx, imod_ndx))
    # Terminal imod (10 zero bytes)
    new_imod.extend(b"\x00" * 10)
    # Terminal inst (EOI): name[20] + wInstBagNdx[2] = 22 bytes
    eoi = bytearray(22)
    eoi[:4] = b"EOI\x00"
    struct.pack_into("<H", eoi, 20, ibag_ndx)
    new_inst.extend(eoi)

    # ── Presets (phdr / pbag / pmod / pgen) ──────────────────────────────────
    p_idx = next(
        i
        for i, x in enumerate(sf["phdr"])
        if x["bank"] == bank and x["preset"] == program
    )
    p = sf["phdr"][p_idx]
    p_next = sf["phdr"][p_idx + 1]

    new_phdr = bytearray()
    new_pbag = bytearray()
    new_pgen = bytearray()

    pbag_ndx = 0
    pgen_ndx = 0

    # phdr entry: name[20] + wPreset[2] + wBank[2] + wPresetBagNdx[2] + lib[4] + genre[4] + morph[4] = 38 bytes
    pe = bytearray(38)
    pe[:20] = make_sf2_name(p["name"], 20)
    struct.pack_into("<HHH", pe, 20, p["preset"], p["bank"], pbag_ndx)
    # bytes 26-37: library/genre/morphology remain 0
    new_phdr.extend(pe)

    for bi in range(p["bag_ndx"], p_next["bag_ndx"]):
        g_start = sf["pbag"][bi]["gen_ndx"]
        g_end = sf["pbag"][bi + 1]["gen_ndx"] if bi + 1 < len(sf["pbag"]) else len(pgen)

        # pbag entry: wPresetGenNdx[2] + wPresetModNdx[2] = 4 bytes
        new_pbag.extend(struct.pack("<HH", pgen_ndx, 0))
        pbag_ndx += 1

        for gi in range(g_start, g_end):
            gen = pgen[gi]
            if gen["oper"] == 41:  # instrument — remap
                new_pgen.extend(
                    struct.pack("<HH", 41, old_to_new_inst.get(gen["amount"], 0))
                )
            else:
                new_pgen.extend(struct.pack("<HH", gen["oper"], gen["amount"]))
            pgen_ndx += 1

    # Terminal pbag
    new_pbag.extend(struct.pack("<HH", pgen_ndx, 0))
    # Terminal phdr (EOP): 38 bytes
    eop = bytearray(38)
    eop[:4] = b"EOP\x00"
    struct.pack_into("<HHH", eop, 20, 255, 255, pbag_ndx)
    new_phdr.extend(eop)

    # ── Assemble RIFF ─────────────────────────────────────────────────────────
    def make_chunk(tag, data):
        tag = tag.encode("ascii") if isinstance(tag, str) else tag
        size = len(data)
        return tag + struct.pack("<I", size) + data + (b"\x00" if size % 2 else b"")

    def make_list(list_type, chunks):
        inner = list_type.encode("ascii") + b"".join(chunks)
        return b"LIST" + struct.pack("<I", len(inner)) + inner

    info_list = make_list("INFO", [make_chunk(k, v) for k, v in sf["info"].items()])
    sdta_list = make_list("sdta", [make_chunk("smpl", bytes(new_smpl))])
    pdta_list = make_list(
        "pdta",
        [
            make_chunk("phdr", bytes(new_phdr)),
            make_chunk("pbag", bytes(new_pbag)),
            make_chunk("pmod", b"\x00" * 10),  # no preset modulators
            make_chunk("pgen", bytes(new_pgen)),
            make_chunk("inst", bytes(new_inst)),
            make_chunk("ibag", bytes(new_ibag)),
            make_chunk("imod", bytes(new_imod)),
            make_chunk("igen", bytes(new_igen)),
            make_chunk("shdr", bytes(new_shdr)),
        ],
    )

    body = info_list + sdta_list + pdta_list
    return b"RIFF" + struct.pack("<I", len(body) + 4) + b"sfbk" + body


GM_NAMES = {
    0: "000_acoustic_grand_piano",
    1: "001_bright_acoustic_piano",
    2: "002_electric_grand_piano",
    3: "003_honkytonk_piano",
    4: "004_electric_piano_1",
    5: "005_electric_piano_2",
    6: "006_harpsichord",
    7: "007_clavinet",
    8: "008_celesta",
    9: "009_glockenspiel",
    10: "010_music_box",
    11: "011_vibraphone",
    12: "012_marimba",
    13: "013_xylophone",
    14: "014_tubular_bells",
    15: "015_dulcimer",
    16: "016_drawbar_organ",
    17: "017_percussive_organ",
    18: "018_rock_organ",
    19: "019_church_organ",
    20: "020_reed_organ",
    21: "021_accordion",
    22: "022_harmonica",
    23: "023_tango_accordion",
    24: "024_acoustic_guitar_nylon",
    25: "025_acoustic_guitar_steel",
    26: "026_electric_guitar_jazz",
    27: "027_electric_guitar_clean",
    28: "028_electric_guitar_muted",
    29: "029_overdriven_guitar",
    30: "030_distortion_guitar",
    31: "031_guitar_harmonics",
    32: "032_acoustic_bass",
    33: "033_electric_bass_finger",
    34: "034_electric_bass_pick",
    35: "035_fretless_bass",
    36: "036_slap_bass_1",
    37: "037_slap_bass_2",
    38: "038_synth_bass_1",
    39: "039_synth_bass_2",
    40: "040_violin",
    41: "041_viola",
    42: "042_cello",
    43: "043_contrabass",
    44: "044_tremolo_strings",
    45: "045_pizzicato_strings",
    46: "046_orchestral_harp",
    47: "047_timpani",
    48: "048_string_ensemble_1",
    49: "049_string_ensemble_2",
    50: "050_synth_strings_1",
    51: "051_synth_strings_2",
    52: "052_choir_aahs",
    53: "053_voice_oohs",
    54: "054_synth_voice",
    55: "055_orchestra_hit",
    56: "056_trumpet",
    57: "057_trombone",
    58: "058_tuba",
    59: "059_muted_trumpet",
    60: "060_french_horn",
    61: "061_brass_section",
    62: "062_synth_brass_1",
    63: "063_synth_brass_2",
    64: "064_soprano_sax",
    65: "065_alto_sax",
    66: "066_tenor_sax",
    67: "067_baritone_sax",
    68: "068_oboe",
    69: "069_english_horn",
    70: "070_bassoon",
    71: "071_clarinet",
    72: "072_piccolo",
    73: "073_flute",
    74: "074_recorder",
    75: "075_pan_flute",
    76: "076_blown_bottle",
    77: "077_shakuhachi",
    78: "078_whistle",
    79: "079_ocarina",
    80: "080_lead_1_square",
    81: "081_lead_2_sawtooth",
    82: "082_lead_3_calliope",
    83: "083_lead_4_chiff",
    84: "084_lead_5_charang",
    85: "085_lead_6_voice",
    86: "086_lead_7_fifths",
    87: "087_lead_8_bass_lead",
    88: "088_pad_1_new_age",
    89: "089_pad_2_warm",
    90: "090_pad_3_polysynth",
    91: "091_pad_4_choir",
    92: "092_pad_5_bowed",
    93: "093_pad_6_metallic",
    94: "094_pad_7_halo",
    95: "095_pad_8_sweep",
    96: "096_fx_1_rain",
    97: "097_fx_2_soundtrack",
    98: "098_fx_3_crystal",
    99: "099_fx_4_atmosphere",
    100: "100_fx_5_brightness",
    101: "101_fx_6_goblins",
    102: "102_fx_7_echoes",
    103: "103_fx_8_sci_fi",
    104: "104_sitar",
    105: "105_banjo",
    106: "106_shamisen",
    107: "107_koto",
    108: "108_kalimba",
    109: "109_bagpipe",
    110: "110_fiddle",
    111: "111_shanai",
    112: "112_tinkle_bell",
    113: "113_agogo",
    114: "114_steel_drums",
    115: "115_woodblock",
    116: "116_taiko_drum",
    117: "117_melodic_tom",
    118: "118_synth_drum",
    119: "119_reverse_cymbal",
    120: "120_guitar_fret_noise",
    121: "121_breath_noise",
    122: "122_seashore",
    123: "123_bird_tweet",
    124: "124_telephone_ring",
    125: "125_helicopter",
    126: "126_applause",
    127: "127_gunshot",
    128: "128_standard_drums",
}


def main():
    input_path = sys.argv[1] if len(sys.argv) > 1 else "soundfont/FluidR3 GM.sf2"
    output_dir = sys.argv[2] if len(sys.argv) > 2 else "web/soundfonts/instruments"

    if not os.path.exists(input_path):
        print(f"Error: {input_path} not found")
        sys.exit(1)

    os.makedirs(output_dir, exist_ok=True)

    print(f"Loading {input_path} ({os.path.getsize(input_path) // 1024 // 1024} MB)...")
    with open(input_path, "rb") as f:
        data = f.read()

    print("Parsing SF2 structure...")
    sf = parse_sf2(data)
    print(f"Found {len(sf['phdr']) - 1} presets, {len(sf['shdr']) - 1} samples")

    index = {"instruments": {}, "fallback": "000_acoustic_grand_piano.sf2"}
    errors = []

    # Split melodic instruments (bank 0)
    for program in range(128):
        name = GM_NAMES.get(program, f"{program:03d}_unknown")
        filename = f"{name}.sf2"
        out_path = os.path.join(output_dir, filename)

        try:
            sf2_bytes = build_sf2(sf, bank=0, program=program)
            if sf2_bytes:
                with open(out_path, "wb") as f:
                    f.write(sf2_bytes)
                size = len(sf2_bytes)
                index["instruments"][str(program)] = {
                    "file": filename,
                    "name": next(
                        (
                            p["name"]
                            for p in sf["phdr"]
                            if p["bank"] == 0 and p["preset"] == program
                        ),
                        name,
                    ),
                    "size": size,
                }
                print(f"  [{program:3d}] {filename} ({size // 1024} KB)")
            else:
                print(f"  [{program:3d}] SKIP: no samples found")
                index["instruments"][str(program)] = {
                    "file": "000_acoustic_grand_piano.sf2",
                    "name": name,
                }
                errors.append(f"No samples for program {program}")
        except Exception as e:
            print(f"  [{program:3d}] ERROR: {e}")
            index["instruments"][str(program)] = {
                "file": "000_acoustic_grand_piano.sf2",
                "name": name,
            }
            errors.append(f"Program {program}: {e}")

    # Split drum kit (bank 128)
    drum_filename = "128_standard_drums.sf2"
    drum_path = os.path.join(output_dir, drum_filename)
    try:
        sf2_bytes = build_sf2(sf, bank=128, program=0)
        if not sf2_bytes:
            sf2_bytes = build_sf2(sf, bank=128, program=128)
        if sf2_bytes:
            with open(drum_path, "wb") as f:
                f.write(sf2_bytes)
            index["instruments"]["128"] = {
                "file": drum_filename,
                "name": "Standard Drums",
                "size": len(sf2_bytes),
            }
            print(f"  [128] {drum_filename} ({len(sf2_bytes) // 1024} KB)")
        else:
            print(f"  [128] SKIP: no drum samples found, using fallback")
    except Exception as e:
        print(f"  [128] ERROR: {e}")
        errors.append(f"Drums: {e}")

    index["fallback"] = "000_acoustic_grand_piano.sf2"

    index_path = os.path.join(os.path.dirname(output_dir), "index.json")
    with open(index_path, "w") as f:
        json.dump(index, f, indent=2)
    print(f"\nWrote index to {index_path}")

    if errors:
        print(f"\nWarnings ({len(errors)}):")
        for e in errors[:10]:
            print(f"  - {e}")

    print(f"\nDone! {len(index['instruments'])} instruments in {output_dir}/")


if __name__ == "__main__":
    main()
