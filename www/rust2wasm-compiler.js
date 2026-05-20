const EXIT_MARKER = 'GORS_EXIT:';
const WASM_END_MARKER = 'GORS_WASM_END';
const READY_MARKER = 'GORS_READY';

export const State = Object.freeze({
  INITIALIZING: 'initializing',
  DOWNLOADING: 'downloading',
  BOOTING: 'booting',
  READY: 'ready',
  COMPILING: 'compiling',
  ERROR: 'error',
});

export class Rust2WasmCompiler {
  constructor() {
    this._emulator = null;
    this._state = State.INITIALIZING;
    this._progress = 0;
    this._stateListeners = [];
    this._serialBuffer = '';
    this._serialLog = [];
    this._pendingCompile = null;
    this._compilationId = 0;
  }

  get state() {
    return this._state;
  }

  get progress() {
    return this._progress;
  }

  get serialLog() {
    return this._serialLog;
  }

  onStateChange(fn) {
    this._stateListeners.push(fn);
    return () => {
      this._stateListeners = this._stateListeners.filter((l) => l !== fn);
    };
  }

  _setState(s, progress) {
    this._state = s;
    this._progress = progress != null ? progress : 0;
    for (const fn of this._stateListeners) fn(s, this._progress);
  }

  _assetUrl(name) {
    const hashed = this._assetManifest[name];
    return new URL(`assets/${hashed || name}`, window.location.href).href;
  }

  async start() {
    this._setState(State.DOWNLOADING, 0);
    this._assetManifest = {};

    const [assetManifestResp, imageManifestResp] = await Promise.all([
      fetch(new URL('assets/asset-manifest.json', window.location.href).href),
      fetch(new URL('assets/manifest.json', window.location.href).href),
    ]);

    if (assetManifestResp.ok) {
      this._assetManifest = await assetManifestResp.json();
    }

    let imageUrl;
    if (imageManifestResp.ok) {
      const manifest = await imageManifestResp.json();
      imageUrl = new URL(`assets/${manifest.image}`, window.location.href).href;
    } else {
      imageUrl = new URL('assets/v86-runrust.img', window.location.href).href;
    }

    this._setState(State.DOWNLOADING, 10);

    // Download disk image with progress tracking
    const imageResp = await fetch(imageUrl);
    const contentLength = parseInt(imageResp.headers.get('content-length') || '0', 10);
    let imageBlob;
    if (contentLength > 0 && imageResp.body) {
      const reader = imageResp.body.getReader();
      const chunks = [];
      let received = 0;
      for (;;) {
        const { done, value } = await reader.read();
        if (done) break;
        chunks.push(value);
        received += value.length;
        this._setState(State.DOWNLOADING, 10 + Math.round((received / contentLength) * 80));
      }
      imageBlob = new Blob(chunks);
    } else {
      imageBlob = await imageResp.blob();
    }
    const imageBlobUrl = URL.createObjectURL(imageBlob);

    this._setState(State.DOWNLOADING, 95);

    await new Promise((resolve, reject) => {
      const script = document.createElement('script');
      script.src = this._assetUrl('libv86.js');
      script.onload = resolve;
      script.onerror = reject;
      document.head.appendChild(script);
    });

    this._setState(State.BOOTING, 0);
    this._bootLineCount = 0;

    this._emulator = new V86({
      wasm_path: this._assetUrl('v86.wasm'),
      bios: { url: this._assetUrl('seabios.bin') },
      vga_bios: { url: this._assetUrl('vgabios.bin') },
      hda: { url: imageBlobUrl, async: true },
      autostart: true,
      memory_size: 256 * 1024 * 1024,
      vga_memory_size: 2 * 1024 * 1024,
      disable_keyboard: true,
      disable_mouse: true,
      screen_dummy: true,
      serial_container_xtermjs: null,
    });

    this._emulator.add_listener('serial0-output-byte', (byte) => {
      const ch = String.fromCharCode(byte);
      this._serialBuffer += ch;
      if (ch === '\n') {
        this._serialLog.push(this._serialBuffer);
        if (this._serialLog.length > 500) this._serialLog.shift();
        if (this._state === State.BOOTING) {
          this._bootLineCount++;
          const pct = Math.min(95, Math.round((this._bootLineCount / 120) * 100));
          this._setState(State.BOOTING, pct);
        }
      }
      this._processSerialBuffer();
    });

    await this._waitForBoot();
    this._setState(State.READY);
  }

  _waitForBoot() {
    return new Promise((resolve) => {
      const check = setInterval(() => {
        if (
          this._serialBuffer.includes('login:') ||
          this._serialBuffer.includes('# ') ||
          this._serialBuffer.includes('/ #')
        ) {
          clearInterval(check);
          this._serialBuffer = '';
          this._sendCommand(`echo "${READY_MARKER}"`);
          this._waitForMarker(READY_MARKER).then(() => {
            this._serialBuffer = '';
            resolve();
          });
        }
      }, 500);
    });
  }

  _sendCommand(cmd) {
    if (!this._emulator) return;
    for (const c of cmd) this._emulator.serial_send(c);
    this._emulator.serial_send('\n');
  }

  _waitForMarker(marker) {
    return new Promise((resolve) => {
      const tick = () => {
        const idx = this._serialBuffer.indexOf(marker);
        if (idx !== -1) {
          const before = this._serialBuffer.substring(0, idx);
          this._serialBuffer = this._serialBuffer.substring(idx + marker.length);
          resolve(before);
          return;
        }
        setTimeout(tick, 50);
      };
      tick();
    });
  }

  _processSerialBuffer() {
    if (!this._pendingCompile) return;

    const exitIdx = this._serialBuffer.indexOf(EXIT_MARKER);
    if (exitIdx === -1) return;
    const endIdx = this._serialBuffer.indexOf('\n', exitIdx);
    if (endIdx === -1) return;

    const exitLine = this._serialBuffer.substring(exitIdx, endIdx);
    const exitCode = parseInt(exitLine.substring(EXIT_MARKER.length), 10);
    const stderr = this._serialBuffer.substring(0, exitIdx).trim();

    if (exitCode !== 0) {
      const { resolve } = this._pendingCompile;
      this._pendingCompile = null;
      this._serialBuffer = '';
      resolve({ success: false, wasmBytes: null, errors: stderr || `rustc exited with code ${exitCode}` });
      return;
    }

    this._serialBuffer = this._serialBuffer.substring(endIdx + 1);
    this._sendCommand('base64 /tmp/out.wasm; echo "' + WASM_END_MARKER + '"');

    this._waitForMarker(WASM_END_MARKER).then((b64) => {
      const { resolve } = this._pendingCompile;
      this._pendingCompile = null;

      const cleaned = b64.replace(/\s/g, '');
      const binary = atob(cleaned);
      const bytes = new Uint8Array(binary.length);
      for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
      resolve({ success: true, wasmBytes: bytes, errors: '' });
    });
  }

  compile(rustSource) {
    const id = ++this._compilationId;

    if (this._pendingCompile) {
      this._pendingCompile.resolve({ success: false, wasmBytes: null, errors: 'cancelled' });
      this._pendingCompile = null;
      this._serialBuffer = '';
    }

    if (this._state !== State.READY && this._state !== State.COMPILING) {
      return Promise.resolve({ success: false, wasmBytes: null, errors: `Compiler not ready (${this._state})` });
    }

    this._setState(State.COMPILING);

    const encoded = btoa(unescape(encodeURIComponent(rustSource)));

    return new Promise((resolve) => {
      this._pendingCompile = { resolve, id };
      this._sendCommand(
        `echo '${encoded}' | base64 -d > /tmp/main.rs && compile-wasm /tmp/main.rs; echo "${EXIT_MARKER}$?"`
      );
    }).then((result) => {
      if (id === this._compilationId && this._state === State.COMPILING) {
        this._setState(State.READY);
      }
      if (id !== this._compilationId) {
        return { success: false, wasmBytes: null, errors: 'cancelled' };
      }
      return result;
    });
  }
}
