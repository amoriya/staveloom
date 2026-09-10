/**
 * SoundFontLoader — on-demand SF2 loading with IndexedDB persistence.
 *
 * Strategy:
 *   1. Memory cache (Map): fastest, in-process lifetime.
 *   2. IndexedDB: persists across page loads, survives browser restart.
 *   3. Network fetch: last resort, fetches from soundfonts/instruments/.
 *
 * The loader deduplicates by *filename* so that multiple programs mapping
 * to the same SF2 (e.g. full_gm.sf2) only trigger one fetch.
 */
class SoundFontLoader {
  constructor() {
    this._memCache = new Map(); // filename -> ArrayBuffer
    this._db = null;
    this._index = null;
    this._dbName = "staveloom-soundfonts-v1";
    this._storeName = "sf2-cache";
  }

  async init() {
    const [index, db] = await Promise.all([
      fetch("./soundfonts/index.json").then((r) => {
        if (!r.ok)
          throw new Error(`Failed to load soundfonts/index.json: ${r.status}`);
        return r.json();
      }),
      this._openDB(),
    ]);
    this._index = index;
    this._db = db;
  }

  /**
   * Load SF2 data for a list of MIDI program numbers.
   * Returns [{ program, data: ArrayBuffer }]
   * Deduplicates: multiple programs mapping to the same file are returned
   * with the same ArrayBuffer reference.
   */
  async loadForPrograms(programs) {
    const unique = [...new Set(programs)];
    const fileToBuffer = new Map();

    for (const prog of unique) {
      const info = this._index.instruments[String(prog)] || {
        file: this._index.fallback || "full_gm.sf2",
      };
      const filename = info.file;

      if (!fileToBuffer.has(filename)) {
        const buf = await this._loadFile(filename);
        fileToBuffer.set(filename, buf);
      }
    }

    return unique.map((prog) => {
      const info = this._index.instruments[String(prog)] || {
        file: this._index.fallback || "full_gm.sf2",
      };
      return { program: prog, data: fileToBuffer.get(info.file) };
    });
  }

  async _loadFile(filename) {
    if (this._memCache.has(filename)) {
      return this._memCache.get(filename);
    }

    const cached = await this._loadFromDB(filename);
    if (cached) {
      this._memCache.set(filename, cached);
      return cached;
    }

    const url = `./soundfonts/instruments/${filename}`;
    const resp = await fetch(url);
    if (!resp.ok) {
      const fallback = this._index.fallback || "full_gm.sf2";
      if (filename !== fallback) {
        console.warn(`SF2 fetch failed for ${filename}, trying fallback`);
        return this._loadFile(fallback);
      }
      throw new Error(`Failed to fetch soundfont: ${url}`);
    }

    const buf = await resp.arrayBuffer();
    this._memCache.set(filename, buf);
    this._saveToDB(filename, buf).catch((e) =>
      console.warn("IndexedDB save failed:", e),
    );
    return buf;
  }

  _openDB() {
    return new Promise((resolve, reject) => {
      const req = indexedDB.open(this._dbName, 1);
      req.onupgradeneeded = (e) => {
        const db = e.target.result;
        if (!db.objectStoreNames.contains(this._storeName)) {
          db.createObjectStore(this._storeName);
        }
      };
      req.onsuccess = (e) => resolve(e.target.result);
      req.onerror = () => resolve(null); // IDB failure is non-fatal
    });
  }

  _loadFromDB(filename) {
    return new Promise((resolve) => {
      if (!this._db) return resolve(null);
      try {
        const tx = this._db.transaction(this._storeName, "readonly");
        const store = tx.objectStore(this._storeName);
        const req = store.get(filename);
        req.onsuccess = () => resolve(req.result || null);
        req.onerror = () => resolve(null);
      } catch {
        resolve(null);
      }
    });
  }

  _saveToDB(filename, buffer) {
    return new Promise((resolve, reject) => {
      if (!this._db) return resolve();
      try {
        const tx = this._db.transaction(this._storeName, "readwrite");
        const store = tx.objectStore(this._storeName);
        const req = store.put(buffer, filename);
        req.onsuccess = () => resolve();
        req.onerror = () => reject(req.error);
      } catch (e) {
        reject(e);
      }
    });
  }

  /**
   * Clear all cached SF2 data from IndexedDB (for debugging / cache reset).
   */
  async clearCache() {
    this._memCache.clear();
    if (!this._db) return;
    const tx = this._db.transaction(this._storeName, "readwrite");
    tx.objectStore(this._storeName).clear();
    return new Promise((r) => {
      tx.oncomplete = r;
    });
  }
}
