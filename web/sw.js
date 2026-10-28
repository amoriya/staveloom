// Service Worker — staveloom Web Player PWA
// Strategy:
//   install  → precache all static app assets
//   activate → purge stale caches
//   fetch    → pages network-first (cache = offline fallback); other GET
//              requests cache-first; dynamic caching on network hit
//              ignoreSearch: true so versioned URLs (?v=N) match cached assets

const CACHE_VERSION = "v53";
const CACHE_NAME = `staveloom-player-${CACHE_VERSION}`;

// Static assets to precache on install.
// Versioned JS files are matched with ignoreSearch:true so ?v=N query strings
// do not cause cache misses. soundfonts/index.json is small and changes rarely.
const STATIC_ASSETS = [
  "./",
  "./index.html",
  "./style.css",
  "./player.js",
  "./virtual-score.js",
  "./soundfont-loader.js",
  "./midi-player.js",
  "./manifest.json",
  "./icon-192.png",
  "./icon-512.png",
  "./pkg/staveloom_wasm.js",
  "./pkg/staveloom_wasm_bg.wasm",
  "./lib/spessasynth_lib.min.js",
  "./lib/spessasynth_processor.min.js",
  "./soundfonts/index.json",
  "./fonts/Bravura-subset.woff2",
];

// ── Install ───────────────────────────────────────────────────────────────────
self.addEventListener("install", (event) => {
  event.waitUntil(
    caches
      .open(CACHE_NAME)
      .then((cache) => {
        // Add assets individually so one failure doesn't abort the whole install.
        // cache: "reload" bypasses the browser's HTTP cache — otherwise a new
        // CACHE_VERSION can still precache stale copies (e.g. an old WASM build).
        return Promise.allSettled(
          STATIC_ASSETS.map((url) =>
            cache
              .add(new Request(url, { cache: "reload" }))
              .catch((err) =>
                console.warn(`[SW] precache miss: ${url} —`, err.message),
              ),
          ),
        );
      })
      .then(() => self.skipWaiting()),
  );
});

// ── Activate ──────────────────────────────────────────────────────────────────
self.addEventListener("activate", (event) => {
  event.waitUntil(
    caches
      .keys()
      .then((names) =>
        Promise.all(
          names
            .filter((n) => n !== CACHE_NAME)
            .map((n) => {
              console.log(`[SW] deleting stale cache: ${n}`);
              return caches.delete(n);
            }),
        ),
      )
      .then(() => self.clients.claim()),
  );
});

// ── Fetch ─────────────────────────────────────────────────────────────────────
self.addEventListener("fetch", (event) => {
  const { request } = event;

  // Only intercept GET requests from the same origin
  if (request.method !== "GET") return;
  const url = new URL(request.url);
  if (url.origin !== self.location.origin) return;

  event.respondWith(handleFetch(request));
});

async function handleFetch(request) {
  const cache = await caches.open(CACHE_NAME);

  // The page itself is network-first: it carries the device detection and the
  // asset versions, so a cached copy would keep serving the previous release
  // until a second reload. The cache is only its offline fallback.
  if (request.mode === "navigate") {
    try {
      const response = await fetch(request);
      if (response.ok) cache.put(request, response.clone());
      return response;
    } catch (_err) {
      const fallback =
        (await cache.match(request, { ignoreSearch: true })) ||
        (await cache.match("./index.html"));
      if (fallback) return fallback;
      return new Response("Offline — resource not cached", {
        status: 503,
        headers: { "Content-Type": "text/plain" },
      });
    }
  }

  // Cache-first: ignoreSearch lets ?v=N versioned URLs hit the unversioned cache entry
  const cached = await cache.match(request, { ignoreSearch: true });
  if (cached) return cached;

  try {
    const response = await fetch(request);
    if (response.ok) {
      // Store under the original request URL so future hits also find it
      cache.put(request, response.clone());
    }
    return response;
  } catch (_err) {
    // Offline fallback for navigation requests
    if (request.mode === "navigate") {
      const fallback = await cache.match("./index.html");
      if (fallback) return fallback;
    }
    return new Response("Offline — resource not cached", {
      status: 503,
      headers: { "Content-Type": "text/plain" },
    });
  }
}
