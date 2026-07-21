import {
	CompilerCancelledError,
	Go2RustCompiler,
} from "../../go2rust-compiler";
import { storeResolverCacheSnapshot } from "../../compiler-cache-storage";
import type {
	CompilerPhaseTiming,
	CompilerStatus,
	PersistentCacheInfo,
} from "../../go2rust-protocol";

export interface HarnessResult {
	success: boolean;
	rustCode: string;
	durationMs: number;
	workerDurationMs: number;
	timings: CompilerPhaseTiming[];
	cacheHit: boolean;
	persistentCache: PersistentCacheInfo;
	statuses: CompilerStatus[];
}

export interface HarnessOutcome {
	status: "fulfilled" | "cancelled" | "rejected";
	result?: HarnessResult;
	message?: string;
}

export interface CompilerHarness {
	compile(source: string): Promise<HarnessResult>;
	compileLatest(sources: string[]): Promise<HarnessOutcome[]>;
	supersedeAfterCancellationDelay(
		activeSource: string,
		latestSource: string,
		delayMs: number,
		synchronousDelayMs: number,
	): Promise<HarnessOutcome[]>;
	preemptWithPendingFlush(
		activeSource: string,
		middleSource: string,
		latestSource: string,
		synchronousDelayMs: number,
	): Promise<{
		requests: HarnessOutcome[];
		flush: { status: "fulfilled" | "rejected"; name?: string };
	}>;
	supersedeWithoutPreemption(
		activeSource: string,
		latestSource: string,
		synchronousDelayMs: number,
	): Promise<HarnessOutcome[]>;
	flushPersistentCache(): Promise<number>;
	restartWorker(): void;
	replacePersistentCache(value: string): Promise<void>;
	stats(): ReturnType<Go2RustCompiler["getStats"]>;
}

declare global {
	interface Window {
		__gorsCompilerHarness: CompilerHarness;
	}
}

let compiler = new Go2RustCompiler();

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

async function compile(source: string): Promise<HarnessResult> {
	const statuses: CompilerStatus[] = [];
	const result = await compiler.compile(source, (status) => {
		statuses.push(status);
	});
	return {
		success: result.success,
		rustCode: result.rustCode,
		durationMs: result.durationMs,
		workerDurationMs: result.workerDurationMs,
		timings: result.timings,
		cacheHit: result.cacheHit,
		persistentCache: result.persistentCache,
		statuses,
	};
}

window.__gorsCompilerHarness = {
	compile,
	async compileLatest(sources) {
		return Promise.all(sources.map((source) => settled(compile(source))));
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
		const statuses: CompilerStatus[] = [];
		const active = settled(
			compiler
				.compile(
					activeSource,
					(status) => {
						statuses.push(status);
						if (status.phase === "compiling") reportCompilerStarted();
					},
					synchronousDelayMs,
				)
				.then((result) => ({
					success: result.success,
					rustCode: result.rustCode,
					durationMs: result.durationMs,
					workerDurationMs: result.workerDurationMs,
					timings: result.timings,
					cacheHit: result.cacheHit,
					persistentCache: result.persistentCache,
					statuses,
				})),
		);
		await compilerStarted;
		compiler.cancelActive("compiler input changed");
		await new Promise((resolve) => setTimeout(resolve, delayMs));
		const latest = settled(compile(latestSource));
		return Promise.all([active, latest]);
	},
	async preemptWithPendingFlush(
		activeSource,
		middleSource,
		latestSource,
		synchronousDelayMs,
	) {
		let reportCompilerStarted = () => {};
		const compilerStarted = new Promise<void>((resolve) => {
			reportCompilerStarted = resolve;
		});
		const active = settled(
			compiler
				.compile(
					activeSource,
					(status) => {
						if (status.phase === "compiling") reportCompilerStarted();
					},
					synchronousDelayMs,
				)
				.then((result) => ({
					success: result.success,
					rustCode: result.rustCode,
					durationMs: result.durationMs,
					workerDurationMs: result.workerDurationMs,
					timings: result.timings,
					cacheHit: result.cacheHit,
					persistentCache: result.persistentCache,
					statuses: [],
				})),
		);
		await compilerStarted;
		const flush = compiler.flushPersistentCache().then(
			() => ({ status: "fulfilled" as const }),
			(error: unknown) => ({
				status: "rejected" as const,
				name: error instanceof Error ? error.name : undefined,
			}),
		);
		const middle = settled(compile(middleSource));
		await new Promise((resolve) => setTimeout(resolve, 25));
		const latest = settled(compile(latestSource));
		return {
			requests: await Promise.all([active, middle, latest]),
			flush: await flush,
		};
	},
	async supersedeWithoutPreemption(
		activeSource,
		latestSource,
		synchronousDelayMs,
	) {
		let reportCompilerStarted = () => {};
		const compilerStarted = new Promise<void>((resolve) => {
			reportCompilerStarted = resolve;
		});
		const active = settled(
			compiler
				.compile(
					activeSource,
					(status) => {
						if (status.phase === "compiling") reportCompilerStarted();
					},
					synchronousDelayMs,
				)
				.then((result) => ({
					success: result.success,
					rustCode: result.rustCode,
					durationMs: result.durationMs,
					workerDurationMs: result.workerDurationMs,
					timings: result.timings,
					cacheHit: result.cacheHit,
					persistentCache: result.persistentCache,
					statuses: [],
				})),
		);
		await compilerStarted;
		const latest = settled(compile(latestSource));
		return Promise.all([active, latest]);
	},
	flushPersistentCache() {
		return compiler.flushPersistentCache();
	},
	restartWorker() {
		compiler.dispose();
		compiler = new Go2RustCompiler();
	},
	async replacePersistentCache(value) {
		await storeResolverCacheSnapshot(new TextEncoder().encode(value));
	},
	stats() {
		return compiler.getStats();
	},
};

document.documentElement.dataset.compilerHarness = "ready";
