/**
 * Merge multiple SF2 ArrayBuffers into a single SF2 ArrayBuffer.
 *
 * SpessaSynth triggers a full synth reset after each addSoundBank() call.
 * Loading N banks causes N resets, which can interfere with channel state.
 * Merging into one SF2 means one addSoundBank() call → one reset → stable.
 *
 * SF2 structure:
 *   RIFF sfbk
 *     LIST INFO  (use first file's; ignore rest)
 *     LIST sdta
 *       smpl  (concatenate all)
 *     LIST pdta
 *       phdr(38B) pbag(4B) pmod(10B) pgen(4B)
 *       inst(22B) ibag(4B) imod(10B) igen(4B)
 *       shdr(46B)
 */
function mergeSF2Buffers(buffers) {
    if (buffers.length === 0) throw new Error('No SF2 buffers to merge');
    if (buffers.length === 1) return buffers[0];

    // Parse one SF2 → { info, smpl, phdr, pbag, pmod, pgen, inst, ibag, imod, igen, shdr }
    function parseSF2(buf) {
        const dv = new DataView(buf);
        let pos = 0;
        function tag() { return String.fromCharCode(dv.getUint8(pos), dv.getUint8(pos+1), dv.getUint8(pos+2), dv.getUint8(pos+3)); }
        function u32() { const v = dv.getUint32(pos, true); pos += 4; return v; }
        function readTag() { const t = tag(); pos += 4; return t; }
        function readSize() { return u32(); }

        if (readTag() !== 'RIFF') throw new Error('Not a RIFF file');
        const riffSize = readSize();
        if (readTag() !== 'sfbk') throw new Error('Not an SF2 file');

        const end = Math.min(12 + riffSize, buf.byteLength);
        const chunks = {};

        while (pos + 8 <= end) {
            const chunkId = readTag();
            const chunkSize = readSize();
            const chunkStart = pos;
            if (chunkId === 'LIST') {
                const listType = readTag();
                const listEnd = chunkStart + chunkSize;
                if (listType === 'sdta') {
                    while (pos + 8 <= listEnd) {
                        const sub = readTag();
                        const sz = readSize();
                        if (sub === 'smpl') chunks.smpl = new Uint8Array(buf, pos, sz);
                        pos += sz + (sz & 1);
                    }
                } else if (listType === 'pdta') {
                    while (pos + 8 <= listEnd) {
                        const sub = readTag();
                        const sz = readSize();
                        chunks[sub] = new Uint8Array(buf, pos, sz);
                        pos += sz + (sz & 1);
                    }
                } else {
                    if (!chunks.info) chunks.info = new Uint8Array(buf, chunkStart - 4, chunkSize + 4);
                    pos = chunkStart + chunkSize;
                }
            } else {
                pos = chunkStart + chunkSize + (chunkSize & 1);
            }
        }
        return chunks;
    }

    const parsed = buffers.map(b => parseSF2(b));

    // Read LE uint16 / uint32 at byte offset within a Uint8Array
    const r16 = (a, o) => a[o] | (a[o+1] << 8);
    const r32 = (a, o) => (a[o] | (a[o+1] << 8) | (a[o+2] << 16) | (a[o+3] << 24)) >>> 0;
    const w16 = (a, o, v) => { a[o] = v & 0xFF; a[o+1] = (v >> 8) & 0xFF; };
    const w32 = (a, o, v) => { a[o] = v & 0xFF; a[o+1] = (v>>8)&0xFF; a[o+2] = (v>>16)&0xFF; a[o+3] = (v>>24)&0xFF; };

    // Merge smpl (raw PCM bytes — each sample is 2 bytes)
    let totalSmpl = 0;
    for (const p of parsed) totalSmpl += (p.smpl ? p.smpl.byteLength : 0);
    const mergedSmpl = new Uint8Array(totalSmpl);
    let smplOffsets = []; // byte offset of each file's smpl data in merged smpl
    let smplPos = 0;
    for (const p of parsed) {
        smplOffsets.push(smplPos);
        if (p.smpl) { mergedSmpl.set(p.smpl, smplPos); smplPos += p.smpl.byteLength; }
    }

    // Count records in each section per file (excluding terminals)
    const PHDR = 38, PBAG = 4, PMOD = 10, PGEN = 4;
    const INST = 22, IBAG = 4, IMOD = 10, IGEN = 4;
    const SHDR = 46;

    // Sections with terminal records (phdr, pbag, inst, ibag, shdr): subtract 1
    // Sections without terminal records (pgen, pmod, igen, imod): use full count
    function countWithTerminal(arr, stride) {
        return arr ? Math.max(0, Math.floor(arr.byteLength / stride) - 1) : 0;
    }
    function countNoTerminal(arr, stride) {
        return arr ? Math.floor(arr.byteLength / stride) : 0;
    }

    const counts = parsed.map(p => ({
        phdr: countWithTerminal(p.phdr, PHDR),
        pbag: countWithTerminal(p.pbag, PBAG),
        pmod: countNoTerminal(p.pmod, PMOD),
        pgen: countNoTerminal(p.pgen, PGEN),
        inst: countWithTerminal(p.inst, INST),
        ibag: countWithTerminal(p.ibag, IBAG),
        imod: countNoTerminal(p.imod, IMOD),
        igen: countNoTerminal(p.igen, IGEN),
        shdr: countWithTerminal(p.shdr, SHDR),
    }));

    // Cumulative offsets (prefix sums)
    const cum = { phdr:0, pbag:0, pmod:0, pgen:0, inst:0, ibag:0, imod:0, igen:0, shdr:0 };
    const cumOffsets = parsed.map((_, fi) => {
        const snap = {...cum};
        const c = counts[fi];
        cum.phdr += c.phdr; cum.pbag += c.pbag; cum.pmod += c.pmod; cum.pgen += c.pgen;
        cum.inst += c.inst; cum.ibag += c.ibag; cum.imod += c.imod; cum.igen += c.igen;
        cum.shdr += c.shdr;
        return snap;
    });
    const totals = cum;

    // Build merged pdta sections
    function mergeSection(name, stride, patchFn) {
        const total = totals[name];
        const out = new Uint8Array((total + 1) * stride); // +1 for terminal
        let outPos = 0;
        for (let fi = 0; fi < parsed.length; fi++) {
            const src = parsed[fi][name];
            if (!src) continue;
            const n = counts[fi][name];
            const co = cumOffsets[fi];
            for (let i = 0; i < n; i++) {
                const srcOff = i * stride;
                const dstOff = outPos;
                out.set(src.subarray(srcOff, srcOff + stride), dstOff);
                patchFn(out, dstOff, co, fi, i);
                outPos += stride;
            }
        }
        return out; // terminal written by caller
    }

    const mergedPhdr = mergeSection('phdr', PHDR, (a, off, co) => {
        w16(a, off + 24, r16(a, off + 24) + co.pbag); // wPresetBagNdx
    });
    // Terminal EOP phdr
    const termPhdrOff = totals.phdr * PHDR;
    mergedPhdr[termPhdrOff]=69; mergedPhdr[termPhdrOff+1]=79; mergedPhdr[termPhdrOff+2]=80; // "EOP"
    w16(mergedPhdr, termPhdrOff + 20, 0xFF); // wPreset = 255
    w16(mergedPhdr, termPhdrOff + 22, 0xFF); // wBank = 255
    w16(mergedPhdr, termPhdrOff + 24, totals.pbag); // wPresetBagNdx = index of terminal pbag

    const mergedPbag = mergeSection('pbag', PBAG, (a, off, co) => {
        w16(a, off + 0, r16(a, off + 0) + co.pgen); // wGenNdx
        w16(a, off + 2, r16(a, off + 2) + co.pmod); // wModNdx
    });
    // Terminal pbag: points past the last valid pgen/pmod
    const termPbagOff = totals.pbag * PBAG;
    w16(mergedPbag, termPbagOff + 0, totals.pgen); // wGenNdx = total pgen count
    w16(mergedPbag, termPbagOff + 2, totals.pmod); // wModNdx = total pmod count

    // pmod and pgen: no terminal record needed (terminal pbag points past end)
    const mergedPmod = mergeSection('pmod', PMOD, () => {});
    const mergedPgen = mergeSection('pgen', PGEN, (a, off, co) => {
        if (r16(a, off) === 41) w16(a, off + 2, r16(a, off + 2) + co.inst); // instrument ref
    });

    const mergedInst = mergeSection('inst', INST, (a, off, co) => {
        w16(a, off + 20, r16(a, off + 20) + co.ibag); // wInstBagNdx
    });
    // Terminal EOI inst
    const termInstOff = totals.inst * INST;
    mergedInst[termInstOff]=69; mergedInst[termInstOff+1]=79; mergedInst[termInstOff+2]=73; // "EOI"
    w16(mergedInst, termInstOff + 20, totals.ibag); // wInstBagNdx = index of terminal ibag

    const mergedIbag = mergeSection('ibag', IBAG, (a, off, co) => {
        w16(a, off + 0, r16(a, off + 0) + co.igen); // wInstGenNdx
        w16(a, off + 2, r16(a, off + 2) + co.imod); // wInstModNdx
    });
    // Terminal ibag: points past the last valid igen/imod
    const termIbagOff = totals.ibag * IBAG;
    w16(mergedIbag, termIbagOff + 0, totals.igen); // wInstGenNdx
    w16(mergedIbag, termIbagOff + 2, totals.imod); // wInstModNdx

    // imod and igen: no terminal record needed
    const mergedImod = mergeSection('imod', IMOD, () => {});
    const mergedIgen = mergeSection('igen', IGEN, (a, off, co, fi) => {
        if (r16(a, off) === 53) w16(a, off + 2, r16(a, off + 2) + co.shdr); // sampleID ref
    });

    const mergedShdr = mergeSection('shdr', SHDR, (a, off, co, fi) => {
        // dwStart(4), dwEnd(4), dwStartloop(4), dwEndloop(4) at byte offsets 20-35
        // SF2 shdr offsets are in SAMPLE UNITS (1 sample = 2 bytes in smpl)
        const sampleOffset = smplOffsets[fi] >>> 1;
        for (const fieldOff of [20, 24, 28, 32])
            w32(a, off + fieldOff, r32(a, off + fieldOff) + sampleOffset);
    });
    // Terminal EOS shdr
    const termShdrOff = totals.shdr * SHDR;
    mergedShdr[termShdrOff]=69; mergedShdr[termShdrOff+1]=79; mergedShdr[termShdrOff+2]=83; // "EOS"

    // RIFF chunk builder helpers
    function makeChunk(id, data) {
        const size = data.byteLength;
        const out = new Uint8Array(8 + size + (size & 1));
        for (let i = 0; i < 4; i++) out[i] = id.charCodeAt(i);
        w32(out, 4, size);
        out.set(data, 8);
        return out;
    }
    function makeList(type, ...chunks) {
        const bodyLen = 4 + chunks.reduce((s, c) => s + c.byteLength, 0);
        const body = new Uint8Array(bodyLen);
        for (let i = 0; i < 4; i++) body[i] = type.charCodeAt(i);
        let p = 4;
        for (const c of chunks) { body.set(c, p); p += c.byteLength; }
        return makeChunk('LIST', body);
    }

    // Slice out just the valid records (no spurious terminal for pgen/pmod/igen/imod)
    const phdrData = mergedPhdr.subarray(0, (totals.phdr + 1) * PHDR);
    const pbagData = mergedPbag.subarray(0, (totals.pbag + 1) * PBAG);
    const pmodData = mergedPmod.subarray(0, totals.pmod * PMOD);
    const pgenData = mergedPgen.subarray(0, totals.pgen * PGEN);
    const instData = mergedInst.subarray(0, (totals.inst + 1) * INST);
    const ibagData = mergedIbag.subarray(0, (totals.ibag + 1) * IBAG);
    const imodData = mergedImod.subarray(0, totals.imod * IMOD);
    const igenData = mergedIgen.subarray(0, totals.igen * IGEN);
    const shdrData = mergedShdr.subarray(0, (totals.shdr + 1) * SHDR);

    const pdtaList = makeList('pdta',
        makeChunk('phdr', phdrData), makeChunk('pbag', pbagData),
        makeChunk('pmod', pmodData), makeChunk('pgen', pgenData),
        makeChunk('inst', instData), makeChunk('ibag', ibagData),
        makeChunk('imod', imodData), makeChunk('igen', igenData),
        makeChunk('shdr', shdrData),
    );
    const sdtaList = makeList('sdta', makeChunk('smpl', mergedSmpl));

    // Minimal valid INFO LIST (required by SF2 spec)
    const infoData = new Uint8Array([
        0x69,0x66,0x69,0x6C, 4,0,0,0, 2,0,1,0,           // ifil: version 2.1
        0x69,0x73,0x6E,0x67, 8,0,0,0, 69,77,85,56,48,48,0,0, // isng: EMU8000
        0x49,0x4E,0x41,0x4D, 8,0,0,0, 77,101,114,103,101,100,0,0, // INAM: Merged
    ]);
    const infoList = makeList('INFO', infoData);

    // Assemble RIFF sfbk
    const sfbkBody = new Uint8Array(4 + infoList.byteLength + sdtaList.byteLength + pdtaList.byteLength);
    sfbkBody[0]=0x73; sfbkBody[1]=0x66; sfbkBody[2]=0x62; sfbkBody[3]=0x6B; // 'sfbk'
    let bp = 4;
    sfbkBody.set(infoList, bp); bp += infoList.byteLength;
    sfbkBody.set(sdtaList, bp); bp += sdtaList.byteLength;
    sfbkBody.set(pdtaList, bp);

    const riff = new Uint8Array(8 + sfbkBody.byteLength);
    riff[0]=0x52; riff[1]=0x49; riff[2]=0x46; riff[3]=0x46; // 'RIFF'
    w32(riff, 4, sfbkBody.byteLength);
    riff.set(sfbkBody, 8);

    return riff.buffer;
}

/**
 * MidiPlayerController — wraps SpessaSynth v4 (WorkletSynthesizer + Sequencer).
 *
 * Usage:
 *   const player = new MidiPlayerController();
 *   await player.init();
 *   await player.loadMidi(midiUint8Array, instruments);
 *   player.play();
 */
class MidiPlayerController {
    constructor() {
        this._ctx = null;
        this._synth = null;
        this._gainNode = null;
        this._seq = null;
        this._sfLoader = new SoundFontLoader();
        this._volume = 1.0;
        this._muted = false;
        this._loadedSoundFonts = new Set(); // Set<ArrayBuffer> — tracks already-added SF2 buffers

        /** Callbacks set by player.js */
        this.onEnded = null;
        this.onTimeUpdate = null;
    }

    async init() {
        await this._sfLoader.init();
    }

    /**
     * Call this SYNCHRONOUSLY inside a direct user-gesture handler (click, change, touchend).
     *
     * iOS Safari requires AudioContext creation and resume() to occur within the
     * synchronous call stack of a user gesture. FileReader.onload and other async
     * callbacks do not qualify. Calling unlockAudio() before any await in the
     * handler ensures the context is created while the gesture is still "active".
     */
    unlockAudio() {
        const Ctx = window.AudioContext || window.webkitAudioContext;
        if (!Ctx) return;

        if (!this._ctx) {
            this._ctx = new Ctx();
            this._gainNode = this._ctx.createGain();
            this._gainNode.gain.value = this._muted ? 0 : this._volume;
            this._gainNode.connect(this._ctx.destination);
        }

        // resume() must also be called within the gesture for iOS to grant permission.
        // It is async but we intentionally do NOT await — we just need to fire it
        // while the gesture flag is still set in the browser.
        if (this._ctx.state === 'suspended') {
            this._ctx.resume().catch(() => {});
        }
    }

    /**
     * Ensures AudioContext is running and WorkletSynthesizer is initialized.
     * unlockAudio() should be called first (synchronously in the gesture handler)
     * so the AudioContext already exists by the time this async method runs.
     */
    async _ensureAudio() {
        // Fallback: if unlockAudio() was never called, create the context now.
        // This won't work on iOS if we're inside an async callback, but at least
        // prevents crashes on desktop/Android where the restriction doesn't apply.
        if (!this._ctx) this.unlockAudio();

        if (this._ctx.state === 'suspended') await this._ctx.resume();

        // WorkletSynthesizer is initialized once; AudioContext is reused.
        if (this._synth) return;

        await this._ctx.audioWorklet.addModule('./lib/spessasynth_processor.min.js');
        const { WorkletSynthesizer } = await import('./lib/spessasynth_lib.min.js');
        // eventsEnabled: true (already the library default — set explicitly so
        // playback isn't silently left without the periodic clock resync below
        // if that default ever changes) makes the audio thread post a "sync"
        // message roughly once a second, letting the Sequencer hard-correct its
        // visualization clock instead of relying solely on its slow ~1%/frame
        // self-correction.
        this._synth = new WorkletSynthesizer(this._ctx, { eventsEnabled: true });
        this._synth.connect(this._gainNode);
    }

    /**
     * Load SF2 soundfonts for the given instruments list.
     * All required SF2 buffers are merged into a single SF2 before calling
     * addSoundBank() once — this avoids the per-bank synth reset that occurs
     * with multiple addSoundBank() calls and breaks multi-instrument playback.
     */
    async _ensureSoundFonts(instruments) {
        const programs = (instruments || []).map(i => i.program);
        if (programs.length === 0) programs.push(0);
        // Pizzicato strings (GM 45) can appear as a mid-track articulation.
        if (!programs.includes(45)) programs.push(45);

        const sfData = await this._sfLoader.loadForPrograms(programs);

        // Collect unique ArrayBuffers (by reference) not yet loaded
        const newBuffers = [];
        for (const { data } of sfData) {
            if (!this._loadedSoundFonts.has(data) && !newBuffers.includes(data)) {
                newBuffers.push(data);
            }
        }

        if (newBuffers.length === 0) return;

        // Mark all as loaded before transfer (transfer detaches them)
        for (const buf of newBuffers) this._loadedSoundFonts.add(buf);

        // Merge into a single SF2 and load with one addSoundBank() call.
        // One call → one preset-list rebuild → no repeated channel resets.
        const merged = mergeSF2Buffers(newBuffers);
        const id = `sf2-merged-${this._loadedSoundFonts.size}`;
        await this._synth.soundBankManager.addSoundBank(merged, id);
        await this._synth.isReady;
    }

    /**
     * Load new MIDI data and prepare for playback.
     * @param {Uint8Array} midiBytes  — raw MIDI file bytes from WASM.
     * @param {Array} instruments     — [{program, name}] from WASM list_instruments().
     */
    async loadMidi(midiBytes, instruments) {
        await this._ensureAudio();
        await this._ensureSoundFonts(instruments);

        const midiBuffer = midiBytes instanceof Uint8Array
            ? midiBytes.buffer.slice(midiBytes.byteOffset, midiBytes.byteOffset + midiBytes.byteLength)
            : midiBytes;

        // Create or reuse Sequencer
        const { Sequencer } = await import('./lib/spessasynth_lib.min.js');
        if (!this._seq) {
            this._seq = new Sequencer(this._synth);
            this._seq.eventHandler.addEvent('songEnded', 'staveloom-player', () => {
                if (this.onEnded) this.onEnded();
            });
        }
        // loadNewSongList sends a postMessage to the AudioWorklet and returns immediately.
        // midiData is set only after the Worklet responds (async). The Sequencer fires
        // 'songChange' once midiData is populated and absoluteStartTime is reset to 0.
        // We must await that event so callers can read duration and currentTime reliably.
        const midiReady = new Promise(resolve => {
            this._seq.eventHandler.addEvent('songChange', 'staveloom-player-load', resolve);
        });
        this._seq.loadNewSongList([{ binary: midiBuffer }]);
        await midiReady;
        // One-shot: replace the resolver with a no-op so it doesn't re-fire on future events
        this._seq.eventHandler.addEvent('songChange', 'staveloom-player-load', () => {});
    }

    async play() {
        if (!this._seq) return;
        // iOS re-suspends AudioContext on tab switch or inactivity.
        // resume() is async — seq.play() on a still-suspended context produces silence.
        // We MUST await the resume before starting playback.
        if (this._ctx && this._ctx.state !== 'running') {
            try { await this._ctx.resume(); } catch (_) {}
        }
        this._seq.play();
        this._resyncClockSoon();
    }

    // The Sequencer's play() anchors its visualization clock
    // (currentHighResolutionTime) using synth.currentTime read at that exact
    // instant. That value is fed asynchronously from the audio-rendering
    // thread, so right after a suspend -> resume transition it can still be
    // stale, making the on-screen cursor appear frozen for a second or so
    // until the library's own slow (~1%/frame) self-correction catches up.
    // Re-anchor directly a couple of frames later, once the audio thread has
    // had a chance to report a fresh currentTime. This mirrors the library's
    // own recalculateStartTime(0) but writes the (plain, non-private) fields
    // directly instead of going through the public currentTime setter, which
    // performs a real seek (clears in-flight notes) — not what we want here.
    _resyncClockSoon() {
        const seq = this._seq;
        requestAnimationFrame(() => requestAnimationFrame(() => {
            if (!seq || seq !== this._seq) return;
            if (typeof seq.absoluteStartTime !== 'number' || !seq.synth) return;
            const synthTime = seq.synth.currentTime;
            if (typeof synthTime !== 'number') return;
            const rate = seq._playbackRate ?? 1;
            seq.absoluteStartTime = synthTime;
            seq.highResTimeOffset = (synthTime - performance.now() / 1000) * rate;
        }));
    }

    pause() {
        if (!this._seq) return;
        this._seq.pause();
    }

    get currentTime() {
        if (!this._seq) return 0;
        // currentHighResolutionTime: performance.now()-based, linear between AudioContext quanta.
        // Designed by SpessaSynth for visualization. currentTime steps in ~2.9ms quanta
        // (128 samples @ 44100Hz), causing cursor to freeze for multiple rAF frames then snap.
        return this._seq.currentHighResolutionTime ?? this._seq.currentTime ?? 0;
    }

    set currentTime(t) {
        if (!this._seq) return;
        this._seq.currentTime = t;
    }

    /** Hardware output latency in seconds (AudioWorklet synthesis-to-speaker delay). */
    get audioLatency() {
        if (!this._ctx) return 0;
        return (this._ctx.outputLatency ?? 0) + (this._ctx.baseLatency ?? 0);
    }

    get duration() {
        if (!this._seq || !this._seq.midiData) return 0;
        return this._seq.duration ?? 0;
    }

    get isPlaying() {
        if (!this._seq) return false;
        return !this._seq.paused;
    }

    setVolume(vol) {
        this._volume = Math.max(0, Math.min(1, vol));
        if (this._gainNode && !this._muted) {
            this._gainNode.gain.value = this._volume;
        }
    }

    setMuted(muted) {
        this._muted = muted;
        if (this._gainNode) {
            this._gainNode.gain.value = muted ? 0 : this._volume;
        }
    }

    /** Check if audio backend is ready (user gesture occurred). */
    get isReady() {
        return this._ctx !== null;
    }
}
