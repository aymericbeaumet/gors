import { MAX_SOURCE_MAP_INDEX_MAPPINGS } from "./src/source-map-index";
import {
	loadGorsWasm,
	type GorsBuildResult,
	type GorsWasm,
	usesThreadedRuntime,
} from "gors-wasm-runtime";
import {
	MAX_PERSISTED_RESOLVER_CACHE_BYTES,
	deleteResolverCacheSnapshot,
	loadResolverCacheSnapshot,
	storeResolverCacheSnapshot,
} from "./compiler-cache-storage";
import type {
	CancelRequest,
	CompileRequest,
	CompilerPhase,
	CompilerPhaseTiming,
	FlushCacheRequest,
	PersistentCacheInfo,
	WorkerCompileResult,
	WorkerRequest,
	WorkerResponse,
} from "./go2rust-protocol";

// Cache generated code and mapping buffers by bytes, not entry count. A single
// stdlib-heavy result can be much larger than dozens of small programs.
const MAX_CACHE_BYTES = 16 * 1024 * 1024;
const MAX_PACKAGED_RESOLVER_CACHE_BYTES = 16 * 1024 * 1024;
const RESOLVER_CACHE_SEED_URL = new URL(
	"./generated/resolver-cache-seed-v1.bin.gz",
	import.meta.url,
);

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
let resolverCacheRestorePromise: Promise<PersistentCacheInfo> | null = null;
let resolverCacheInfo: PersistentCacheInfo = {
	restored: false,
	importedEntries: 0,
	bytes: 0,
};
let resolverCacheGeneration = 0;
let persistedResolverCacheGeneration = 0;
let persistedResolverCacheBytes = 0;
let resolverCachePersistTimer: ReturnType<typeof setTimeout> | null = null;
interface ResolverCachePersistResult {
	bytes: number;
	generation: number;
	stored: boolean;
}
let resolverCachePersistPromise: Promise<ResolverCachePersistResult> | null =
	null;
let resolverCachePersistenceDisabled = false;

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

async function discardResolverCacheSnapshot(): Promise<void> {
	try {
		await deleteResolverCacheSnapshot();
	} catch {
		// Storage failures are cache misses, never compilation failures.
	}
}

async function readStreamCapped(
	stream: ReadableStream<Uint8Array>,
	maxBytes: number,
): Promise<Uint8Array<ArrayBuffer> | null> {
	const reader = stream.getReader();
	const chunks: Uint8Array[] = [];
	let byteLength = 0;
	while (true) {
		const { done, value } = await reader.read();
		if (done) break;
		byteLength += value.byteLength;
		if (byteLength > maxBytes) {
			await reader.cancel();
			return null;
		}
		chunks.push(value);
	}

	if (byteLength === 0) return null;
	const bytes = new Uint8Array(byteLength);
	let offset = 0;
	for (const chunk of chunks) {
		bytes.set(chunk, offset);
		offset += chunk.byteLength;
	}
	return bytes;
}

async function loadResolverCacheSeed(): Promise<Uint8Array | null> {
	try {
		const response = await fetch(RESOLVER_CACHE_SEED_URL);
		if (!response.ok || !response.body) return null;
		const contentLength = Number(response.headers.get("Content-Length"));
		if (
			Number.isFinite(contentLength) &&
			contentLength > MAX_PERSISTED_RESOLVER_CACHE_BYTES
		) {
			return null;
		}

		const packaged = await readStreamCapped(
			response.body,
			MAX_PERSISTED_RESOLVER_CACHE_BYTES,
		);
		if (!packaged) return null;

		// Some CDNs transparently decode a .gz response before Fetch exposes its
		// body. Accept that raw archive directly; otherwise decode the gzip
		// stream ourselves.
		const isGzip = packaged[0] === 0x1f && packaged[1] === 0x8b;
		if (!isGzip) {
			return packaged;
		}
		if (packaged.byteLength > MAX_PACKAGED_RESOLVER_CACHE_BYTES) return null;
		if (typeof DecompressionStream !== "function") return null;
		return readStreamCapped(
			new Blob([packaged])
				.stream()
				.pipeThrough(new DecompressionStream("gzip")),
			MAX_PERSISTED_RESOLVER_CACHE_BYTES,
		);
	} catch {
		return null;
	}
}

async function importResolverCache(
	gors: GorsWasm,
	bytes: Uint8Array,
): Promise<PersistentCacheInfo | null> {
	try {
		const importedEntries = gors.import_resolver_cache(bytes);
		return {
			restored: true,
			importedEntries,
			bytes: bytes.byteLength,
		};
	} catch {
		return null;
	}
}

async function restoreResolverCache(
	gors: GorsWasm,
): Promise<PersistentCacheInfo> {
	try {
		const bytes = await loadResolverCacheSnapshot();
		if (bytes) {
			const restored = await importResolverCache(gors, bytes);
			if (restored) {
				persistedResolverCacheBytes = bytes.byteLength;
				resolverCacheInfo = restored;
				return resolverCacheInfo;
			}
		}

		await discardResolverCacheSnapshot();
		const seed = await loadResolverCacheSeed();
		if (seed) {
			const restored = await importResolverCache(gors, seed);
			if (restored) {
				resolverCacheInfo = restored;
				// The seed is a bundled first-load fallback, not proof that an
				// IndexedDB snapshot exists. The normal delayed export persists
				// the post-compile superset.
				persistedResolverCacheBytes = 0;
				persistedResolverCacheGeneration = -1;
			}
		}
		return resolverCacheInfo;
	} catch {
		const seed = await loadResolverCacheSeed();
		if (!seed) return resolverCacheInfo;
		const restored = await importResolverCache(gors, seed);
		if (restored) {
			resolverCacheInfo = restored;
			persistedResolverCacheGeneration = -1;
		}
		return resolverCacheInfo;
	}
}

function ensureResolverCacheRestored(
	gors: GorsWasm,
): Promise<PersistentCacheInfo> {
	resolverCacheRestorePromise ??= restoreResolverCache(gors);
	return resolverCacheRestorePromise;
}

function cancelScheduledResolverCachePersist(): void {
	if (!resolverCachePersistTimer) return;
	clearTimeout(resolverCachePersistTimer);
	resolverCachePersistTimer = null;
}

async function persistResolverCacheOnce(
	gors: GorsWasm,
): Promise<ResolverCachePersistResult> {
	if (resolverCacheGeneration === persistedResolverCacheGeneration) {
		return {
			bytes: persistedResolverCacheBytes,
			generation: persistedResolverCacheGeneration,
			stored: true,
		};
	}

	const generation = resolverCacheGeneration;
	try {
		const bytes = gors.export_resolver_cache();
		if (bytes.byteLength > MAX_PERSISTED_RESOLVER_CACHE_BYTES) {
			resolverCachePersistenceDisabled = true;
			await discardResolverCacheSnapshot();
			return { bytes: 0, generation, stored: false };
		}
		const stored = await storeResolverCacheSnapshot(bytes);
		if (!stored) return { bytes: 0, generation, stored: false };
		persistedResolverCacheGeneration = generation;
		persistedResolverCacheBytes = bytes.byteLength;
		return { bytes: bytes.byteLength, generation, stored: true };
	} catch {
		return { bytes: 0, generation, stored: false };
	}
}

async function flushResolverCache(gors: GorsWasm): Promise<number> {
	cancelScheduledResolverCachePersist();
	if (resolverCachePersistenceDisabled) return 0;
	if (resolverCachePersistPromise) {
		const result = await resolverCachePersistPromise;
		if (!result.stored) return 0;
	}
	if (resolverCacheGeneration === persistedResolverCacheGeneration) {
		return persistedResolverCacheBytes;
	}

	resolverCachePersistPromise = persistResolverCacheOnce(gors);
	let result: ResolverCachePersistResult;
	try {
		result = await resolverCachePersistPromise;
	} finally {
		resolverCachePersistPromise = null;
	}
	if (!result.stored) return 0;

	// A compile may have populated more roots while IndexedDB was writing.
	if (resolverCacheGeneration !== persistedResolverCacheGeneration) {
		return flushResolverCache(gors);
	}
	return persistedResolverCacheBytes;
}

function scheduleResolverCachePersist(gors: GorsWasm): void {
	if (resolverCachePersistenceDisabled) return;
	resolverCacheGeneration++;
	cancelScheduledResolverCachePersist();
	resolverCachePersistTimer = setTimeout(() => {
		resolverCachePersistTimer = null;
		void flushResolverCache(gors);
	}, 1_000);
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
	persistentCache: PersistentCacheInfo,
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
			persistentCache,
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
		postResult(id, cached.result, startedAt, timings, true, resolverCacheInfo);
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
		postStatus(id, "loading-cache", startedAt);
		const persistentCache = await ensureResolverCacheRestored(gors);
		timings.push({
			phase: "loading-cache",
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
			scheduleResolverCachePersist(gors);
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
		postResult(id, result, startedAt, timings, false, persistentCache);
		scheduleResolverCachePersist(gors);
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
	cancelScheduledResolverCachePersist();
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

async function flushCache(request: FlushCacheRequest): Promise<void> {
	let storedBytes = 0;
	try {
		const gors = await loadGorsWasm();
		await ensureResolverCacheRestored(gors);
		storedBytes = await flushResolverCache(gors);
	} catch {
		// Cache persistence is an optimization and never blocks compilation.
	}
	worker.postMessage({
		id: request.id,
		type: "cache-flushed",
		storedBytes,
	});
}

function cancelRequests(request: CancelRequest): void {
	cancelScheduledResolverCachePersist();
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
	else if (data.type === "cancel") cancelRequests(data);
	else void flushCache(data);
};

worker.postMessage({ type: "ready", workerId, threaded: usesThreadedRuntime });
