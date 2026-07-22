import { MAX_SOURCE_MAP_INDEX_MAPPINGS } from "./src/source-map-index";
import { loadGorsWasm, type GorsBuildResult } from "gors-wasm-runtime";
import type {
	CancelRequest,
	CompileRequest,
	CompilerPhase,
	CompilerPhaseTiming,
	WorkerCompileResult,
	WorkerRequest,
	WorkerResponse,
} from "./go2rust-protocol";

// Cache generated code and mapping buffers by bytes, not entry count. A single
// stdlib-heavy result can be much larger than dozens of small programs.
const MAX_CACHE_BYTES = 16 * 1024 * 1024;

const worker = self as unknown as {
	onmessage: ((event: MessageEvent<WorkerRequest>) => void) | null;
	postMessage(message: WorkerResponse, transfer?: Transferable[]): void;
};

interface CacheEntry {
	result: WorkerCompileResult;
	bytes: number;
}

function createWorkerId(): string {
	if (typeof crypto.randomUUID === "function") return crypto.randomUUID();
	const bytes = crypto.getRandomValues(new Uint8Array(16));
	return [...bytes].map((byte) => byte.toString(16).padStart(2, "0")).join("");
}

const workerId = createWorkerId();
const cache = new Map<string, CacheEntry>();
let cacheBytes = 0;
let queuedRequest: CompileRequest | null = null;
let activeRequestId: number | null = null;
let drainScheduled = false;
let draining = false;
const cancelledRequestIds = new Set<number>();

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
	bytes += result.sourceMap.positions.byteLength;
	for (const name of result.sourceMap.names) bytes += name.length * 2 + 8;
	return bytes;
}

function touchCache(goSource: string, entry: CacheEntry): void {
	const existing = cache.get(goSource);
	if (existing) cacheBytes -= existing.bytes;
	cache.delete(goSource);

	if (entry.bytes > MAX_CACHE_BYTES) return;

	cache.set(goSource, entry);
	cacheBytes += entry.bytes;
	while (cacheBytes > MAX_CACHE_BYTES) {
		const oldestKey = cache.keys().next().value;
		if (oldestKey === undefined) break;
		const oldest = cache.get(oldestKey);
		cache.delete(oldestKey);
		if (oldest) cacheBytes -= oldest.bytes;
	}
}

function normalizeResult(result: GorsBuildResult): WorkerCompileResult {
	try {
		if (result.success) {
			const mappingCount = result.mapping_count();
			const sourceMapEnabled = mappingCount <= MAX_SOURCE_MAP_INDEX_MAPPINGS;
			return {
				success: true,
				rustCode: result.output,
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
	return {
		result: {
			...result,
			sourceMap: { ...result.sourceMap, positions },
		},
		transfer: [positions.buffer],
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

async function executeCompile(request: CompileRequest): Promise<void> {
	const { id, goSource } = request;
	const startedAt = performance.now();
	const timings: CompilerPhaseTiming[] = [];

	const cached = cache.get(goSource);
	if (cached) {
		const cacheStartedAt = performance.now();
		postStatus(id, "cache-hit", startedAt);
		touchCache(goSource, cached);
		timings.push({
			phase: "cache-hit",
			durationMs: performance.now() - cacheStartedAt,
		});
		postStatus(id, "complete", startedAt);
		postResult(id, cached.result, startedAt, timings, true);
		return;
	}

	try {
		let phaseStartedAt = performance.now();
		postStatus(id, "loading-wasm", startedAt);
		const gors = await loadGorsWasm();
		timings.push({
			phase: "loading-wasm",
			durationMs: performance.now() - phaseStartedAt,
		});
		if (cancelledRequestIds.has(id)) return;

		phaseStartedAt = performance.now();
		postStatus(id, "compiling", startedAt);
		const requestedTestDelay = request.testSynchronousDelayMs ?? 0;
		if (Number.isFinite(requestedTestDelay) && requestedTestDelay > 0) {
			const deadline = performance.now() + Math.min(requestedTestDelay, 5_000);
			let spinCount = 0;
			while (performance.now() < deadline) spinCount++;
			void spinCount;
		}
		const buildResult = gors.build_rust(goSource);
		timings.push({
			phase: "compiling",
			durationMs: performance.now() - phaseStartedAt,
		});
		if (cancelledRequestIds.has(id)) {
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

		touchCache(goSource, {
			result,
			bytes: estimateResultBytes(goSource, result),
		});
		postStatus(id, "complete", startedAt);
		postResult(id, result, startedAt, timings, false);
	} catch (error) {
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
			activeRequestId = request.id;
			if (!cancelledRequestIds.has(request.id)) {
				await executeCompile(request);
			}
			cancelledRequestIds.delete(request.id);
			activeRequestId = null;

			// Let compile/cancel events queued while synchronous Wasm was running
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

function cancelRequests(request: CancelRequest): void {
	for (const id of request.ids) {
		if (activeRequestId === id) cancelledRequestIds.add(id);
		if (queuedRequest?.id === id) {
			queuedRequest = null;
			worker.postMessage({
				id,
				type: "cancelled",
				reason: "compiler request cancelled",
			});
		}
	}
}

worker.onmessage = ({ data }) => {
	if (data.type === "compile") enqueueCompile(data);
	else cancelRequests(data);
};

worker.postMessage({ type: "ready", workerId });
