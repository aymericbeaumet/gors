const COMPILE_DONE = 'GORS_COMPILE_DONE:';
const RUN_DONE = 'GORS_RUN_DONE:';
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
  RUNNING: 'running',
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
      memory_size: 512 * 1024 * 1024,
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
    this._serialBuffer = this._serialBuffer.substring(idx + this._markerTarget.length);
    const resolve = this._markerResolve;
    this._markerResolve = null;
    this._markerTarget = null;
    resolve();
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

    if (this._state !== State.READY && this._state !== State.COMPILING && this._state !== State.RUNNING) {
      return { cancelled: false, compile: { success: false, stderr: `VM not ready (${this._state})` } };
    }

    this._setState(State.COMPILING);

    await this._emulator.create_file(`tmp/${jobId}.rs`, new TextEncoder().encode(rustSource));
    this._serialBuffer = '';
    this._sendCommand(`gors-compile ${jobId}`);
    await this._waitForMarker(COMPILE_DONE + jobId);

    if (this._currentJobId !== jobId) {
      this._setState(State.READY);
      return { cancelled: true, compile: null };
    }

    const compileStatus = (await this._readFile(`tmp/${jobId}.compile.status`)).trim();
    const compileStderr = (await this._readFile(`tmp/${jobId}.compile.err`)).replace(ANSI_RE, '');

    this._setState(State.READY);
    return {
      cancelled: false,
      jobId,
      compile: {
        success: compileStatus === '0',
        stderr: compileStderr.trim() || (compileStatus !== '0' ? 'compilation failed' : ''),
      },
    };
  }

  async runJob(jobId) {
    this._currentJobId = jobId;

    if (this._state !== State.READY && this._state !== State.COMPILING && this._state !== State.RUNNING) {
      return { cancelled: false, run: null };
    }

    this._setState(State.RUNNING);

    this._serialBuffer = '';
    this._sendCommand(`gors-run ${jobId}`);
    await this._waitForMarker(RUN_DONE + jobId);

    if (this._currentJobId !== jobId) {
      this._setState(State.READY);
      return { cancelled: true, run: null };
    }

    this._setState(State.READY);

    const exitCode = parseInt((await this._readFile(`tmp/${jobId}.run.status`)).trim(), 10) || 0;
    const stdout = await this._readFile(`tmp/${jobId}.run.out`);
    const runStderr = await this._readFile(`tmp/${jobId}.run.err`);

    return {
      cancelled: false,
      run: { exitCode, stdout: stdout.trim(), stderr: runStderr.trim() },
    };
  }

  async run(rustSource) {
    const compileResult = await this.compile(rustSource);
    if (compileResult.cancelled || !compileResult.compile.success) {
      return { ...compileResult, run: null };
    }

    const runResult = await this.runJob(compileResult.jobId);
    return { ...runResult, compile: compileResult.compile };
  }
}
