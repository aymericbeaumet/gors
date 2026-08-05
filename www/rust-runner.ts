import type { RuntimeDependency } from "./runtime-dependency";
import type { V86Emulator } from "./src/v86";
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
import {
	V86_BOOT_CONTRACT,
	fetchV86BootManifest,
	v86BootAssetUrl,
	v86RootfsBlobBaseUrl,
	v86RootfsIndexUrl,
	type V86BootManifest,
	type V86ManifestFetcher,
} from "./v86-boot-manifest";
import {
	IndexedDbV86StateStore,
	deleteSavedV86State,
	loadSavedV86State,
	saveV86State,
	type V86StateStore,
} from "./v86-state-cache";

export {
	RustRunnerBusyError,
	RustRunnerCancelledError,
	RustRunnerDisposedError,
	RustRunnerPoisonedError,
	RustRunnerProtocolError,
	RustRunnerTimeoutError,
} from "./v86-job-coordinator";
export type {
	CompileOutput,
	CompileResult,
	RunJobResult,
	RunOutput,
} from "./v86-job-coordinator";

const DEFAULT_BOOT_TIMEOUT_MS = 8 * 60 * 1000;
const DEFAULT_COMPILE_TIMEOUT_MS = 8 * 60 * 1000;
const DEFAULT_RUN_TIMEOUT_MS = 2 * 60 * 1000;

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

export interface RustRunnerDependencies {
	readonly baseUrl: string;
	readonly createEmulator: (options: Record<string, unknown>) => V86Emulator;
	readonly fetcher: V86ManifestFetcher;
	readonly loadScript: (url: string, signal: AbortSignal) => Promise<void>;
	readonly stateStore: V86StateStore;
}

export interface RustRunnerOptions {
	bootTimeoutMs?: number;
	compileTimeoutMs?: number;
	runTimeoutMs?: number;
	nonceFactory?: () => string;
	dependencies?: RustRunnerDependencies;
}

type StateListener = (state: State) => void;
type SerialByteListener = (byte: number) => void;

export class RustRunnerDownloadError extends Error {
	constructor(readonly asset: string) {
		super(`failed to download V86 boot asset ${asset}`);
		this.name = "RustRunnerDownloadError";
	}
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

async function withDeadline<T>(
	parentSignal: AbortSignal,
	timeoutMs: number,
	action: (signal: AbortSignal) => Promise<T>,
): Promise<T> {
	const controller = new AbortController();
	const forwardAbort = () => controller.abort(parentSignal.reason);
	if (parentSignal.aborted) forwardAbort();
	else parentSignal.addEventListener("abort", forwardAbort, { once: true });
	const timeout = setTimeout(() => {
		controller.abort(new RustRunnerTimeoutError("boot", timeoutMs));
	}, timeoutMs);
	try {
		throwIfAborted(controller.signal);
		return await abortable(action(controller.signal), controller.signal);
	} finally {
		clearTimeout(timeout);
		parentSignal.removeEventListener("abort", forwardAbort);
	}
}

function defaultLoadScript(url: string, signal: AbortSignal): Promise<void> {
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
		script.src = url;
		script.onload = () => finish(resolve);
		script.onerror = () =>
			finish(() => reject(new Error(`failed to load V86 script ${url}`)));
		signal.addEventListener("abort", onAbort, { once: true });
		document.head.appendChild(script);
	});
}

export function v86DeploymentRootUrl(currentUrl: string): string {
	return new URL("/", new URL(currentUrl).origin).href;
}

function defaultDependencies(): RustRunnerDependencies {
	return {
		baseUrl: v86DeploymentRootUrl(window.location.href),
		createEmulator: (options) => new V86(options),
		fetcher: (input, init) => fetch(input, init),
		loadScript: defaultLoadScript,
		stateStore: new IndexedDbV86StateStore(),
	};
}

interface DownloadFailureWatch {
	readonly promise: Promise<never>;
	readonly dispose: () => void;
	readonly emulator: V86Emulator;
}

export class RustRunner {
	private readonly bootTimeoutMs: number;
	private readonly compileTimeoutMs: number;
	private readonly dependencies: RustRunnerDependencies;
	private readonly nonceFactory: (() => string) | undefined;
	private readonly runTimeoutMs: number;
	private coordinator: V86JobCoordinator | null = null;
	private currentState: State = State.INITIALIZING;
	private disposed = false;
	private disposePromise: Promise<void> | null = null;
	private downloadFailureWatch: DownloadFailureWatch | null = null;
	private emulator: V86Emulator | null = null;
	private emulatorFailure: Error | null = null;
	private serialByteListeners: SerialByteListener[] = [];
	private startController: AbortController | null = null;
	private startPromise: Promise<void> | null = null;
	private stateListeners: StateListener[] = [];
	private teardownTail: Promise<void> = Promise.resolve();

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
		this.dependencies = options.dependencies ?? defaultDependencies();
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

	dispose(): Promise<void> {
		if (this.disposePromise) return this.disposePromise;
		this.disposed = true;
		this.startController?.abort(new RustRunnerDisposedError());
		this.setState(State.ERROR);
		this.stateListeners = [];
		this.serialByteListeners = [];
		this.disposePromise = this.teardownEmulator().catch(() => {
			// Disposal is terminal. Teardown attempted both stop and destroy, and a
			// failed host cleanup must not become an unhandled browser rejection.
		});
		return this.disposePromise;
	}

	async start(): Promise<void> {
		if (this.disposed) throw new RustRunnerDisposedError();
		if (this.currentState === State.READY) return;
		if (this.startPromise) return this.startPromise;

		const controller = new AbortController();
		this.startController = controller;

		const attempt = this.startAttempt(controller.signal)
			.catch(async (error: unknown) => {
				if (!this.disposed) this.setState(State.ERROR);
				let admittedError =
					error instanceof Error ? error : new Error(String(error));
				try {
					await this.teardownEmulator();
				} catch (teardownError) {
					admittedError = new AggregateError(
						[admittedError, teardownError],
						"V86 startup and emulator teardown both failed",
					);
				}
				throw controller.signal.aborted
					? abortError(controller.signal)
					: admittedError;
			})
			.finally(() => {
				if (this.startController === controller) this.startController = null;
				if (this.startPromise === attempt) this.startPromise = null;
			});
		this.startPromise = attempt;
		return attempt;
	}

	private async startAttempt(signal: AbortSignal): Promise<void> {
		await abortable(this.teardownTail, signal);
		const manifest = await withDeadline(
			signal,
			this.bootTimeoutMs,
			async (acquisitionSignal) => {
				this.setState(State.DOWNLOADING);
				const manifestUrl = new URL(
					"assets/boot-manifest.json",
					this.dependencies.baseUrl,
				).href;
				const manifest = await fetchV86BootManifest(
					manifestUrl,
					acquisitionSignal,
					this.dependencies.fetcher,
				);
				await abortable(
					this.dependencies.loadScript(
						v86BootAssetUrl(this.dependencies.baseUrl, manifest, "libv86"),
						acquisitionSignal,
					),
					acquisitionSignal,
				);
				return manifest;
			},
		);
		let savedState: ArrayBuffer | null = null;
		try {
			savedState = await withDeadline(
				signal,
				this.bootTimeoutMs,
				(stateSignal) =>
					abortable(
						loadSavedV86State(
							this.dependencies.stateStore,
							manifest.bootIdentity,
							manifest.contract.vm.maxSavedStateBytes,
						),
						stateSignal,
					),
			);
		} catch {
			throwIfAborted(signal);
		}
		const maxStateBytes = manifest.contract.vm.maxSavedStateBytes;
		let statePersistenceSafe = true;
		if (savedState) {
			try {
				const emulator = await withDeadline(
					signal,
					this.bootTimeoutMs,
					(restoreSignal) =>
						this.bootEmulator(manifest, savedState, restoreSignal),
				);
				this.assertCurrentEmulator(emulator);
				this.setState(State.READY);
				return;
			} catch (error) {
				throwIfAborted(signal);
				if (error instanceof RustRunnerDownloadError) throw error;
				await this.teardownEmulator();
				try {
					await withDeadline(signal, this.bootTimeoutMs, (cleanupSignal) =>
						abortable(
							deleteSavedV86State(this.dependencies.stateStore),
							cleanupSignal,
						),
					);
				} catch {
					throwIfAborted(signal);
					statePersistenceSafe = false;
				}
			}
		}

		const emulator = await withDeadline(
			signal,
			this.bootTimeoutMs,
			(coldSignal) => this.bootEmulator(manifest, null, coldSignal),
		);
		if (statePersistenceSafe) {
			try {
				await withDeadline(signal, this.bootTimeoutMs, async (stateSignal) => {
					const state = await abortable(emulator.save_state(), stateSignal);
					await abortable(
						saveV86State(
							this.dependencies.stateStore,
							manifest.bootIdentity,
							state,
							maxStateBytes,
						),
						stateSignal,
					);
				});
			} catch {
				throwIfAborted(signal);
			}
		}
		throwIfAborted(signal);
		this.assertCurrentEmulator(emulator);
		this.setState(State.READY);
	}

	private async bootEmulator(
		manifest: V86BootManifest,
		savedState: ArrayBuffer | null,
		signal: AbortSignal,
	): Promise<V86Emulator> {
		this.setState(State.BOOTING);
		const vm = manifest.contract.vm;
		const filesystem: Record<string, unknown> = {
			baseurl: v86RootfsBlobBaseUrl(this.dependencies.baseUrl, manifest),
		};
		if (!savedState) {
			filesystem.basefs = v86RootfsIndexUrl(
				this.dependencies.baseUrl,
				manifest,
			);
		}
		const emulator = this.dependencies.createEmulator({
			wasm_path: v86BootAssetUrl(
				this.dependencies.baseUrl,
				manifest,
				"v86Wasm",
			),
			bios: {
				url: v86BootAssetUrl(this.dependencies.baseUrl, manifest, "seabios"),
			},
			vga_bios: {
				url: v86BootAssetUrl(this.dependencies.baseUrl, manifest, "vgabios"),
			},
			autostart: vm.autostart,
			memory_size: vm.memorySizeBytes,
			vga_memory_size: vm.vgaMemorySizeBytes,
			disable_keyboard: vm.disableKeyboard,
			disable_mouse: vm.disableMouse,
			filesystem,
			bzimage_initrd_from_filesystem: vm.bzimageInitrdFromFilesystem,
			cmdline: vm.cmdline,
			initial_state: savedState ? { buffer: savedState } : undefined,
		});
		this.emulator = emulator;
		this.emulatorFailure = null;
		const coordinator = this.attachCoordinator(emulator);
		const downloadFailure = this.watchDownloadFailure(emulator);
		this.downloadFailureWatch = downloadFailure;
		await Promise.race([
			this.completeBootProtocol(coordinator, savedState !== null, signal),
			downloadFailure.promise,
		]);
		throwIfAborted(signal);
		this.assertCurrentEmulator(emulator);
		return emulator;
	}

	private async completeBootProtocol(
		coordinator: V86JobCoordinator,
		restored: boolean,
		signal: AbortSignal,
	): Promise<void> {
		const protocol = V86_BOOT_CONTRACT.guestProtocol;
		if (restored) {
			coordinator.resetSerialProtocol();
		} else {
			await coordinator.waitForSystemMarker(protocol.bootReadyMarker, signal);
			coordinator.resetSerialProtocol();
		}
		const ready = coordinator.waitForSystemMarker(protocol.readyMarker, signal);
		try {
			this.sendCommand(
				restored
					? `echo "${protocol.readyMarker}"`
					: `export PATH="/usr/local/bin:$PATH"; echo "${protocol.readyMarker}"`,
			);
		} catch (error) {
			void ready.catch(() => {});
			throw error;
		}
		await ready;
		coordinator.resetSerialProtocol();
	}

	private watchDownloadFailure(emulator: V86Emulator): DownloadFailureWatch {
		let attached = true;
		let failed = false;
		let rejectFailure: (error: Error) => void = () => {};
		const listener = (event: { file_name?: unknown }) => {
			if (failed || this.emulator !== emulator) return;
			failed = true;
			const asset =
				typeof event === "object" &&
				event !== null &&
				"file_name" in event &&
				typeof event.file_name === "string"
					? event.file_name
					: "unknown";
			const error = new RustRunnerDownloadError(asset);
			rejectFailure(error);
			this.coordinator?.invalidate(error);
		};
		const promise = new Promise<never>((_resolve, reject) => {
			rejectFailure = reject;
			emulator.add_listener("download-error", listener);
		});
		return {
			emulator,
			promise,
			dispose: () => {
				if (!attached) return;
				attached = false;
				emulator.remove_listener("download-error", listener);
			},
		};
	}

	private attachCoordinator(emulator: V86Emulator): V86JobCoordinator {
		const coordinator = new V86JobCoordinator(emulator, {
			compileTimeoutMs: this.compileTimeoutMs,
			runTimeoutMs: this.runTimeoutMs,
			nonceFactory: this.nonceFactory,
			onPhaseChange: (phase) => this.setExecutionPhase(phase),
			onPoison: (error) => {
				if (this.coordinator !== coordinator || this.emulator !== emulator) {
					return;
				}
				this.emulatorFailure = error;
				this.setState(State.ERROR);
				void this.teardownEmulator().catch(() => {
					// The poisoned generation remains unusable. A later start waits
					// on the failed teardown and therefore cannot reuse it.
				});
			},
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

	private sendCommand(command: string): void {
		if (!this.emulator) throw new Error("V86 emulator is unavailable");
		this.emulator.serial0_send(`${command}\n`);
	}

	private assertCurrentEmulator(emulator: V86Emulator): void {
		if (this.emulator !== emulator || !this.coordinator) {
			throw (
				this.emulatorFailure ??
				new Error("V86 emulator generation was invalidated during startup")
			);
		}
	}

	private teardownEmulator(): Promise<void> {
		const coordinator = this.coordinator;
		const emulator = this.emulator;
		const downloadFailure = this.downloadFailureWatch;
		this.coordinator = null;
		this.emulator = null;
		if (downloadFailure?.emulator === emulator) {
			this.downloadFailureWatch = null;
			downloadFailure.dispose();
		}
		coordinator?.dispose();
		if (!emulator) return this.teardownTail;

		const teardown = this.teardownTail.then(async () => {
			const failures: Error[] = [];
			try {
				await emulator.stop();
			} catch (error) {
				failures.push(
					error instanceof Error ? error : new Error(String(error)),
				);
			}
			try {
				await emulator.destroy();
			} catch (error) {
				failures.push(
					error instanceof Error ? error : new Error(String(error)),
				);
			}
			if (failures.length > 0) {
				throw new AggregateError(failures, "failed to destroy V86 emulator");
			}
		});
		this.teardownTail = teardown;
		void teardown.catch(() => {
			// Keep the serialized teardown tail observable to callers without
			// producing an unhandled rejection when invalidation is asynchronous.
		});
		return teardown;
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
