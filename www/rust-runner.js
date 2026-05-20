const DONE_PREFIX = 'GORS_DONE:';
const BOOT_READY_MARKER = 'GORS_BOOT_READY';
const READY_MARKER = 'GORS_READY';

const IDB_NAME = 'gors-vm';
const IDB_STORE = 'state';

export const State = Object.freeze({
  INITIALIZING: 'initializing',
  DOWNLOADING: 'downloading',
  BOOTING: 'booting',
  READY: 'ready',
  COMPILING: 'compiling',
  ERROR: 'error',
});

function openIDB() {
  return new Promise((resolve, reject) => {
    const req = indexedDB.open(IDB_NAME, 1);
    req.onupgradeneeded = () => req.result.createObjectStore(IDB_STORE);
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
}

async function idbGet(key) {
  const db = await openIDB();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(IDB_STORE, 'readonly');
    const req = tx.objectStore(IDB_STORE).get(key);
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
}

async function idbSet(key, value) {
  const db = await openIDB();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(IDB_STORE, 'readwrite');
    tx.objectStore(IDB_STORE).put(value, key);
    tx.oncomplete = () => resolve();
    tx.onerror = () => reject(tx.error);
  });
}

async function hashString(str) {
  const data = new TextEncoder().encode(str);
  const buf = await crypto.subtle.digest('SHA-256', data);
  return Array.from(new Uint8Array(buf)).map((b) => b.toString(16).padStart(2, '0')).join('').slice(0, 16);
}

// eslint-disable-next-line no-control-regex
const ANSI_RE = /\x1b\[[0-9;]*m/g;

let nextJobId = 1;

export class RustRunner {
  constructor() {
    this._emulator = null;
    this._state = State.INITIALIZING;
    this._stateListeners = [];
    this._serialBuffer = '';
    this._serialByteListeners = [];
    this._markerResolve = null;
    this._markerTarget = null;
    this._currentJobId = null;
  }

  get state() { return this._state; }

  onStateChange(fn) {
    this._stateListeners.push(fn);
    return () => { this._stateListeners = this._stateListeners.filter((l) => l !== fn); };
  }

  onSerialByte(fn) {
    this._serialByteListeners.push(fn);
  }

  _setState(s) {
    this._state = s;
    for (const fn of this._stateListeners) fn(s);
  }

  _assetUrl(name) {
    const hashed = this._assetManifest[name];
    return new URL(`assets/${hashed || name}`, window.location.href).href;
  }

  async start() {
    this._setState(State.DOWNLOADING);
    this._assetManifest = {};

    const rootfsUrl = new URL('assets/rootfs.json', window.location.href).href;

    const [assetManifestResp, rootfsResp] = await Promise.all([
      fetch(new URL('assets/asset-manifest.json', window.location.href).href),
      fetch(rootfsUrl),
    ]);

    if (assetManifestResp.ok) {
      this._assetManifest = await assetManifestResp.json();
    }

    let rootfsText = '';
    if (rootfsResp.ok) {
      rootfsText = await rootfsResp.text();
    }

    await new Promise((resolve, reject) => {
      const script = document.createElement('script');
      script.src = this._assetUrl('libv86.js');
      script.onload = resolve;
      script.onerror = reject;
      document.head.appendChild(script);
    });

    const stateVersion = await hashString(rootfsText);
    let savedState = null;
    try {
      const saved = await idbGet('vm-state');
      if (saved && saved.version === stateVersion) {
        savedState = saved.state;
      }
    } catch { /* ignore */ }

    this._setState(State.BOOTING);

    this._emulator = new V86({
      wasm_path: this._assetUrl('v86.wasm'),
      bios: { url: this._assetUrl('seabios.bin') },
      vga_bios: { url: this._assetUrl('vgabios.bin') },
      autostart: true,
      memory_size: 256 * 1024 * 1024,
      vga_memory_size: 2 * 1024 * 1024,
      disable_keyboard: true,
      disable_mouse: true,
      filesystem: {
        baseurl: new URL('assets/rootfs-flat/', window.location.href).href,
        basefs: rootfsUrl,
      },
      bzimage_initrd_from_filesystem: true,
      cmdline: 'rw root=host9p rootfstype=9p rootflags=trans=virtio,cache=loose modules=virtio_pci tsc=reliable console=ttyS0 quiet',
      initial_state: savedState ? { buffer: savedState } : undefined,
    });

    this._emulator.add_listener('serial0-output-byte', (byte) => {
      for (const fn of this._serialByteListeners) fn(byte);
      this._serialBuffer += String.fromCharCode(byte);
      this._checkMarker();
    });

    if (savedState) {
      this._serialBuffer = '';
      this._sendCommand(`echo "${READY_MARKER}"`);
      await this._waitForMarker(READY_MARKER);
      this._serialBuffer = '';
    } else {
      await this._waitForBoot();
      try {
        const state = await this._emulator.save_state();
        await idbSet('vm-state', { version: stateVersion, state });
      } catch { /* ignore */ }
    }

    this._setState(State.READY);
  }

  _waitForBoot() {
    return this._waitForMarker(BOOT_READY_MARKER).then(() => {
      this._serialBuffer = '';
      this._sendCommand(`export PATH="/usr/local/bin:$PATH"; echo "${READY_MARKER}"`);
      return this._waitForMarker(READY_MARKER);
    }).then(() => {
      this._serialBuffer = '';
    });
  }

  _sendCommand(cmd) {
    if (this._emulator) this._emulator.serial0_send(cmd + '\n');
  }

  _waitForMarker(marker) {
    return new Promise((resolve) => {
      this._markerTarget = marker;
      this._markerResolve = resolve;
      this._checkMarker();
    });
  }

  _checkMarker() {
    if (!this._markerResolve || !this._markerTarget) return;
    const idx = this._serialBuffer.indexOf(this._markerTarget);
    if (idx === -1) return;
    const before = this._serialBuffer.substring(0, idx);
    this._serialBuffer = this._serialBuffer.substring(idx + this._markerTarget.length);
    const resolve = this._markerResolve;
    this._markerResolve = null;
    this._markerTarget = null;
    resolve(before);
  }

  async _readFile(path) {
    try {
      const bytes = await this._emulator.read_file(path);
      return new TextDecoder().decode(bytes);
    } catch {
      return '';
    }
  }

  async compile(rustSource) {
    const jobId = String(nextJobId++);
    this._currentJobId = jobId;

    if (this._state !== State.READY && this._state !== State.COMPILING) {
      return { success: false, output: '', errors: `VM not ready (${this._state})` };
    }

    this._setState(State.COMPILING);

    // Write source via 9p
    await this._emulator.create_file(`tmp/${jobId}.rs`, new TextEncoder().encode(rustSource));

    // Run compile-run, wait for done marker with status
    this._serialBuffer = '';
    this._sendCommand(`compile-run ${jobId}`);

    // Marker format: GORS_DONE:<id>:compile_error or GORS_DONE:<id>:ok:<exit_code>
    await this._waitForMarker(DONE_PREFIX + jobId + ':');
    // tail now contains everything after "GORS_DONE:<id>:"
    // Read until newline to get the status
    const nlIdx = this._serialBuffer.indexOf('\n');
    const statusLine = nlIdx !== -1 ? this._serialBuffer.substring(0, nlIdx).trim() : this._serialBuffer.trim();
    this._serialBuffer = nlIdx !== -1 ? this._serialBuffer.substring(nlIdx + 1) : '';

    if (this._currentJobId !== jobId) {
      this._setState(State.READY);
      return { success: false, output: '', errors: 'cancelled' };
    }

    this._setState(State.READY);

    // Read output files via 9p (raw text, no JSON escaping needed)
    if (statusLine === 'compile_error' || statusLine.startsWith('compile_error')) {
      const errors = (await this._readFile(`tmp/${jobId}.err`)).replace(ANSI_RE, '');
      return { success: false, output: '', errors: errors || 'compilation failed' };
    }

    const exitCode = parseInt(statusLine.replace(/^ok:?/, ''), 10) || 0;
    const stdout = await this._readFile(`tmp/${jobId}.out`);
    const stderr = await this._readFile(`tmp/${jobId}.err`);

    if (exitCode !== 0) {
      return { success: true, output: stdout.trim(), errors: (stderr || `program exited with code ${exitCode}`).trim() };
    }

    return { success: true, output: stdout.trim(), errors: stderr.trim() };
  }
}
