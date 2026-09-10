// staveloom Web Player — Phase 3: Virtual SVG Rendering

(async function () {
  // ── WASM Module ──────────────────────────────────────────────────────────
  let wasmReady = false;
  let wasm = {};

  async function initWasm() {
    try {
      const mod = await import("./pkg/staveloom_wasm.js");
      await mod.default();
      wasm.parse_and_render = mod.parse_and_render;
      wasm.parse_midi_and_render = mod.parse_midi_and_render;
      wasm.list_midi_parts = mod.list_midi_parts;
      wasm.list_parts = mod.list_parts;
      wasm.list_instruments = mod.list_instruments;
      wasmReady = true;
    } catch (e) {
      console.error("WASM init failed:", e);
      showError("WASM module failed to load. Try refreshing the page.");
    }
  }

  // ── MIDI Player ──────────────────────────────────────────────────────────
  const midiPlayer = new MidiPlayerController();

  // ── State ────────────────────────────────────────────────────────────────
  let state = {
    filename: null,
    metadata: null,
    parts: [],
    instruments: [],
    currentZoom: 100,
    isPlaying: false,
    isScrubbing: false,
    rawFileBytes: null,
    midiBytes: null,
    svgContent: null, // combined SVG string for download
    systems: [], // array of SystemSvg objects from WASM
    currentSystemIndex: 0,
    currentLineY: null,
    mobileUserOverridden: false,
    preMobileWidthValue: null,
  };

  const MOBILE_AUTO_BREAKPOINT = 480;

  let animFrameId = null;

  // ── DOM Elements ─────────────────────────────────────────────────────────
  const dropzone = document.getElementById("dropzone");
  const fileInput = document.getElementById("file-input");
  const loadingOverlay = document.getElementById("loading-overlay");
  const loadingText = document.getElementById("loading-text");

  const scoreInfoSection = document.getElementById("score-info-section");
  const scoreTitle = document.getElementById("score-title");
  const scoreCreator = document.getElementById("score-creator");

  const partsSection = document.getElementById("parts-section");
  const partsList = document.getElementById("parts-list");
  const applyFiltersBtn = document.getElementById("apply-filters-btn");

  const mobileToggle = document.getElementById("mobile-toggle");
  const elasticToggle = document.getElementById("elastic-toggle");
  const horizontalToggle = document.getElementById("horizontal-toggle");
  const widthSlider = document.getElementById("width-slider");
  const widthVal = document.getElementById("width-val");
  const widthControlGroup = document.getElementById("width-control-group");

  const scoreViewport = document.getElementById("score-viewport");
  const scoreCanvasContainer = document.getElementById(
    "score-canvas-container",
  );
  const scoreCanvas = document.getElementById("score-canvas");

  const floatingController = document.getElementById("floating-controller");
  const playBtn = document.getElementById("play-btn");
  const playIcon = document.getElementById("play-icon");
  const pauseIcon = document.getElementById("pause-icon");
  const rewindBtn = document.getElementById("rewind-btn");

  const timelineScrubber = document.getElementById("timeline-scrubber");
  const timelineProgressBar = document.getElementById("timeline-progress-bar");
  const currentTimeEl = document.getElementById("current-time");
  const totalTimeEl = document.getElementById("total-time");

  const muteBtn = document.getElementById("mute-btn");
  const volUpIcon = document.getElementById("vol-up-icon");
  const volMuteIcon = document.getElementById("vol-mute-icon");
  const volumeSlider = document.getElementById("volume-slider");

  const zoomInBtn = document.getElementById("zoom-in-btn");
  const zoomOutBtn = document.getElementById("zoom-out-btn");
  const zoomLevelEl = document.getElementById("zoom-level");

  const downloadSvgBtn = document.getElementById("download-svg-btn");
  const downloadMidiBtn = document.getElementById("download-midi-btn");

  // ── Virtual Renderer ─────────────────────────────────────────────────────
  let virtualRenderer;

  // ── Initialization ───────────────────────────────────────────────────────
  async function init() {
    virtualRenderer = new VirtualScoreRenderer(scoreCanvas, scoreViewport);

    const wasmProm = initWasm();
    const sfProm = midiPlayer
      .init()
      .catch((e) => console.warn("SoundFontLoader init failed:", e));

    applyAutoMobileDetect(true);
    window.addEventListener("resize", () => {
      if (!state.mobileUserOverridden) applyAutoMobileDetect(true);
    });

    setupDragAndDrop();
    setupControls();
    setupKeyboardShortcuts();
    setupMidiPlayerCallbacks();
    setupMobileAudio();

    await Promise.all([wasmProm, sfProm]);
  }

  // ── Mobile Layout Auto-Detect ────────────────────────────────────────────
  function applyAutoMobileDetect(snapWidth) {
    mobileToggle.checked = window.innerWidth <= MOBILE_AUTO_BREAKPOINT;
    syncMobileDependentControls(snapWidth);
  }

  // Keep the elastic toggle and width slider in sync with the mobile toggle's
  // current state. Snaps the width down on the off→on transition and restores
  // it back on the on→off transition, so turning mobile off actually reverts
  // the rendered width (not just the internal spacing constants).
  function syncMobileDependentControls(snapWidth) {
    elasticToggle.disabled = mobileToggle.checked;
    if (!snapWidth) return;
    if (mobileToggle.checked) {
      if (state.preMobileWidthValue === null) {
        state.preMobileWidthValue = widthSlider.value;
      }
      const snapped = Math.max(320, Math.min(window.innerWidth, 500));
      widthSlider.value = snapped;
      widthVal.innerText = `${snapped}px`;
    } else if (state.preMobileWidthValue !== null) {
      widthSlider.value = state.preMobileWidthValue;
      widthVal.innerText = `${state.preMobileWidthValue}px`;
      state.preMobileWidthValue = null;
    }
  }

  // ── Drag & Drop ──────────────────────────────────────────────────────────
  function setupDragAndDrop() {
    dropzone.addEventListener("click", () => fileInput.click());
    fileInput.addEventListener("change", (e) => {
      if (e.target.files.length > 0) handleFile(e.target.files[0]);
    });

    ["dragenter", "dragover"].forEach((ev) => {
      dropzone.addEventListener(
        ev,
        (e) => {
          e.preventDefault();
          e.stopPropagation();
          dropzone.classList.add("dragover");
        },
        false,
      );
    });

    ["dragleave", "drop"].forEach((ev) => {
      dropzone.addEventListener(
        ev,
        (e) => {
          e.preventDefault();
          e.stopPropagation();
          dropzone.classList.remove("dragover");
        },
        false,
      );
    });

    dropzone.addEventListener(
      "drop",
      (e) => {
        const files = e.dataTransfer.files;
        if (files.length > 0) {
          // drop is a user gesture — unlock audio before going async
          midiPlayer.unlockAudio();
          handleFile(files[0]);
        }
      },
      false,
    );
  }

  // ── Controls ─────────────────────────────────────────────────────────────
  function setupControls() {
    mobileToggle.addEventListener("change", () => {
      state.mobileUserOverridden = true;
      syncMobileDependentControls(true);
      triggerRender();
    });

    elasticToggle.addEventListener("change", triggerRender);

    horizontalToggle.addEventListener("change", () => {
      widthControlGroup.classList.toggle("hidden", horizontalToggle.checked);
      state.currentZoom = 100;
      triggerRender();
    });

    widthSlider.addEventListener("input", (e) => {
      widthVal.innerText = `${e.target.value}px`;
    });
    widthSlider.addEventListener("change", triggerRender);

    applyFiltersBtn.addEventListener("click", triggerRender);

    playBtn.addEventListener("click", togglePlay);
    rewindBtn.addEventListener("click", rewind);

    timelineScrubber.addEventListener("mousedown", () => {
      state.isScrubbing = true;
    });
    timelineScrubber.addEventListener("touchstart", () => {
      state.isScrubbing = true;
    });

    timelineScrubber.addEventListener("input", (e) => {
      const pct = parseFloat(e.target.value);
      timelineProgressBar.style.width = `${pct}%`;
      const dur = midiPlayer.duration;
      if (dur) currentTimeEl.innerText = formatTime((pct / 100) * dur);
    });

    timelineScrubber.addEventListener("change", (e) => {
      const pct = parseFloat(e.target.value);
      const dur = midiPlayer.duration;
      if (dur) midiPlayer.currentTime = (pct / 100) * dur;
      state.isScrubbing = false;
      updateCursorPosition();
    });

    muteBtn.addEventListener("click", toggleMute);
    volumeSlider.addEventListener("input", (e) => {
      midiPlayer.setVolume(parseFloat(e.target.value));
      updateVolumeUI();
    });

    zoomInBtn.addEventListener("click", () => adjustZoom(10));
    zoomOutBtn.addEventListener("click", () => adjustZoom(-10));

    downloadSvgBtn.addEventListener("click", downloadSVG);
    downloadMidiBtn.addEventListener("click", downloadMIDI);
  }

  function setupKeyboardShortcuts() {
    window.addEventListener("keydown", (e) => {
      const tag = document.activeElement.tagName;
      if (tag === "INPUT" || tag === "BUTTON") return;

      if (e.code === "Space") {
        e.preventDefault();
        togglePlay();
      } else if (e.code === "ArrowLeft") {
        e.preventDefault();
        const t = Math.max(0, midiPlayer.currentTime - 5);
        midiPlayer.currentTime = t;
        updateTimelineScrubber();
        updateCursorPosition();
      } else if (e.code === "ArrowRight") {
        e.preventDefault();
        const t = Math.min(
          midiPlayer.duration || 0,
          midiPlayer.currentTime + 5,
        );
        midiPlayer.currentTime = t;
        updateTimelineScrubber();
        updateCursorPosition();
      }
    });
  }

  function setupMobileAudio() {
    // iOS suspends AudioContext after backgrounding or inactivity.
    // Every touchstart is a valid user gesture — use it to keep the context alive.
    document.addEventListener(
      "touchstart",
      () => {
        midiPlayer.unlockAudio();
      },
      { passive: true, capture: true },
    );

    // When the page becomes visible again (tab switch back), attempt to resume.
    // This alone may not suffice on iOS (visibilitychange isn't always a gesture),
    // but combined with the next touchstart it ensures audio resumes promptly.
    document.addEventListener("visibilitychange", () => {
      if (document.visibilityState === "visible") {
        midiPlayer.unlockAudio();
      }
    });
  }

  function setupMidiPlayerCallbacks() {
    midiPlayer.onEnded = () => {
      pauseIcon.classList.add("hidden");
      playIcon.classList.remove("hidden");
      state.isPlaying = false;
      cancelAnimationFrame(animFrameId);
      midiPlayer.currentTime = 0;
      timelineScrubber.value = 0;
      timelineProgressBar.style.width = "0%";
      currentTimeEl.innerText = "0:00";
      updateCursorPosition();
    };
  }

  // ── MIDI Scanning ────────────────────────────────────────────────────────
  function scanMidiPrograms(midi) {
    const programs = new Set();
    for (let i = 0; i < midi.length - 1; i++) {
      if (midi[i] >= 0xc0 && midi[i] <= 0xcf && midi[i + 1] < 0x80) {
        const channel = midi[i] & 0x0f;
        programs.add(channel === 9 ? 128 : midi[i + 1]);
      }
    }
    return [...programs].map((p) => ({ program: p }));
  }

  // ── File Handling ────────────────────────────────────────────────────────
  function handleFile(file) {
    if (!wasmReady) {
      showError("WASM is still loading. Please wait a moment and try again.");
      return;
    }
    // iOS: AudioContext must be created/resumed while still in the synchronous
    // user-gesture call stack. FileReader.onload is async and loses gesture context.
    midiPlayer.unlockAudio();
    const reader = new FileReader();
    reader.onload = async (e) => {
      const bytes = new Uint8Array(e.target.result);
      await processFile(file.name, bytes, true);
    };
    reader.readAsArrayBuffer(file);
  }

  function isMidiFile(bytes) {
    // MIDI magic: 'MThd' = 0x4D 0x54 0x68 0x64
    return (
      bytes.length >= 4 &&
      bytes[0] === 0x4d &&
      bytes[1] === 0x54 &&
      bytes[2] === 0x68 &&
      bytes[3] === 0x64
    );
  }

  async function processFile(filename, bytes, fullReset) {
    // Stop audio immediately so the old song does not play during re-render/SF2 load.
    if (state.isPlaying) {
      midiPlayer.pause();
      state.isPlaying = false;
      playIcon.classList.remove("hidden");
      pauseIcon.classList.add("hidden");
      cancelAnimationFrame(animFrameId);
    }
    showLoading("Parsing & rendering score...");
    try {
      const midi = isMidiFile(bytes);

      let result, parts, instruments;

      if (midi) {
        // MIDI path: list parts, apply filter, render
        if (fullReset) {
          parts = wasm.list_midi_parts(bytes);
          instruments = [];
        } else {
          parts = state.parts;
          instruments = state.instruments;
        }

        let filterCsv = "";
        if (!fullReset && state.parts.length > 1) {
          const checked = partsList.querySelectorAll(
            'input[type="checkbox"]:checked',
          );
          const active = Array.from(checked).map((cb) => cb.value);
          filterCsv = active.join(",");
        }

        result = wasm.parse_midi_and_render(
          bytes,
          elasticToggle.checked,
          mobileToggle.checked,
          horizontalToggle.checked,
          parseInt(widthSlider.value),
          filterCsv,
        );
      } else {
        // MusicXML path (original behaviour)
        parts = wasm.list_parts(bytes);
        instruments = wasm.list_instruments(bytes);

        let filterCsv = "";
        if (!fullReset && state.parts.length > 1) {
          const checked = partsList.querySelectorAll(
            'input[type="checkbox"]:checked',
          );
          const active = Array.from(checked).map((cb) => cb.value);
          filterCsv = active.join(",");
        }

        result = wasm.parse_and_render(
          bytes,
          elasticToggle.checked,
          mobileToggle.checked,
          horizontalToggle.checked,
          parseInt(widthSlider.value),
          filterCsv,
        );
      }

      if (fullReset) {
        state.filename = filename;
        state.parts = parts;
        state.instruments = instruments;
        state.rawFileBytes = bytes;
        state.isMidi = midi;
        populatePartsList(parts);
        scoreTitle.innerText = filename;
        scoreCreator.innerText = midi
          ? "MIDI File"
          : instruments.length > 0
            ? instruments.map((i) => i.name).join(", ")
            : "No Instruments";
        scoreInfoSection.classList.remove("hidden");
        if (partsSection) {
          partsSection.classList.remove("hidden");
        }
      }

      // Store systems and build combined SVG for download
      state.systems = result.systems;
      state.svgContent = buildCombinedSvg(result.systems);
      state.metadata = result.metadata;
      state.midiBytes = result.midi;
      state.currentSystemIndex = 0;
      state.currentLineY = null;
      // Seek to beginning so cursor shows at beat 0, not the previous file's position.
      if (midiPlayer.isReady) midiPlayer.currentTime = 0;

      // Mount into virtual renderer
      if (result.systems.length === 1) {
        virtualRenderer.loadSingle(result.systems[0].svg_content);
      } else {
        virtualRenderer.load(result.systems);
      }

      // Layout
      if (horizontalToggle.checked) {
        scoreViewport.classList.add("horizontal-mode");
      } else {
        scoreViewport.classList.remove("horizontal-mode");
      }
      if (fullReset) {
        // New file: scroll to beginning. rAF ensures it fires after browser lays out the new content.
        requestAnimationFrame(() => {
          scoreViewport.scrollLeft = 0;
          scoreViewport.scrollTop = 0;
        });
      }

      updateZoomUI();
      floatingController.classList.remove("hidden");
      // Reset timeline display to 0:00 / 0:00 before MIDI loads
      timelineScrubber.value = 0;
      timelineProgressBar.style.width = "0%";
      currentTimeEl.innerText = "0:00";
      totalTimeEl.innerText = "0:00";
      updateCursorPosition();

      hideLoading();

      // Load MIDI into SpessaSynth (async, non-blocking)
      showLoadingSubtle("Loading soundfont...");
      try {
        const declaredPrograms = new Set(
          (instruments || []).map((i) => i.program),
        );
        const extraInstruments = scanMidiPrograms(result.midi).filter(
          (p) => !declaredPrograms.has(p.program),
        );
        const allInstruments = [...(instruments || []), ...extraInstruments];
        await midiPlayer.loadMidi(result.midi, allInstruments);
        totalTimeEl.innerText = formatTime(midiPlayer.duration);
        // Re-position cursor at t=0 using the newly loaded MIDI metadata
        updateCursorPosition();
      } catch (e) {
        console.error("MIDI/SoundFont load failed:", e);
        showError(`Soundfont load failed: ${e.message}`);
      } finally {
        hideLoadingSubtle();
      }
    } catch (e) {
      hideLoading();
      console.error("Render error:", e);
      showError(`Failed to render score: ${e.message || e}`);
    }
  }

  async function triggerRender() {
    if (!state.rawFileBytes || !wasmReady) return;
    await processFile(state.filename, state.rawFileBytes, false);
  }

  // Build a single combined SVG from per-system SVGs for download
  function buildCombinedSvg(systems) {
    if (systems.length === 0) return "";
    if (systems.length === 1) return systems[0].svg_content;
    const w = systems[0].width;
    const last = systems[systems.length - 1];
    const h = last.y_offset + last.height;
    const styleMatch = systems[0].svg_content.match(/<style>[\s\S]*?<\/style>/);
    const style = styleMatch ? styleMatch[0] : "";
    const groups = systems
      .map((sys) => {
        const svg = sys.svg_content;
        const gStart = svg.indexOf("<g ");
        if (gStart === -1) return "";
        const openEnd = svg.indexOf(">", gStart) + 1;
        let depth = 1,
          j = openEnd;
        while (j < svg.length && depth > 0) {
          if (svg[j] !== "<") {
            j++;
            continue;
          }
          if (
            svg[j + 1] === "g" &&
            (svg[j + 2] === " " || svg[j + 2] === ">")
          ) {
            depth++;
            j += 2;
          } else if (
            svg[j + 1] === "/" &&
            svg[j + 2] === "g" &&
            svg[j + 3] === ">"
          ) {
            depth--;
            if (depth === 0) break;
            j += 4;
          } else j++;
        }
        return `<g id="s${sys.index}">${svg.substring(openEnd, j)}</g>`;
      })
      .join("\n");
    return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${w} ${h}" width="${w}" height="${h}">\n${style}\n${groups}\n</svg>`;
  }

  // ── Parts List ───────────────────────────────────────────────────────────
  function populatePartsList(parts) {
    partsList.innerHTML = "";
    if (parts.length <= 1) {
      partsSection.classList.add("hidden");
      return;
    }
    parts.forEach((part) => {
      const label = document.createElement("label");
      label.className = "checkbox-item";
      const checkbox = document.createElement("input");
      checkbox.type = "checkbox";
      checkbox.value = part.id;
      checkbox.checked = true;
      label.appendChild(checkbox);
      label.appendChild(document.createTextNode(part.name || part.id));
      partsList.appendChild(label);
    });
    partsSection.classList.remove("hidden");
  }

  // ── Playback Loop ────────────────────────────────────────────────────────
  function startPlaybackLoop() {
    function loop() {
      if (!state.isScrubbing) {
        updateTimelineScrubber();
        updateCursorPosition();
      }
      animFrameId = requestAnimationFrame(loop);
    }
    animFrameId = requestAnimationFrame(loop);
  }

  function updateTimelineScrubber() {
    const dur = midiPlayer.duration;
    if (!dur) return;
    const t = midiPlayer.currentTime;
    const pct = (t / dur) * 100;
    timelineScrubber.value = pct;
    timelineProgressBar.style.width = `${pct}%`;
    currentTimeEl.innerText = formatTime(t);
    totalTimeEl.innerText = formatTime(dur);
  }

  function updateCursorPosition() {
    const beats = state.metadata ? state.metadata.beats : null;
    if (!beats || beats.length === 0) {
      virtualRenderer.hideCursor();
      return;
    }

    const currTime = midiPlayer.currentTime + midiPlayer.audioLatency;
    const pos = getCursorPosition(currTime, beats);

    if (pos) {
      virtualRenderer.updateCursor(
        pos.system_index,
        pos.x,
        pos.y_start,
        pos.y_end,
      );
      autoScroll(pos);
    } else {
      virtualRenderer.hideCursor();
    }
  }

  function getCursorPosition(time, beats) {
    let idx = -1;
    for (let i = 0; i < beats.length; i++) {
      if (beats[i].time_seconds <= time) idx = i;
      else break;
    }

    if (idx === -1) {
      const b = beats[0];
      return {
        x: b.x,
        y_start: b.y_start,
        y_end: b.y_end,
        system_index: b.system_index || 0,
      };
    }
    if (idx === beats.length - 1) {
      const b = beats[idx];
      return {
        x: b.x,
        y_start: b.y_start,
        y_end: b.y_end,
        system_index: b.system_index || 0,
      };
    }

    const beatA = beats[idx];
    const beatB = beats[idx + 1];
    const sameLine =
      beatA.system_index === beatB.system_index &&
      Math.abs(beatA.y_start - beatB.y_start) < 10 &&
      beatB.x > beatA.x;

    if (sameLine) {
      const ratio =
        (time - beatA.time_seconds) / (beatB.time_seconds - beatA.time_seconds);
      return {
        x: beatA.x + ratio * (beatB.x - beatA.x),
        y_start: beatA.y_start,
        y_end: beatA.y_end,
        system_index: beatA.system_index || 0,
      };
    } else {
      const ratio =
        (time - beatA.time_seconds) / (beatB.time_seconds - beatA.time_seconds);
      const beat = ratio < 0.85 ? beatA : beatB;
      return {
        x: beat.x,
        y_start: beat.y_start,
        y_end: beat.y_end,
        system_index: beat.system_index || 0,
      };
    }
  }

  function autoScroll(pos) {
    if (horizontalToggle.checked) {
      virtualRenderer.scrollHorizontalToCursor();
    } else {
      // Checked on every cursor update (not just on system change) so the
      // cursor gets scrolled back into view if it drifts off-screen (e.g.
      // the user scrolled away manually) even while staying on one system.
      // scrollToSystem() is a cheap no-op once the cursor is fully visible.
      state.currentSystemIndex = pos.system_index;
      virtualRenderer.scrollToSystem(pos.system_index);
    }
  }

  // ── Playback Controls ────────────────────────────────────────────────────
  function togglePlay() {
    if (!midiPlayer.isReady) {
      startPlayback();
      return;
    }
    if (state.isPlaying) {
      midiPlayer.pause();
      state.isPlaying = false;
      playIcon.classList.remove("hidden");
      pauseIcon.classList.add("hidden");
      cancelAnimationFrame(animFrameId);
    } else {
      startPlayback();
    }
  }

  async function startPlayback() {
    if (!state.midiBytes) return;
    // iOS: play button is a user gesture — unlock/resume AudioContext here.
    // await midiPlayer.play() so we don't mark isPlaying until the AudioContext
    // is confirmed running (resume() is async; calling seq.play() before it
    // completes produces silence on iOS).
    midiPlayer.unlockAudio();
    await midiPlayer.play();
    state.isPlaying = true;
    playIcon.classList.add("hidden");
    pauseIcon.classList.remove("hidden");
    startPlaybackLoop();
  }

  function rewind() {
    midiPlayer.currentTime = 0;
    updateTimelineScrubber();
    updateCursorPosition();
  }

  function toggleMute() {
    const nowMuted = !volumeSlider.disabled;
    midiPlayer.setMuted(nowMuted);
    volUpIcon.classList.toggle("hidden", nowMuted);
    volMuteIcon.classList.toggle("hidden", !nowMuted);
    volumeSlider.disabled = nowMuted;
  }

  function updateVolumeUI() {
    const vol = parseFloat(volumeSlider.value);
    const muted = vol === 0;
    volUpIcon.classList.toggle("hidden", muted);
    volMuteIcon.classList.toggle("hidden", !muted);
  }

  // ── Zoom ─────────────────────────────────────────────────────────────────
  function adjustZoom(delta) {
    state.currentZoom = Math.min(250, Math.max(50, state.currentZoom + delta));
    updateZoomUI();
    updateCursorPosition();
  }

  function updateZoomUI() {
    zoomLevelEl.innerText = `${state.currentZoom}%`;
    scoreCanvasContainer.style.transform = `scale(${state.currentZoom / 100})`;
  }

  // ── Downloads ────────────────────────────────────────────────────────────
  function downloadSVG() {
    if (!state.svgContent) return;
    const blob = new Blob([state.svgContent], {
      type: "image/svg+xml;charset=utf-8",
    });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `${(state.filename || "score").replace(/\.[^.]+$/, "")}.svg`;
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    URL.revokeObjectURL(url);
  }

  function downloadMIDI() {
    if (!state.midiBytes) return;
    const blob = new Blob([state.midiBytes], { type: "audio/midi" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `${(state.filename || "score").replace(/\.[^.]+$/, "")}.mid`;
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    URL.revokeObjectURL(url);
  }

  // ── Loading / Error UI ───────────────────────────────────────────────────
  function showLoading(msg) {
    loadingText.innerText = msg;
    loadingOverlay.classList.remove("hidden");
  }

  function hideLoading() {
    loadingOverlay.classList.add("hidden");
  }

  function showLoadingSubtle(msg) {
    loadingText.innerText = msg;
    loadingOverlay.classList.remove("hidden");
  }

  function hideLoadingSubtle() {
    loadingOverlay.classList.add("hidden");
  }

  function showError(msg) {
    console.error(msg);
    const existing = document.getElementById("staveloom-error-toast");
    if (existing) existing.remove();
    const toast = document.createElement("div");
    toast.id = "staveloom-error-toast";
    toast.style.cssText =
      "position:fixed;bottom:24px;left:50%;transform:translateX(-50%);background:#e53935;color:#fff;padding:12px 20px;border-radius:8px;font-size:14px;z-index:9999;max-width:80%";
    toast.textContent = msg;
    document.body.appendChild(toast);
    setTimeout(() => toast.remove(), 6000);
  }

  function formatTime(seconds) {
    if (!seconds || isNaN(seconds) || seconds === Infinity) return "0:00";
    const mins = Math.floor(seconds / 60);
    const secs = Math.floor(seconds % 60);
    return `${mins}:${secs.toString().padStart(2, "0")}`;
  }

  // ── Start ────────────────────────────────────────────────────────────────
  document.addEventListener("DOMContentLoaded", () => init());
})();
