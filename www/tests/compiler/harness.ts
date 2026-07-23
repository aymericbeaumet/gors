import {
	CompilerCancelledError,
	Go2RustCompiler,
	type CompileResult,
} from "../../go2rust-compiler";
import type {
	CompilerError,
	CompilerPhaseTiming,
	CompilerStatus,
} from "../../go2rust-protocol";

export interface HarnessResult {
	success: boolean;
	rustCode: string;
	runtimeDependency: {
		schemaVersion: number;
		contractIdentity: string;
		operationIds: number[];
	} | null;
	durationMs: number;
	workerDurationMs: number;
	timings: CompilerPhaseTiming[];
	cacheHit: boolean;
	error: CompilerError | null;
	statuses: CompilerStatus[];
}

export interface HarnessOutcome {
	status: "fulfilled" | "cancelled" | "rejected";
	result?: HarnessResult;
	message?: string;
}

export interface CompilerHarness {
	compile(source: string): Promise<HarnessResult>;
	compileSettled(source: string): Promise<HarnessOutcome>;
	compileLatest(sources: string[]): Promise<HarnessOutcome[]>;
	cancelThenRetrySameSource(
		source: string,
		synchronousDelayMs: number,
	): Promise<HarnessOutcome[]>;
	supersedeAfterCancellationDelay(
		activeSource: string,
		latestSource: string,
		delayMs: number,
		synchronousDelayMs: number,
	): Promise<HarnessOutcome[]>;
	supersedeDuringWasmLoad(
		activeSource: string,
		latestSource: string,
		loadDelayMs: number,
	): Promise<HarnessOutcome[]>;
	watchdogStuckWasmLoad(
		source: string,
		loadDelayMs: number,
		livenessTimeoutMs: number,
	): Promise<{
		outcome: HarnessOutcome;
		retry: HarnessOutcome;
		stats: ReturnType<Go2RustCompiler["getStats"]>;
	}>;
	stats(): ReturnType<Go2RustCompiler["getStats"]>;
}

declare global {
	interface Window {
		__gorsCompilerHarness: CompilerHarness;
	}
}

const compiler = new Go2RustCompiler();

function settled(promise: Promise<HarnessResult>): Promise<HarnessOutcome> {
	return promise.then(
		(result) => ({ status: "fulfilled", result }),
		(error: unknown) => ({
			status:
				error instanceof CompilerCancelledError ? "cancelled" : "rejected",
			message: error instanceof Error ? error.message : String(error),
		}),
	);
}

function toHarnessResult(
	result: CompileResult,
	statuses: CompilerStatus[],
): HarnessResult {
	return {
		success: result.success,
		rustCode: result.rustCode,
		runtimeDependency: result.success
			? {
					schemaVersion: result.runtimeDependency.schemaVersion,
					contractIdentity: result.runtimeDependency.contractIdentity,
					operationIds: Array.from(result.runtimeDependency.operationIds),
				}
			: null,
		durationMs: result.durationMs,
		workerDurationMs: result.workerDurationMs,
		timings: result.timings,
		cacheHit: result.cacheHit,
		error: result.error,
		statuses,
	};
}

async function compileWithControls(
	source: string,
	onStatus?: (status: CompilerStatus) => void,
	synchronousDelayMs = 0,
	wasmLoadDelayMs = 0,
): Promise<HarnessResult> {
	const statuses: CompilerStatus[] = [];
	const result = await compiler.compile(
		source,
		(status) => {
			statuses.push(status);
			onStatus?.(status);
		},
		synchronousDelayMs,
		wasmLoadDelayMs,
	);
	return toHarnessResult(result, statuses);
}

async function compile(source: string): Promise<HarnessResult> {
	return compileWithControls(source);
}

window.__gorsCompilerHarness = {
	compile,
	compileSettled(source) {
		return settled(compile(source));
	},
	async compileLatest(sources) {
		return Promise.all(sources.map((source) => settled(compile(source))));
	},
	async cancelThenRetrySameSource(source, synchronousDelayMs) {
		let reportCompilerStarted = () => {};
		const compilerStarted = new Promise<void>((resolve) => {
			reportCompilerStarted = resolve;
		});
		const active = settled(
			compileWithControls(
				source,
				(status) => {
					if (status.phase === "compiling") reportCompilerStarted();
				},
				synchronousDelayMs,
			),
		);
		await compilerStarted;
		compiler.cancelActive("compiler request cancelled by test");
		const cancelled = await active;

		// Give an unterminated worker enough time to finish and expose an
		// incorrectly published exact-output cache entry.
		await new Promise((resolve) =>
			setTimeout(resolve, synchronousDelayMs + 100),
		);
		return [cancelled, await settled(compile(source))];
	},
	async supersedeAfterCancellationDelay(
		activeSource,
		latestSource,
		delayMs,
		synchronousDelayMs,
	) {
		let reportCompilerStarted = () => {};
		const compilerStarted = new Promise<void>((resolve) => {
			reportCompilerStarted = resolve;
		});
		const active = settled(
			compileWithControls(
				activeSource,
				(status) => {
					if (status.phase === "compiling") reportCompilerStarted();
				},
				synchronousDelayMs,
			),
		);
		await compilerStarted;
		compiler.cancelActive("compiler input changed");
		await new Promise((resolve) => setTimeout(resolve, delayMs));
		const latest = settled(compile(latestSource));
		return Promise.all([active, latest]);
	},
	async supersedeDuringWasmLoad(activeSource, latestSource, loadDelayMs) {
		let reportWasmLoading = () => {};
		const wasmLoading = new Promise<void>((resolve) => {
			reportWasmLoading = resolve;
		});
		const active = settled(
			compileWithControls(
				activeSource,
				(status) => {
					if (status.phase === "loading-wasm") reportWasmLoading();
				},
				0,
				loadDelayMs,
			),
		);
		await wasmLoading;
		const latest = settled(compile(latestSource));
		const deadline = new Promise<HarnessOutcome>((resolve) => {
			setTimeout(
				() =>
					resolve({
						status: "rejected",
						message: "replacement remained blocked behind Wasm loading",
					}),
				5_000,
			);
		});
		return [await active, await Promise.race([latest, deadline])];
	},
	async watchdogStuckWasmLoad(source, loadDelayMs, livenessTimeoutMs) {
		const watchdogCompiler = new Go2RustCompiler(livenessTimeoutMs);
		const statuses: CompilerStatus[] = [];
		const outcome = await settled(
			watchdogCompiler
				.compile(source, (status) => statuses.push(status), 0, loadDelayMs)
				.then((result) => toHarnessResult(result, statuses)),
		);
		const retryStatuses: CompilerStatus[] = [];
		const retry = await settled(
			watchdogCompiler
				.compile(source, (status) => retryStatuses.push(status))
				.then((result) => toHarnessResult(result, retryStatuses)),
		);
		const stats = watchdogCompiler.getStats();
		watchdogCompiler.dispose();
		return { outcome, retry, stats };
	},
	stats() {
		return compiler.getStats();
	},
};

document.documentElement.dataset.compilerHarness = "ready";
