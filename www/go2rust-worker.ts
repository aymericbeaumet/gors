import { MAX_SOURCE_MAP_INDEX_MAPPINGS } from "./src/source-map-index";
import type { GorsBuildResult } from "./gors-wasm-loader";
import {
	CompilerSourceRevision,
	createCompilerSessionLoader,
} from "./gors-compiler-session";
import type {
	CompileRequest,
	CompilerPhase,
	CompilerPhaseTiming,
	WorkerCompileResult,
	WorkerResponse,
	WorkerSuccessResult,
} from "./go2rust-protocol";
import {
	admitRuntimeDependency,
	runtimeDependencyCacheIdentity,
} from "./runtime-dependency";

// Cache generated code and mapping buffers by bytes, not entry count. A single
// stdlib-heavy result can be much larger than dozens of small programs.
const MAX_CACHE_BYTES = 16 * 1024 * 1024;

const worker = self as unknown as {
	onmessage: ((event: MessageEvent<CompileRequest>) => void) | null;
	postMessage(message: WorkerResponse, transfer?: Transferable[]): void;
};

interface CacheEntry {
	identity: string;
	goSource: string;
	result: WorkerSuccessResult;
	bytes: number;
}

interface ActiveCompile {
	request: CompileRequest;
	cancelled: boolean;
}

function createWorkerId(): string {
	if (typeof crypto.randomUUID === "function") return crypto.randomUUID();
	const bytes = crypto.getRandomValues(new Uint8Array(16));
	return [...bytes].map((byte) => byte.toString(16).padStart(2, "0")).join("");
}

const workerId = createWorkerId();
const cache = new Map<string, CacheEntry>();
const cacheIdentityBySource = new Map<string, string>();
let cacheBytes = 0;
let queuedRequest: CompileRequest | null = null;
let activeCompile: ActiveCompile | null = null;
let drainScheduled = false;
let draining = false;
const loadCompiler = createCompilerSessionLoader();
const compilerSourceRevision = new CompilerSourceRevision();

function postStatus(id: number, phase: CompilerPhase, startedAt: number): void {
	worker.postMessage({
		id,
		type: "status",
		phase,
		elapsedMs: performance.now() - startedAt,
	});
}

function estimateResultBytes(
	goSource: string,
	result: WorkerCompileResult,
): number {
	let bytes = goSource.length * 2;
	if (!result.success) {
		const error = result.error;
		return (
			bytes +
			(error.message.length + error.kind.length) * 2 +
			5 * Float64Array.BYTES_PER_ELEMENT
		);
	}

	bytes += result.rustCode.length * 2;
	bytes += result.runtimeDependency.contractIdentity.length * 2;
	bytes += result.runtimeDependency.operationIds.byteLength;
	bytes += result.sourceMap.positions.byteLength;
	for (const name of result.sourceMap.names) bytes += name.length * 2 + 8;
	return bytes;
}

function touchCache(
	goSource: string,
	result: WorkerSuccessResult,
	bytes: number,
): void {
	const runtimeDependency = admitRuntimeDependency(
		result.runtimeDependency,
		"compiler worker cache insertion",
	);
	const identity = runtimeDependencyCacheIdentity(goSource, runtimeDependency);
	const existingIdentity = cacheIdentityBySource.get(goSource);
	const existing = existingIdentity ? cache.get(existingIdentity) : undefined;
	if (existing) cacheBytes -= existing.bytes;
	if (existingIdentity) cache.delete(existingIdentity);
	cacheIdentityBySource.delete(goSource);

	if (bytes > MAX_CACHE_BYTES) return;

	const entry: CacheEntry = {
		identity,
		goSource,
		result: { ...result, runtimeDependency },
		bytes,
	};
	cache.set(identity, entry);
	cacheIdentityBySource.set(goSource, identity);
	cacheBytes += bytes;
	while (cacheBytes > MAX_CACHE_BYTES) {
		const oldestIdentity = cache.keys().next().value;
		if (oldestIdentity === undefined) break;
		const oldest = cache.get(oldestIdentity);
		cache.delete(oldestIdentity);
		if (oldest) {
			cacheBytes -= oldest.bytes;
			if (cacheIdentityBySource.get(oldest.goSource) === oldestIdentity) {
				cacheIdentityBySource.delete(oldest.goSource);
			}
		}
	}
}

function readCached(goSource: string): CacheEntry | undefined {
	const identity = cacheIdentityBySource.get(goSource);
	if (identity === undefined) return undefined;
	const entry = cache.get(identity);
	if (!entry) {
		throw new Error("compiler worker cache index references a missing entry");
	}
	const runtimeDependency = admitRuntimeDependency(
		entry.result.runtimeDependency,
		"compiler worker cache reuse",
	);
	const expectedIdentity = runtimeDependencyCacheIdentity(
		goSource,
		runtimeDependency,
	);
	if (
		entry.goSource !== goSource ||
		entry.identity !== identity ||
		expectedIdentity !== identity
	) {
		throw new Error(
			"compiler worker cache identity does not match its payload",
		);
	}
	return {
		...entry,
		result: { ...entry.result, runtimeDependency },
	};
}

function normalizeResult(result: GorsBuildResult): WorkerCompileResult {
	try {
		if (result.success) {
			const runtimeDependency = admitRuntimeDependency(
				{
					schemaVersion: result.runtime_dependency_schema_version,
					contractIdentity: result.runtime_contract_identity,
					operationIds: result.get_runtime_operation_ids(),
				},
				"Wasm compiler result",
			);
			const mappingCount = result.mapping_count();
			const sourceMapEnabled = mappingCount <= MAX_SOURCE_MAP_INDEX_MAPPINGS;
			return {
				success: true,
				rustCode: result.output,
				runtimeDependency,
				sourceMap: {
					success: sourceMapEnabled,
					positions: sourceMapEnabled
						? result.get_mapping_positions()
						: new Uint32Array(),
					names: sourceMapEnabled
						? (JSON.parse(result.get_mapping_names_json()) as string[])
						: [],
				},
				error: null,
			};
		}

		return {
			success: false,
			rustCode: "",
			runtimeDependency: null,
			sourceMap: null,
			error: {
				message: result.error_message,
				kind:
					result.error_kind === "scanner"
						? "scanner error"
						: result.error_kind === "parser"
							? "syntax error"
							: "compile error",
				line: result.error_line,
				column: result.error_column,
				endColumn: result.error_end_column,
			},
		};
	} finally {
		result.free();
	}
}

function transferableResult(result: WorkerCompileResult): {
	result: WorkerCompileResult;
	transfer: Transferable[];
} {
	if (!result.success) return { result, transfer: [] };

	// The cache retains the original buffer. Transfer a single compact copy to
	// the UI instead of cloning every nested source-map tuple.
	const positions = result.sourceMap.positions.slice();
	const runtimeDependency = admitRuntimeDependency(
		result.runtimeDependency,
		"compiler worker transfer",
	);
	return {
		result: {
			...result,
			runtimeDependency,
			sourceMap: { ...result.sourceMap, positions },
		},
		transfer: [positions.buffer, runtimeDependency.operationIds.buffer],
	};
}

function postResult(
	id: number,
	result: WorkerCompileResult,
	startedAt: number,
	timings: CompilerPhaseTiming[],
	cacheHit: boolean,
): void {
	const outgoing = transferableResult(result);
	worker.postMessage(
		{
			id,
			type: "result",
			ok: true,
			result: outgoing.result,
			workerDurationMs: performance.now() - startedAt,
			timings,
			cacheHit,
		},
		outgoing.transfer,
	);
}

function mayPublish(active: ActiveCompile): boolean {
	return activeCompile === active && !active.cancelled;
}

function requestedDelay(value: number | undefined, maximumMs: number): number {
	if (value === undefined || !Number.isFinite(value) || value <= 0) return 0;
	return Math.min(value, maximumMs);
}

function delay(ms: number): Promise<void> {
	return new Promise((resolve) => setTimeout(resolve, ms));
}

async function executeCompile(active: ActiveCompile): Promise<void> {
	const { request } = active;
	const { id, goSource } = request;
	const startedAt = performance.now();
	const timings: CompilerPhaseTiming[] = [];

	try {
		if (!mayPublish(active)) return;
		const cached = readCached(goSource);
		// An older artifact cannot bypass reinstalling its source in the retained
		// compiler; otherwise the following edit would fork from a stale revision.
		if (
			cached &&
			compilerSourceRevision.canReuseCachedResult(goSource) &&
			mayPublish(active)
		) {
			const cacheStartedAt = performance.now();
			postStatus(id, "cache-hit", startedAt);
			touchCache(goSource, cached.result, cached.bytes);
			timings.push({
				phase: "cache-hit",
				durationMs: performance.now() - cacheStartedAt,
			});
			postStatus(id, "complete", startedAt);
			postResult(id, cached.result, startedAt, timings, true);
			return;
		}

		let phaseStartedAt = performance.now();
		postStatus(id, "loading-wasm", startedAt);
		const loadDelayMs = requestedDelay(request.testWasmLoadDelayMs, 60_000);
		if (loadDelayMs > 0) await delay(loadDelayMs);
		if (!mayPublish(active)) return;
		const compiler = await loadCompiler();
		timings.push({
			phase: "loading-wasm",
			durationMs: performance.now() - phaseStartedAt,
		});
		if (!mayPublish(active)) return;

		phaseStartedAt = performance.now();
		postStatus(id, "compiling", startedAt);
		const compileDelayMs = requestedDelay(
			request.testSynchronousDelayMs,
			5_000,
		);
		if (compileDelayMs > 0) {
			const deadline = performance.now() + compileDelayMs;
			let spinCount = 0;
			while (performance.now() < deadline) spinCount++;
			void spinCount;
		}
		compilerSourceRevision.beginCompile();
		const buildResult = compiler.build_rust(goSource);
		compilerSourceRevision.completeCompile(goSource, buildResult.success);
		timings.push({
			phase: "compiling",
			durationMs: performance.now() - phaseStartedAt,
		});
		if (!mayPublish(active)) {
			buildResult.free();
			return;
		}

		phaseStartedAt = performance.now();
		postStatus(id, "indexing-source-map", startedAt);
		const result = normalizeResult(buildResult);
		timings.push({
			phase: "indexing-source-map",
			durationMs: performance.now() - phaseStartedAt,
		});

		if (!mayPublish(active)) return;
		if (result.success) {
			touchCache(goSource, result, estimateResultBytes(goSource, result));
		}
		postStatus(id, "complete", startedAt);
		postResult(id, result, startedAt, timings, false);
	} catch (error) {
		if (!mayPublish(active)) return;
		worker.postMessage({
			id,
			type: "result",
			ok: false,
			error: error instanceof Error ? error.message : String(error),
		});
	}
}

function yieldToWorkerMessages(): Promise<void> {
	return new Promise((resolve) => setTimeout(resolve, 0));
}

async function drainQueue(): Promise<void> {
	if (draining) return;
	draining = true;
	drainScheduled = false;
	try {
		while (queuedRequest) {
			const request = queuedRequest;
			queuedRequest = null;
			const active: ActiveCompile = { request, cancelled: false };
			activeCompile = active;
			await executeCompile(active);
			if (activeCompile === active) activeCompile = null;

			// Let compile events queued while synchronous Wasm was running
			// collapse into one latest request before starting more work.
			await yieldToWorkerMessages();
		}
	} finally {
		draining = false;
		if (queuedRequest) scheduleDrain();
	}
}

function scheduleDrain(): void {
	if (draining || drainScheduled) return;
	drainScheduled = true;
	setTimeout(() => {
		void drainQueue();
	}, 0);
}

function enqueueCompile(request: CompileRequest): void {
	postStatus(request.id, "queued", performance.now());
	if (activeCompile && !activeCompile.cancelled) {
		activeCompile.cancelled = true;
		worker.postMessage({
			id: activeCompile.request.id,
			type: "cancelled",
			reason: "superseded by newer compiler input",
		});
	}
	if (queuedRequest) {
		worker.postMessage({
			id: queuedRequest.id,
			type: "cancelled",
			reason: "superseded by newer compiler input",
		});
	}
	queuedRequest = request;
	scheduleDrain();
}

worker.onmessage = ({ data }) => enqueueCompile(data);

worker.postMessage({ type: "ready", workerId });
