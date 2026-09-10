// VirtualScoreRenderer — Phase 3: IntersectionObserver-based system virtualization
//
// Memory strategy:
//   Short scores (≤ COMBINE_THRESHOLD systems): combine all into one <svg> via
//   _combineSystems() then call loadSingle(). This avoids N separate rendering
//   contexts (each <svg> element creates its own layout/paint context in the browser).
//
//   Long scores (> COMBINE_THRESHOLD): virtualize — only ~5 systems mounted at once.
//   rootMargin is computed from actual system geometry instead of a percentage so that
//   the pre-load zone stays at ~2 system heights regardless of viewport size.

const COMBINE_THRESHOLD = 10;

class VirtualScoreRenderer {
  constructor(container, scrollRoot) {
    this.container = container; // div#score-canvas
    this.scrollRoot = scrollRoot; // div#score-viewport (IntersectionObserver root)
    this.systems = [];
    this.wrappers = [];
    this.mounted = new Set();
    this.observer = null;

    this._cursorSystemIndex = -1;
    this._cursorX = 0;
    this._cursorYStart = 0;
    this._cursorYEnd = 0;
    this._cursorVisible = false;
    this._singleMode = false;
  }

  // ── Load (multi-system virtualized) ───────────────────────────────────────
  load(systems) {
    this._teardown();

    // Short scores: combine all systems into one SVG to avoid N rendering contexts.
    // For ≤ COMBINE_THRESHOLD systems, single-SVG memory is always better.
    if (systems.length <= COMBINE_THRESHOLD) {
      this.loadSingle(this._combineSystems(systems));
      return;
    }

    this._singleMode = false;
    this.systems = systems;

    for (let i = 0; i < systems.length; i++) {
      const wrapper = document.createElement("div");
      wrapper.className = "system-wrapper";
      wrapper.dataset.systemIndex = i;
      // Responsive: fill container width, height via aspect-ratio
      wrapper.style.width = "100%";
      wrapper.style.aspectRatio = `${systems[i].width} / ${systems[i].height}`;
      this.container.appendChild(wrapper);
      this.wrappers.push(wrapper);
    }

    // Compute rootMargin from actual system geometry (~2 system heights).
    // This keeps ~5 systems mounted at once regardless of viewport size, instead
    // of '100% 0px' which mounted ~21 systems on a 600px viewport.
    const sysAspect = systems[0].height / systems[0].width;
    const approxSysH = Math.ceil(
      (this.scrollRoot.clientWidth || 800) * sysAspect,
    );
    const margin = Math.max(approxSysH * 2, 120);

    this.observer = new IntersectionObserver(
      (entries) => this._handleIntersection(entries),
      {
        root: this.scrollRoot,
        rootMargin: `${margin}px 0px`,
        threshold: 0,
      },
    );
    for (const w of this.wrappers) this.observer.observe(w);
  }

  // ── Load (single SVG — horizontal mode, short score, or 1-system score) ──
  loadSingle(svgContent) {
    this._teardown();
    this._singleMode = true;
    this.container.innerHTML = svgContent;
    const svgEl = this.container.querySelector("svg");
    if (svgEl) this._injectCursor(svgEl);
  }

  // ── Cursor ────────────────────────────────────────────────────────────────
  // systemIndex: which system owns the cursor
  // x, y_start, y_end: SVG coordinate space (full-SVG absolute coordinates)
  updateCursor(systemIndex, x, y_start, y_end) {
    if (this._singleMode) {
      const svgEl = this.container.querySelector("svg");
      if (!svgEl) return;
      this._applyCursor(svgEl, x, y_start, y_end);
      this._cursorVisible = true;
      return;
    }

    const prevIdx = this._cursorSystemIndex;

    if (prevIdx !== systemIndex) {
      if (prevIdx >= 0 && this.wrappers[prevIdx]) {
        // Re-observe old system so the Observer can unmount it when out of range.
        if (this.observer) this.observer.observe(this.wrappers[prevIdx]);
        const c = this.wrappers[prevIdx].querySelector("#playback-cursor");
        if (c) c.classList.add("hidden");
      }
      if (this.wrappers[systemIndex] && this.observer) {
        // Unobserve cursor system so the Observer cannot unmount it mid-playback.
        this.observer.unobserve(this.wrappers[systemIndex]);
      }
    }

    this._cursorSystemIndex = systemIndex;
    this._cursorX = x;
    this._cursorYStart = y_start;
    this._cursorYEnd = y_end;
    this._cursorVisible = true;

    const wrapper = this.wrappers[systemIndex];
    if (!wrapper) return;
    let svgEl = wrapper.querySelector("svg");
    if (!svgEl) {
      this._mountSystem(systemIndex);
      svgEl = wrapper.querySelector("svg");
    }
    if (svgEl) this._applyCursor(svgEl, x, y_start, y_end);
  }

  hideCursor() {
    this._cursorVisible = false;
    if (this._singleMode) {
      const c = this.container.querySelector("#playback-cursor");
      if (c) c.classList.add("hidden");
      return;
    }
    // Re-observe cursor system so it can be naturally unmounted when scrolled away
    if (
      this._cursorSystemIndex >= 0 &&
      this.wrappers[this._cursorSystemIndex]
    ) {
      if (this.observer)
        this.observer.observe(this.wrappers[this._cursorSystemIndex]);
      const c =
        this.wrappers[this._cursorSystemIndex].querySelector(
          "#playback-cursor",
        );
      if (c) c.classList.add("hidden");
    }
  }

  // ── Scroll ────────────────────────────────────────────────────────────────
  // Called on every cursor update (not just when the system changes), so it
  // must be a cheap no-op when the cursor is already fully visible.
  scrollToSystem(systemIndex) {
    if (this._singleMode) {
      // Combined single-SVG mode: the cursor is already at its new position.
      const cursor = this.container.querySelector("#playback-cursor");
      if (!cursor || cursor.classList.contains("hidden")) return;
      this._scrollElementIntoView(cursor);
      return;
    }
    const wrapper = this.wrappers[systemIndex];
    if (!wrapper) return;
    // Prefer the cursor's own vertical span (all parts stacked at this
    // beat) — that's what actually needs to be visible, and it's the
    // signal for "too many parts to show at once" below. Fall back to the
    // whole system wrapper if the cursor isn't mounted yet.
    const cursor = wrapper.querySelector("#playback-cursor");
    this._scrollElementIntoView(cursor && !cursor.classList.contains("hidden") ? cursor : wrapper);
  }

  // One staff's height in renderer coordinate units (4 line-gaps at the
  // default staff_line_distance of 10 — see Renderer::default()).
  static STAFF_HEIGHT_UNITS = 40;

  // Scrolls `el` fully into view if it isn't already, aligning its top to
  // the viewport top with a staff-height's worth of headroom so the system
  // above isn't clipped flush against the edge.
  _scrollElementIntoView(el) {
    if (!el) return;
    const elRect = el.getBoundingClientRect();
    const rootRect = this.scrollRoot.getBoundingClientRect();
    const fullyVisible =
      elRect.top >= rootRect.top && elRect.bottom <= rootRect.bottom;
    if (fullyVisible) return;
    const svgEl =
      (el.closest && el.closest("svg")) ||
      (el.querySelector && el.querySelector("svg"));
    const svgHeightAttr = svgEl ? parseFloat(svgEl.getAttribute("height")) : 0;
    const pxPerUnit =
      svgEl && svgHeightAttr
        ? svgEl.getBoundingClientRect().height / svgHeightAttr
        : 1;
    const margin = VirtualScoreRenderer.STAFF_HEIGHT_UNITS * pxPerUnit;
    this.scrollRoot.scrollTop += elRect.top - rootRect.top - margin;
  }

  // ── Horizontal scroll (single SVG mode) ──────────────────────────────────
  scrollHorizontalToCursor() {
    if (!this._singleMode) return;
    const svgEl = this.container.querySelector("svg");
    if (!svgEl) return;
    const cursor = svgEl.getElementById("playback-cursor");
    if (!cursor || cursor.classList.contains("hidden")) return;
    const cursorRect = cursor.getBoundingClientRect();
    const rootRect = this.scrollRoot.getBoundingClientRect();
    const cursorCenter = cursorRect.left + cursorRect.width / 2;
    const viewportCenter = rootRect.left + rootRect.width / 2;
    this.scrollRoot.scrollLeft += cursorCenter - viewportCenter;
  }

  get mountedCount() {
    return this.mounted.size;
  }

  // ── Private ───────────────────────────────────────────────────────────────
  _teardown() {
    if (this.observer) {
      this.observer.disconnect();
      this.observer = null;
    }
    this.container.innerHTML = "";
    this.systems = [];
    this.wrappers = [];
    this.mounted.clear();
    this._cursorSystemIndex = -1;
    this._cursorVisible = false;
  }

  _handleIntersection(entries) {
    for (const entry of entries) {
      const idx = parseInt(entry.target.dataset.systemIndex, 10);
      if (entry.isIntersecting) {
        this._mountSystem(idx);
      } else {
        this._unmountSystem(idx);
      }
    }
  }

  _mountSystem(idx) {
    if (this.mounted.has(idx)) return;
    const wrapper = this.wrappers[idx];
    const sys = this.systems[idx];
    if (!wrapper || !sys) return;
    wrapper.innerHTML = sys.svg_content;
    this.mounted.add(idx);
    const svgEl = wrapper.querySelector("svg");
    if (svgEl) {
      this._injectCursor(svgEl);
      // Re-apply cursor if this system is the active one
      if (this._cursorVisible && idx === this._cursorSystemIndex) {
        this._applyCursor(
          svgEl,
          this._cursorX,
          this._cursorYStart,
          this._cursorYEnd,
        );
      }
    }
  }

  _unmountSystem(idx) {
    if (!this.mounted.has(idx)) return;
    // Cursor system is unobserved so the observer never calls this for it.
    this.wrappers[idx].innerHTML = "";
    this.mounted.delete(idx);
  }

  // Combine N per-system SVGs into a single full-height SVG.
  // Each per-system SVG has viewBox="0 y_start W H" with elements in absolute
  // coordinates. Merging into viewBox="0 0 W total_H" places all groups correctly.
  _combineSystems(systems) {
    if (systems.length === 0) return "";
    const w = systems[0].width;
    const last = systems[systems.length - 1];
    const h = last.y_offset + last.height;
    const styleMatch = systems[0].svg_content.match(/<style>[\s\S]*?<\/style>/);
    const style = styleMatch ? styleMatch[0] : "";
    const groups = systems
      .map((sys) => {
        const content = this._extractGroupContent(sys.svg_content);
        return `<g id="s${sys.index}">${content}</g>`;
      })
      .join("\n");
    return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${w} ${h}" width="${w}" height="${h}">\n${style}\n${groups}\n</svg>`;
  }

  // Extract inner content of the first <g ...>...</g> with correct nesting depth.
  _extractGroupContent(svg) {
    const gStart = svg.indexOf("<g ");
    if (gStart === -1) return "";
    const openEnd = svg.indexOf(">", gStart) + 1;
    let depth = 1,
      i = openEnd;
    while (i < svg.length && depth > 0) {
      if (svg[i] !== "<") {
        i++;
        continue;
      }
      // Opening <g> or <g>
      if (svg[i + 1] === "g" && (svg[i + 2] === " " || svg[i + 2] === ">")) {
        depth++;
        i += 2;
        // Closing </g>
      } else if (
        svg[i + 1] === "/" &&
        svg[i + 2] === "g" &&
        svg[i + 3] === ">"
      ) {
        depth--;
        if (depth === 0) break;
        i += 4;
      } else {
        i++;
      }
    }
    return svg.substring(openEnd, i);
  }

  _injectCursor(svgEl) {
    if (!svgEl || svgEl.getElementById("playback-cursor")) return;
    const cursor = document.createElementNS(
      "http://www.w3.org/2000/svg",
      "line",
    );
    cursor.setAttribute("id", "playback-cursor");
    cursor.setAttribute("stroke", "#ff0055");
    cursor.setAttribute("stroke-width", "2.5");
    cursor.setAttribute("stroke-opacity", "0.85");
    cursor.setAttribute(
      "style",
      "transition: x1 0.04s linear, x2 0.04s linear;",
    );
    cursor.classList.add("hidden");
    svgEl.appendChild(cursor);
  }

  _applyCursor(svgEl, x, y_start, y_end) {
    let cursor = svgEl.getElementById("playback-cursor");
    if (!cursor) {
      this._injectCursor(svgEl);
      cursor = svgEl.getElementById("playback-cursor");
    }
    if (!cursor) return;
    cursor.setAttribute("x1", x.toString());
    cursor.setAttribute("y1", y_start.toString());
    cursor.setAttribute("x2", x.toString());
    cursor.setAttribute("y2", y_end.toString());
    cursor.classList.remove("hidden");
  }
}
