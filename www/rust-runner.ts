import type { RuntimeDependency } from "./runtime-dependency";
import {
	RustRunnerDisposedError,
	RustRunnerTimeoutError,
	V86JobCoordinator,
	type CompileFailedBeforeStart,
	type CompileOutput,
	type CompileResult,
	type RunJobResult,
	type V86ExecutionPhase,
} from "./v86-job-coordinator";

export {
	RustRunnerBusyError,
	RustRunnerCancelledError,
	RustRunnerDisposedError,
	RustRunnerProtocolError,
	RustRunnerTimeoutError,
} from "./v86-job-coordinator";
export type {
	CompileOutput,
	CompileResult,
	RunJobResult,
	RunOutput,
} from "./v86-job-coordinator";

const BOOT_READY_MARKER = "GORS_BOOT_READY";
const READY_MARKER = "GORS_READY";
const DEFAULT_BOOT_TIMEOUT_MS = 8 * 60 * 1000;
const DEFAULT_COMPILE_TIMEOUT_MS = 8 * 60 * 1000;
const DEFAULT_RUN_TIMEOUT_MS = 2 * 60 * 1000;

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

export interface RustRunnerOptions {
	bootTimeoutMs?: number;
	compileTimeoutMs?: number;
	runTimeoutMs?: number;
	nonceFactory?: () => string;
}

interface SavedVmState {
	version: string;
	state: ArrayBuffer;
}

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

function validateTimeout(name: string, timeoutMs: number): number {
	if (!Number.isFinite(timeoutMs) || timeoutMs <= 0) {
		throw new Error(`${name} timeout must be positive`);
	}
	return timeoutMs;
}

function abortError(signal: AbortSignal): Error {
	return signal.reason instanceof Error
		? signal.reason
		: new RustRunnerDisposedError();
}

function throwIfAborted(signal: AbortSignal): void {
	if (signal.aborted) throw abortError(signal);
}

function abortable<T>(promise: Promise<T>, signal: AbortSignal): Promise<T> {
	if (signal.aborted) return Promise.reject(abortError(signal));

	return new Promise<T>((resolve, reject) => {
		let settled = false;
		const onAbort = () => {
			if (settled) return;
			settled = true;
			signal.removeEventListener("abort", onAbort);
			reject(abortError(signal));
		};
		const finish = (action: () => void) => {
			if (settled) return;
			settled = true;
			signal.removeEventListener("abort", onAbort);
			action();
		};
		signal.addEventListener("abort", onAbort, { once: true });
		promise.then(
			(value) => finish(() => resolve(value)),
			(error: unknown) =>
				finish(() =>
					reject(error instanceof Error ? error : new Error(String(error))),
				),
		);
	});
}

export class RustRunner {
	private readonly bootTimeoutMs: number;
	private readonly compileTimeoutMs: number;
	private readonly nonceFactory: (() => string) | undefined;
	private readonly runTimeoutMs: number;
	private assetManifest: Record<string, string> = {};
	private coordinator: V86JobCoordinator | null = null;
	private currentState: State = State.INITIALIZING;
	private disposed = false;
	private emulator: V86Emulator | null = null;
	private serialByteListeners: SerialByteListener[] = [];
	private startController: AbortController | null = null;
	private startPromise: Promise<void> | null = null;
	private stateListeners: StateListener[] = [];

	constructor(options: RustRunnerOptions = {}) {
		this.bootTimeoutMs = validateTimeout(
			"boot",
			options.bootTimeoutMs ?? DEFAULT_BOOT_TIMEOUT_MS,
		);
		this.compileTimeoutMs = validateTimeout(
			"compile",
			options.compileTimeoutMs ?? DEFAULT_COMPILE_TIMEOUT_MS,
		);
		this.runTimeoutMs = validateTimeout(
			"run",
			options.runTimeoutMs ?? DEFAULT_RUN_TIMEOUT_MS,
		);
		this.nonceFactory = options.nonceFactory;
	}

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

	onSerialByte(fn: SerialByteListener): () => void {
		this.serialByteListeners.push(fn);
		return () => {
			this.serialByteListeners = this.serialByteListeners.filter(
				(listener) => listener !== fn,
			);
		};
	}

	sendSerial(data: string): void {
		if (!this.disposed) this.emulator?.serial0_send(data);
	}

	cancelActive(reason = "V86 execution cancelled"): void {
		this.coordinator?.cancelActive(reason);
	}

	dispose(): void {
		if (this.disposed) return;
		this.disposed = true;
		this.startController?.abort(new RustRunnerDisposedError());
		this.teardownEmulator();
		this.setState(State.ERROR);
		this.stateListeners = [];
		this.serialByteListeners = [];
	}

	async start(): Promise<void> {
		if (this.disposed) throw new RustRunnerDisposedError();
		if (this.currentState === State.READY) return;
		if (this.startPromise) return this.startPromise;

		const controller = new AbortController();
		this.startController = controller;
		const timeout = setTimeout(() => {
			controller.abort(new RustRunnerTimeoutError("boot", this.bootTimeoutMs));
		}, this.bootTimeoutMs);

		const attempt = this.startAttempt(controller.signal)
			.catch((error: unknown) => {
				this.teardownEmulator();
				if (!this.disposed) this.setState(State.ERROR);
				throw controller.signal.aborted ? abortError(controller.signal) : error;
			})
			.finally(() => {
				clearTimeout(timeout);
				if (this.startController === controller) this.startController = null;
				if (this.startPromise === attempt) this.startPromise = null;
			});
		this.startPromise = attempt;
		return attempt;
	}

	private async startAttempt(signal: AbortSignal): Promise<void> {
		this.setState(State.DOWNLOADING);
		this.assetManifest = {};

		const rootfsUrl = new URL("assets/rootfs.json", window.location.href).href;
		const [assetManifestResp, rootfsResp] = await abortable(
			Promise.all([
				fetch(new URL("assets/asset-manifest.json", window.location.href).href),
				fetch(rootfsUrl),
			]),
			signal,
		);
		throwIfAborted(signal);

		if (assetManifestResp.ok) {
			this.assetManifest = await abortable(assetManifestResp.json(), signal);
		}

		let rootfsText = "";
		if (rootfsResp.ok) {
			rootfsText = await abortable(rootfsResp.text(), signal);
		}

		await this.loadV86Script(signal);
		const stateVersion = await abortable(hashString(rootfsText), signal);
		let savedState: ArrayBuffer | null = null;
		try {
			const saved = await abortable(idbGet("vm-state"), signal);
			if (isSavedVmState(saved) && saved.version === stateVersion) {
				savedState = saved.state;
			}
		} catch {
			throwIfAborted(signal);
		}

		this.setState(State.BOOTING);
		const emulator = new V86({
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
		this.emulator = emulator;
		const coordinator = this.attachCoordinator(emulator);
		throwIfAborted(signal);

		if (savedState) {
			coordinator.resetSerialProtocol();
			const ready = coordinator.waitForSystemMarker(READY_MARKER, signal);
			try {
				this.sendCommand(`echo "${READY_MARKER}"`);
			} catch (error) {
				void ready.catch(() => {});
				throw error;
			}
			await ready;
			coordinator.resetSerialProtocol();
		} else {
			await coordinator.waitForSystemMarker(BOOT_READY_MARKER, signal);
			coordinator.resetSerialProtocol();
			const ready = coordinator.waitForSystemMarker(READY_MARKER, signal);
			try {
				this.sendCommand(
					`export PATH="/usr/local/bin:$PATH"; echo "${READY_MARKER}"`,
				);
			} catch (error) {
				void ready.catch(() => {});
				throw error;
			}
			await ready;
			coordinator.resetSerialProtocol();
			try {
				const state = await abortable(emulator.save_state(), signal);
				await abortable(
					idbSet("vm-state", { version: stateVersion, state }),
					signal,
				);
			} catch {
				throwIfAborted(signal);
			}
		}

		throwIfAborted(signal);
		this.setState(State.READY);
	}

	private attachCoordinator(emulator: V86Emulator): V86JobCoordinator {
		const coordinator = new V86JobCoordinator(emulator, {
			compileTimeoutMs: this.compileTimeoutMs,
			runTimeoutMs: this.runTimeoutMs,
			nonceFactory: this.nonceFactory,
			onPhaseChange: (phase) => this.setExecutionPhase(phase),
		});
		this.coordinator = coordinator;
		emulator.add_listener("serial0-output-byte", (byte) => {
			if (this.coordinator !== coordinator || this.emulator !== emulator)
				return;
			for (const fn of this.serialByteListeners) fn(byte);
			coordinator.acceptSerialByte(byte);
		});
		return coordinator;
	}

	private setExecutionPhase(phase: V86ExecutionPhase | null): void {
		if (this.disposed) return;
		this.setState(
			phase === "compiling"
				? State.COMPILING
				: phase === "running"
					? State.RUNNING
					: State.READY,
		);
	}

	private setState(state: State): void {
		this.currentState = state;
		for (const fn of this.stateListeners) fn(state);
	}

	private assetUrl(name: string): string {
		const hashed = this.assetManifest[name];
		return new URL(`assets/${hashed || name}`, window.location.href).href;
	}

	private loadV86Script(signal: AbortSignal): Promise<void> {
		if (signal.aborted) return Promise.reject(abortError(signal));
		return new Promise<void>((resolve, reject) => {
			const script = document.createElement("script");
			let settled = false;
			const onAbort = () => {
				if (settled) return;
				settled = true;
				signal.removeEventListener("abort", onAbort);
				script.onload = null;
				script.onerror = null;
				script.remove();
				reject(abortError(signal));
			};
			const finish = (action: () => void) => {
				if (settled) return;
				settled = true;
				signal.removeEventListener("abort", onAbort);
				script.onload = null;
				script.onerror = null;
				action();
			};
			script.src = this.assetUrl("libv86.js");
			script.onload = () => finish(resolve);
			script.onerror = () =>
				finish(() => reject(new Error("failed to load libv86.js")));
			signal.addEventListener("abort", onAbort, { once: true });
			document.head.appendChild(script);
		});
	}

	private sendCommand(command: string): void {
		this.emulator?.serial0_send(`${command}\n`);
	}

	private teardownEmulator(): void {
		const coordinator = this.coordinator;
		const emulator = this.emulator;
		this.coordinator = null;
		this.emulator = null;
		coordinator?.dispose();
		emulator?.stop?.();
	}

	private readyCoordinator(
		requestedKind: "compile" | "run" | "compile-and-run",
	): V86JobCoordinator | null {
		if (this.disposed) throw new RustRunnerDisposedError();
		this.coordinator?.assertCanAdmit(requestedKind);
		if (this.currentState !== State.READY) return null;
		return this.coordinator;
	}

	async compile(
		rustSource: string,
		runtimeDependency: RuntimeDependency,
	): Promise<CompileResult> {
		const coordinator = this.readyCoordinator("compile");
		if (!coordinator) return this.notReadyCompileResult();
		return coordinator.compile(rustSource, runtimeDependency);
	}

	async runJob(jobId: string): Promise<RunJobResult> {
		const coordinator = this.readyCoordinator("run");
		if (!coordinator) return { cancelled: false, run: null };
		return coordinator.runJob(jobId);
	}

	async run(
		rustSource: string,
		runtimeDependency: RuntimeDependency,
	): Promise<RunJobResult & { compile: CompileOutput | null }> {
		const coordinator = this.readyCoordinator("compile-and-run");
		if (!coordinator) {
			return { ...this.notReadyCompileResult(), run: null };
		}
		return coordinator.run(rustSource, runtimeDependency);
	}

	private notReadyCompileResult(): CompileFailedBeforeStart {
		return {
			cancelled: false,
			compile: {
				success: false,
				stderr: this.emulator
					? `VM not ready (${this.currentState})`
					: "VM not initialized",
			},
		};
	}
}
