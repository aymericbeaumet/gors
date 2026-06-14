const COMPILE_DONE = "GORS_COMPILE_DONE:";
const RUN_DONE = "GORS_RUN_DONE:";
const BOOT_READY_MARKER = "GORS_BOOT_READY";
const READY_MARKER = "GORS_READY";

const IDB_NAME = "gors-vm";
const IDB_STORE = "state";

export const State = Object.freeze({
	INITIALIZING: "initializing",
	DOWNLOADING: "downloading",
	BOOTING: "booting",
	READY: "ready",
	COMPILING: "compiling",
	RUNNING: "running",
	ERROR: "error",
});

export type State = (typeof State)[keyof typeof State];

interface SavedVmState {
	version: string;
	state: ArrayBuffer;
}

interface CompileOutput {
	success: boolean;
	stderr: string;
}

interface CompileCompleted {
	cancelled: false;
	jobId: string;
	compile: CompileOutput;
}

interface CompileFailedBeforeStart {
	cancelled: false;
	compile: CompileOutput;
	jobId?: undefined;
}

interface CancelledCompile {
	cancelled: true;
	compile: null;
	jobId?: undefined;
}

type CompileResult =
	| CompileCompleted
	| CompileFailedBeforeStart
	| CancelledCompile;

interface RunOutput {
	exitCode: number;
	stdout: string;
	stderr: string;
}

type RunJobResult =
	| {
			cancelled: false;
			run: RunOutput | null;
	  }
	| {
			cancelled: true;
			run: null;
	  };

type StateListener = (state: State) => void;
type SerialByteListener = (byte: number) => void;

function openIDB(): Promise<IDBDatabase> {
	return new Promise((resolve, reject) => {
		const req = indexedDB.open(IDB_NAME, 1);
		req.onupgradeneeded = () => req.result.createObjectStore(IDB_STORE);
		req.onsuccess = () => resolve(req.result);
		req.onerror = () => reject(req.error);
	});
}

async function idbGet(key: string): Promise<unknown> {
	const db = await openIDB();
	return new Promise((resolve, reject) => {
		const tx = db.transaction(IDB_STORE, "readonly");
		const req = tx.objectStore(IDB_STORE).get(key);
		req.onsuccess = () => resolve(req.result);
		req.onerror = () => reject(req.error);
	});
}

async function idbSet(key: string, value: unknown): Promise<void> {
	const db = await openIDB();
	return new Promise((resolve, reject) => {
		const tx = db.transaction(IDB_STORE, "readwrite");
		tx.objectStore(IDB_STORE).put(value, key);
		tx.oncomplete = () => resolve();
		tx.onerror = () => reject(tx.error);
	});
}

async function hashString(value: string): Promise<string> {
	const data = new TextEncoder().encode(value);
	const buf = await crypto.subtle.digest("SHA-256", data);
	return Array.from(new Uint8Array(buf))
		.map((byte) => byte.toString(16).padStart(2, "0"))
		.join("")
		.slice(0, 16);
}

function isSavedVmState(value: unknown): value is SavedVmState {
	return (
		typeof value === "object" &&
		value !== null &&
		"version" in value &&
		"state" in value &&
		typeof value.version === "string" &&
		value.state instanceof ArrayBuffer
	);
}

// eslint-disable-next-line no-control-regex
const ANSI_RE = /\x1b\[[0-9;]*m/g;

let nextJobId = 1;

export class RustRunner {
	private emulator: V86Emulator | null = null;
	private currentState: State = State.INITIALIZING;
	private stateListeners: StateListener[] = [];
	private serialBuffer = "";
	private serialByteListeners: SerialByteListener[] = [];
	private markerResolve: (() => void) | null = null;
	private markerTarget: string | null = null;
	private currentJobId: string | null = null;
	private assetManifest: Record<string, string> = {};

	get state(): State {
		return this.currentState;
	}

	onStateChange(fn: StateListener): () => void {
		this.stateListeners.push(fn);
		return () => {
			this.stateListeners = this.stateListeners.filter(
				(listener) => listener !== fn,
			);
		};
	}

	onSerialByte(fn: SerialByteListener): void {
		this.serialByteListeners.push(fn);
	}

	sendSerial(data: string): void {
		this.emulator?.serial0_send(data);
	}

	private setState(state: State): void {
		this.currentState = state;
		for (const fn of this.stateListeners) fn(state);
	}

	private assetUrl(name: string): string {
		const hashed = this.assetManifest[name];
		return new URL(`assets/${hashed || name}`, window.location.href).href;
	}

	async start(): Promise<void> {
		this.setState(State.DOWNLOADING);
		this.assetManifest = {};

		const rootfsUrl = new URL("assets/rootfs.json", window.location.href).href;

		const [assetManifestResp, rootfsResp] = await Promise.all([
			fetch(new URL("assets/asset-manifest.json", window.location.href).href),
			fetch(rootfsUrl),
		]);

		if (assetManifestResp.ok) {
			this.assetManifest = await assetManifestResp.json();
		}

		let rootfsText = "";
		if (rootfsResp.ok) {
			rootfsText = await rootfsResp.text();
		}

		await new Promise<void>((resolve, reject) => {
			const script = document.createElement("script");
			script.src = this.assetUrl("libv86.js");
			script.onload = () => resolve();
			script.onerror = () => reject(new Error("failed to load libv86.js"));
			document.head.appendChild(script);
		});

		const stateVersion = await hashString(rootfsText);
		let savedState: ArrayBuffer | null = null;
		try {
			const saved = await idbGet("vm-state");
			if (isSavedVmState(saved) && saved.version === stateVersion) {
				savedState = saved.state;
			}
		} catch {
			/* ignore */
		}

		this.setState(State.BOOTING);

		this.emulator = new V86({
			wasm_path: this.assetUrl("v86.wasm"),
			bios: { url: this.assetUrl("seabios.bin") },
			vga_bios: { url: this.assetUrl("vgabios.bin") },
			autostart: true,
			memory_size: 512 * 1024 * 1024,
			vga_memory_size: 2 * 1024 * 1024,
			disable_keyboard: true,
			disable_mouse: true,
			filesystem: {
				baseurl: new URL("assets/rootfs-flat/", window.location.href).href,
				basefs: rootfsUrl,
			},
			bzimage_initrd_from_filesystem: true,
			cmdline:
				"rw root=host9p rootfstype=9p rootflags=trans=virtio,cache=loose modules=virtio_pci tsc=reliable console=ttyS0 quiet",
			initial_state: savedState ? { buffer: savedState } : undefined,
		});

		this.emulator.add_listener("serial0-output-byte", (byte) => {
			for (const fn of this.serialByteListeners) fn(byte);
			this.serialBuffer += String.fromCharCode(byte);
			this.checkMarker();
		});

		if (savedState) {
			this.serialBuffer = "";
			this.sendCommand(`echo "${READY_MARKER}"`);
			await this.waitForMarker(READY_MARKER);
			this.serialBuffer = "";
		} else {
			await this.waitForBoot();
			try {
				const state = await this.emulator.save_state();
				await idbSet("vm-state", { version: stateVersion, state });
			} catch {
				/* ignore */
			}
		}

		this.setState(State.READY);
	}

	private waitForBoot(): Promise<void> {
		return this.waitForMarker(BOOT_READY_MARKER)
			.then(() => {
				this.serialBuffer = "";
				this.sendCommand(
					`export PATH="/usr/local/bin:$PATH"; echo "${READY_MARKER}"`,
				);
				return this.waitForMarker(READY_MARKER);
			})
			.then(() => {
				this.serialBuffer = "";
			});
	}

	private sendCommand(cmd: string): void {
		this.emulator?.serial0_send(`${cmd}\n`);
	}

	private waitForMarker(marker: string): Promise<void> {
		return new Promise((resolve) => {
			this.markerTarget = marker;
			this.markerResolve = resolve;
			this.checkMarker();
		});
	}

	private checkMarker(): void {
		if (!this.markerResolve || !this.markerTarget) return;
		const index = this.serialBuffer.indexOf(this.markerTarget);
		if (index === -1) return;
		this.serialBuffer = this.serialBuffer.substring(
			index + this.markerTarget.length,
		);
		const resolve = this.markerResolve;
		this.markerResolve = null;
		this.markerTarget = null;
		resolve();
	}

	private async readFile(path: string): Promise<string> {
		try {
			if (!this.emulator) return "";
			const bytes = await this.emulator.read_file(path);
			return new TextDecoder().decode(bytes);
		} catch {
			return "";
		}
	}

	async compile(rustSource: string): Promise<CompileResult> {
		const jobId = String(nextJobId++);
		this.currentJobId = jobId;

		if (
			this.currentState !== State.READY &&
			this.currentState !== State.COMPILING &&
			this.currentState !== State.RUNNING
		) {
			return {
				cancelled: false,
				compile: {
					success: false,
					stderr: `VM not ready (${this.currentState})`,
				},
			};
		}

		if (!this.emulator) {
			return {
				cancelled: false,
				compile: { success: false, stderr: "VM not initialized" },
			};
		}

		this.setState(State.COMPILING);

		await this.emulator.create_file(
			`tmp/${jobId}.rs`,
			new TextEncoder().encode(rustSource),
		);
		this.serialBuffer = "";
		this.sendCommand(`gors-compile ${jobId}`);
		await this.waitForMarker(COMPILE_DONE + jobId);

		if (this.currentJobId !== jobId) {
			this.setState(State.READY);
			return { cancelled: true, compile: null };
		}

		const compileStatus = (
			await this.readFile(`tmp/${jobId}.compile.status`)
		).trim();
		const compileStderr = (
			await this.readFile(`tmp/${jobId}.compile.err`)
		).replace(ANSI_RE, "");

		this.setState(State.READY);
		return {
			cancelled: false,
			jobId,
			compile: {
				success: compileStatus === "0",
				stderr:
					compileStderr.trim() ||
					(compileStatus !== "0" ? "compilation failed" : ""),
			},
		};
	}

	async runJob(jobId: string): Promise<RunJobResult> {
		this.currentJobId = jobId;

		if (
			this.currentState !== State.READY &&
			this.currentState !== State.COMPILING &&
			this.currentState !== State.RUNNING
		) {
			return { cancelled: false, run: null };
		}

		this.setState(State.RUNNING);

		this.serialBuffer = "";
		this.sendCommand(`gors-run ${jobId}`);
		await this.waitForMarker(RUN_DONE + jobId);

		if (this.currentJobId !== jobId) {
			this.setState(State.READY);
			return { cancelled: true, run: null };
		}

		this.setState(State.READY);

		const exitCode =
			Number.parseInt(
				(await this.readFile(`tmp/${jobId}.run.status`)).trim(),
				10,
			) || 0;
		const stdout = await this.readFile(`tmp/${jobId}.run.out`);
		const runStderr = await this.readFile(`tmp/${jobId}.run.err`);

		return {
			cancelled: false,
			run: { exitCode, stdout: stdout.trim(), stderr: runStderr.trim() },
		};
	}

	async run(
		rustSource: string,
	): Promise<RunJobResult & { compile: CompileOutput | null }> {
		const compileResult = await this.compile(rustSource);
		if (
			compileResult.cancelled ||
			!compileResult.compile.success ||
			typeof compileResult.jobId !== "string"
		) {
			return { ...compileResult, run: null };
		}

		const runResult = await this.runJob(compileResult.jobId);
		return { ...runResult, compile: compileResult.compile };
	}
}
