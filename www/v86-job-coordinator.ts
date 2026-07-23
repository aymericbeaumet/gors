import {
	admitRuntimeDependency,
	runtimeDependencyRequestJson,
	type RuntimeDependency,
} from "./runtime-dependency";
import { V86_BOOT_CONTRACT } from "./v86-boot-contract";

const GUEST_PROTOCOL = V86_BOOT_CONTRACT.guestProtocol;
const COMPILE_DONE = GUEST_PROTOCOL.compileDonePrefix;
const RUN_DONE = GUEST_PROTOCOL.runDonePrefix;
const NONCE_PATTERN = new RegExp(
	`^[0-9a-f]{${GUEST_PROTOCOL.nonceHexLength}}$`,
);
const MAX_SERIAL_LINE_LENGTH = 16 * 1024;
const MAX_PENDING_SERIAL_LINES = 64;

export interface V86GuestEmulator {
	create_file(path: string, data: Uint8Array): Promise<void>;
	read_file(path: string): Promise<Uint8Array>;
	serial0_send(data: string): void;
}

export interface CompileOutput {
	success: boolean;
	stderr: string;
}

export interface CompileCompleted {
	cancelled: false;
	jobId: string;
	compile: CompileOutput;
}

export interface CompileFailedBeforeStart {
	cancelled: false;
	compile: CompileOutput;
	jobId?: undefined;
}

export type CompileResult = CompileCompleted | CompileFailedBeforeStart;

export interface RunOutput {
	exitCode: number;
	stdout: string;
	stderr: string;
}

export interface RunJobResult {
	cancelled: false;
	run: RunOutput | null;
}

export type V86ExecutionPhase = "compiling" | "running";

export interface V86JobCoordinatorOptions {
	compileTimeoutMs: number;
	runTimeoutMs: number;
	nonceFactory?: () => string;
	onPhaseChange?: (phase: V86ExecutionPhase | null) => void;
	onPoison?: (error: Error) => void;
}

type FlightKind = "compile" | "run" | "compile-and-run";

interface ActiveFlight {
	readonly controller: AbortController;
	readonly kind: FlightKind;
	readonly nonce: string;
	finished: boolean;
	guestWorkStarted: boolean;
	timeout: ReturnType<typeof setTimeout> | null;
}

interface MarkerWaiter {
	readonly abort: () => void;
	readonly expected: string;
	readonly reject: (error: Error) => void;
	readonly resolve: () => void;
	readonly signal: AbortSignal;
	settled: boolean;
}

export class RustRunnerBusyError extends Error {
	constructor(
		readonly activeKind: FlightKind,
		readonly requestedKind: FlightKind,
	) {
		super(
			`V86 runner is busy with ${activeKind}; cannot admit ${requestedKind}`,
		);
		this.name = "RustRunnerBusyError";
	}
}

export class RustRunnerCancelledError extends Error {
	constructor(message = "V86 execution cancelled") {
		super(message);
		this.name = "RustRunnerCancelledError";
	}
}

export class RustRunnerDisposedError extends Error {
	constructor() {
		super("V86 runner disposed");
		this.name = "RustRunnerDisposedError";
	}
}

export class RustRunnerTimeoutError extends Error {
	constructor(
		readonly phase: "boot" | V86ExecutionPhase,
		readonly timeoutMs: number,
	) {
		super(`V86 ${phase} timed out after ${timeoutMs}ms`);
		this.name = "RustRunnerTimeoutError";
	}
}

export class RustRunnerProtocolError extends Error {
	constructor(message: string) {
		super(message);
		this.name = "RustRunnerProtocolError";
	}
}

export class RustRunnerPoisonedError extends Error {
	constructor(readonly failure: Error) {
		super(
			`V86 emulator generation is unusable and must be restarted: ${failure.message}`,
		);
		this.name = "RustRunnerPoisonedError";
	}
}

function defaultNonceFactory(): string {
	const bytes = new Uint8Array(GUEST_PROTOCOL.nonceHexLength / 2);
	crypto.getRandomValues(bytes);
	return Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join(
		"",
	);
}

function validateTimeout(name: string, timeoutMs: number): void {
	if (!Number.isFinite(timeoutMs) || timeoutMs <= 0) {
		throw new Error(`${name} timeout must be positive`);
	}
}

function abortError(signal: AbortSignal): Error {
	return signal.reason instanceof Error
		? signal.reason
		: new RustRunnerCancelledError();
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

function parseExitStatus(path: string, value: string): number {
	if (!/^(?:0|[1-9][0-9]{0,2})$/.test(value)) {
		throw new RustRunnerProtocolError(
			`guest published malformed exit status in ${path}`,
		);
	}
	const status = Number(value);
	if (status > 255) {
		throw new RustRunnerProtocolError(
			`guest published out-of-range exit status in ${path}`,
		);
	}
	return status;
}

function isMalformedJobMarker(line: string): boolean {
	for (const prefix of [COMPILE_DONE, RUN_DONE]) {
		if (line.startsWith(prefix)) {
			return !NONCE_PATTERN.test(line.slice(prefix.length));
		}
	}
	return false;
}

export class V86JobCoordinator {
	private readonly compileTimeoutMs: number;
	private readonly issuedNonces = new Set<string>();
	private readonly nonceFactory: () => string;
	private readonly onPhaseChange: (phase: V86ExecutionPhase | null) => void;
	private readonly onPoison: (error: Error) => void;
	private readonly pendingSerialLines: string[] = [];
	private readonly runTimeoutMs: number;
	private activeFlight: ActiveFlight | null = null;
	private disposed = false;
	private markerWaiter: MarkerWaiter | null = null;
	private poisoned: RustRunnerPoisonedError | null = null;
	private serialLine = "";

	constructor(
		private readonly emulator: V86GuestEmulator,
		options: V86JobCoordinatorOptions,
	) {
		validateTimeout("compile", options.compileTimeoutMs);
		validateTimeout("run", options.runTimeoutMs);
		this.compileTimeoutMs = options.compileTimeoutMs;
		this.runTimeoutMs = options.runTimeoutMs;
		this.nonceFactory = options.nonceFactory ?? defaultNonceFactory;
		this.onPhaseChange = options.onPhaseChange ?? (() => {});
		this.onPoison = options.onPoison ?? (() => {});
	}

	get busy(): boolean {
		return this.activeFlight !== null;
	}

	assertCanAdmit(requestedKind: FlightKind): void {
		if (this.disposed) throw new RustRunnerDisposedError();
		if (this.poisoned) throw this.poisoned;
		if (this.activeFlight) {
			throw new RustRunnerBusyError(this.activeFlight.kind, requestedKind);
		}
	}

	acceptSerialByte(byte: number): void {
		const character = String.fromCharCode(byte & 0xff);
		if (character === "\n") {
			const line = this.serialLine.endsWith("\r")
				? this.serialLine.slice(0, -1)
				: this.serialLine;
			this.serialLine = "";
			this.acceptSerialLine(line);
			return;
		}

		this.serialLine += character;
		if (this.serialLine.length <= MAX_SERIAL_LINE_LENGTH) return;

		this.serialLine = "";
		if (this.markerWaiter) {
			this.settleMarker(
				this.markerWaiter,
				new RustRunnerProtocolError(
					"V86 serial protocol line exceeded the configured limit",
				),
			);
		}
	}

	resetSerialProtocol(): void {
		if (this.markerWaiter) {
			throw new RustRunnerProtocolError(
				"cannot reset V86 serial protocol while awaiting a marker",
			);
		}
		this.serialLine = "";
		this.pendingSerialLines.length = 0;
	}

	waitForSystemMarker(marker: string, signal: AbortSignal): Promise<void> {
		return this.waitForMarker(marker, signal);
	}

	cancelActive(reason = "V86 execution cancelled"): void {
		this.activeFlight?.controller.abort(new RustRunnerCancelledError(reason));
	}

	invalidate(error: Error): void {
		if (this.disposed || this.poisoned) return;
		this.poison(error);
	}

	dispose(): void {
		if (this.disposed) return;
		this.disposed = true;
		const error = new RustRunnerDisposedError();
		this.activeFlight?.controller.abort(error);
		if (this.markerWaiter) this.settleMarker(this.markerWaiter, error);
		this.pendingSerialLines.length = 0;
		this.serialLine = "";
	}

	compile(
		rustSource: string,
		runtimeDependency: RuntimeDependency,
	): Promise<CompileCompleted> {
		return this.withFlight("compile", async (flight) => {
			this.armDeadline(flight, "compiling", this.compileTimeoutMs);
			return this.compileAdmitted(flight, rustSource, runtimeDependency);
		});
	}

	runJob(jobId: string): Promise<RunJobResult> {
		return this.withFlight("run", async (flight) => {
			this.armDeadline(flight, "running", this.runTimeoutMs);
			return this.runAdmitted(flight, jobId);
		});
	}

	run(
		rustSource: string,
		runtimeDependency: RuntimeDependency,
	): Promise<RunJobResult & { compile: CompileOutput }> {
		return this.withFlight("compile-and-run", async (flight) => {
			this.armDeadline(flight, "compiling", this.compileTimeoutMs);
			const compileResult = await this.compileAdmitted(
				flight,
				rustSource,
				runtimeDependency,
			);
			if (!compileResult.compile.success) {
				return { cancelled: false, compile: compileResult.compile, run: null };
			}

			this.armDeadline(flight, "running", this.runTimeoutMs);
			const runResult = await this.runAdmitted(flight, compileResult.jobId);
			return { ...runResult, compile: compileResult.compile };
		});
	}

	private acceptSerialLine(line: string): void {
		const waiter = this.markerWaiter;
		if (waiter) {
			if (line === waiter.expected) this.settleMarker(waiter);
			else if (isMalformedJobMarker(line)) {
				this.settleMarker(
					waiter,
					new RustRunnerProtocolError(
						"guest published a malformed V86 completion marker",
					),
				);
			}
			return;
		}

		this.pendingSerialLines.push(line);
		if (this.pendingSerialLines.length > MAX_PENDING_SERIAL_LINES) {
			this.pendingSerialLines.shift();
		}
	}

	private waitForMarker(expected: string, signal: AbortSignal): Promise<void> {
		if (this.disposed) return Promise.reject(new RustRunnerDisposedError());
		if (this.markerWaiter) {
			return Promise.reject(
				new RustRunnerProtocolError(
					"V86 serial protocol already has an active marker waiter",
				),
			);
		}
		if (signal.aborted) return Promise.reject(abortError(signal));

		const pendingIndex = this.pendingSerialLines.indexOf(expected);
		if (pendingIndex !== -1) {
			this.pendingSerialLines.splice(0, pendingIndex + 1);
			return Promise.resolve();
		}

		return new Promise<void>((resolve, reject) => {
			const waiter: MarkerWaiter = {
				abort: () => this.settleMarker(waiter, abortError(signal)),
				expected,
				reject,
				resolve,
				signal,
				settled: false,
			};
			this.markerWaiter = waiter;
			signal.addEventListener("abort", waiter.abort, { once: true });
		});
	}

	private settleMarker(waiter: MarkerWaiter, error?: Error): void {
		if (waiter.settled || this.markerWaiter !== waiter) return;
		waiter.settled = true;
		this.markerWaiter = null;
		waiter.signal.removeEventListener("abort", waiter.abort);
		if (error) waiter.reject(error);
		else waiter.resolve();
	}

	private issueNonce(): string {
		for (let attempt = 0; attempt < 16; attempt++) {
			const nonce = this.nonceFactory();
			if (!NONCE_PATTERN.test(nonce)) {
				throw new RustRunnerProtocolError(
					`V86 job nonce must be ${GUEST_PROTOCOL.nonceHexLength} lowercase hexadecimal characters`,
				);
			}
			if (!this.issuedNonces.has(nonce)) {
				this.issuedNonces.add(nonce);
				return nonce;
			}
		}
		throw new RustRunnerProtocolError(
			"V86 job nonce factory repeatedly produced an existing nonce",
		);
	}

	private beginFlight(kind: FlightKind): ActiveFlight {
		this.assertCanAdmit(kind);
		const flight: ActiveFlight = {
			controller: new AbortController(),
			kind,
			nonce: this.issueNonce(),
			finished: false,
			guestWorkStarted: false,
			timeout: null,
		};
		this.activeFlight = flight;
		return flight;
	}

	private finishFlight(flight: ActiveFlight): void {
		if (flight.finished) return;
		flight.finished = true;
		if (flight.timeout) clearTimeout(flight.timeout);
		flight.timeout = null;
		if (!flight.controller.signal.aborted) {
			flight.controller.abort(
				new RustRunnerCancelledError("V86 execution settled"),
			);
		}
		if (this.activeFlight === flight) this.activeFlight = null;
		if (!this.disposed && !this.poisoned) this.onPhaseChange(null);
	}

	private async withFlight<T>(
		kind: FlightKind,
		action: (flight: ActiveFlight) => Promise<T>,
	): Promise<T> {
		const flight = this.beginFlight(kind);
		try {
			return await action(flight);
		} catch (error) {
			const admittedError =
				error instanceof Error ? error : new Error(String(error));
			if (!this.disposed && flight.guestWorkStarted) {
				this.poison(admittedError);
			}
			throw admittedError;
		} finally {
			this.finishFlight(flight);
		}
	}

	private poison(error: Error): void {
		if (this.disposed || this.poisoned) return;
		this.poisoned = new RustRunnerPoisonedError(error);
		if (this.activeFlight && !this.activeFlight.controller.signal.aborted) {
			this.activeFlight.controller.abort(error);
		}
		this.onPoison(error);
	}

	private armDeadline(
		flight: ActiveFlight,
		phase: V86ExecutionPhase,
		timeoutMs: number,
	): void {
		if (flight.timeout) clearTimeout(flight.timeout);
		throwIfAborted(flight.controller.signal);
		this.onPhaseChange(phase);
		flight.timeout = setTimeout(() => {
			flight.controller.abort(new RustRunnerTimeoutError(phase, timeoutMs));
		}, timeoutMs);
	}

	private assertFlight(flight: ActiveFlight): void {
		throwIfAborted(flight.controller.signal);
		if (this.disposed) throw new RustRunnerDisposedError();
		if (this.activeFlight !== flight || flight.finished) {
			throw new RustRunnerCancelledError("V86 execution is no longer active");
		}
	}

	private async createFile(
		flight: ActiveFlight,
		path: string,
		data: Uint8Array,
	): Promise<void> {
		flight.guestWorkStarted = true;
		await abortable(
			Promise.resolve().then(() => this.emulator.create_file(path, data)),
			flight.controller.signal,
		);
		this.assertFlight(flight);
	}

	private async readRequiredText(
		flight: ActiveFlight,
		path: string,
	): Promise<string> {
		let bytes: Uint8Array;
		flight.guestWorkStarted = true;
		try {
			bytes = await abortable(
				Promise.resolve().then(() => this.emulator.read_file(path)),
				flight.controller.signal,
			);
		} catch (error) {
			throwIfAborted(flight.controller.signal);
			throw new RustRunnerProtocolError(
				`guest did not publish required file ${path}: ${
					error instanceof Error ? error.message : String(error)
				}`,
			);
		}
		this.assertFlight(flight);
		try {
			return new TextDecoder("utf-8", { fatal: true }).decode(bytes);
		} catch {
			throw new RustRunnerProtocolError(
				`guest published non-UTF-8 text in ${path}`,
			);
		}
	}

	private sendCommand(command: string): void {
		this.emulator.serial0_send(`${command}\n`);
	}

	private async sendCommandAndWaitForMarker(
		flight: ActiveFlight,
		command: string,
		expectedMarker: string,
	): Promise<void> {
		const marker = this.waitForMarker(expectedMarker, flight.controller.signal);
		flight.guestWorkStarted = true;
		try {
			this.sendCommand(command);
		} catch (error) {
			flight.controller.abort(
				error instanceof Error ? error : new Error(String(error)),
			);
		}
		await marker;
		this.assertFlight(flight);
	}

	private async compileAdmitted(
		flight: ActiveFlight,
		rustSource: string,
		runtimeDependency: RuntimeDependency,
	): Promise<CompileCompleted> {
		const admittedRuntime = admitRuntimeDependency(
			runtimeDependency,
			"RustRunner compile input",
		);
		const runtimeRequest = runtimeDependencyRequestJson(admittedRuntime);
		const jobId = flight.nonce;
		const encoder = new TextEncoder();

		await this.createFile(
			flight,
			`${GUEST_PROTOCOL.jobDirectory}/${jobId}.rs`,
			encoder.encode(rustSource),
		);
		await this.createFile(
			flight,
			`${GUEST_PROTOCOL.jobDirectory}/${jobId}.runtime.json`,
			encoder.encode(runtimeRequest),
		);

		this.resetSerialProtocol();
		await this.sendCommandAndWaitForMarker(
			flight,
			`${GUEST_PROTOCOL.compileCommand} ${jobId}`,
			COMPILE_DONE + flight.nonce,
		);

		const statusPath = `${GUEST_PROTOCOL.jobDirectory}/${jobId}.compile.status`;
		const compileStatus = parseExitStatus(
			statusPath,
			await this.readRequiredText(flight, statusPath),
		);
		const compileStderr = await this.readRequiredText(
			flight,
			`${GUEST_PROTOCOL.jobDirectory}/${jobId}.compile.err`,
		);

		return {
			cancelled: false,
			jobId,
			compile: {
				success: compileStatus === 0,
				stderr: compileStderr,
			},
		};
	}

	private async runAdmitted(
		flight: ActiveFlight,
		jobId: string,
	): Promise<RunJobResult> {
		if (!NONCE_PATTERN.test(jobId)) {
			throw new RustRunnerProtocolError(
				`V86 compiled job ID must be ${GUEST_PROTOCOL.nonceHexLength} lowercase hexadecimal characters`,
			);
		}

		const runNonce = flight.nonce;
		this.resetSerialProtocol();
		await this.sendCommandAndWaitForMarker(
			flight,
			`${GUEST_PROTOCOL.runCommand} ${jobId} ${runNonce}`,
			RUN_DONE + runNonce,
		);

		const outputPrefix = `${GUEST_PROTOCOL.jobDirectory}/${jobId}.${runNonce}.run`;
		const statusPath = `${outputPrefix}.status`;
		const exitCode = parseExitStatus(
			statusPath,
			await this.readRequiredText(flight, statusPath),
		);
		const stdout = await this.readRequiredText(flight, `${outputPrefix}.out`);
		const stderr = await this.readRequiredText(flight, `${outputPrefix}.err`);

		return {
			cancelled: false,
			run: { exitCode, stdout, stderr },
		};
	}
}
