import { SourceMapIndex } from "./src/source-map-index";
import type {
	CompilerError,
	CompilerPhase,
	CompilerPhaseTiming,
	CompilerStatus,
	PersistentCacheInfo,
	WorkerResponse,
} from "./go2rust-protocol";

export type CompileResult =
	| {
			success: true;
			rustCode: string;
			sourceMap: SourceMapIndex;
			error: null;
			durationMs: number;
			workerDurationMs: number;
			timings: CompilerPhaseTiming[];
			cacheHit: boolean;
			persistentCache: PersistentCacheInfo;
	  }
	| {
			success: false;
			rustCode: "";
			sourceMap: null;
			error: CompilerError;
			durationMs: number;
			workerDurationMs: number;
			timings: CompilerPhaseTiming[];
			cacheHit: boolean;
			persistentCache: PersistentCacheInfo;
	  };

type PendingRequest = {
	startedAt: number;
	goSource: string;
	phase?: CompilerPhase;
	resolve: (result: CompileResult) => void;
	reject: (error: Error) => void;
	onStatus?: (status: CompilerStatus) => void;
};

type PendingCacheFlush = {
	resolve: (storedBytes: number) => void;
	reject: (error: Error) => void;
};

export interface CompilerWorkerStats {
	workerId: string | null;
	workerUsesThreads: boolean | null;
	workerPhase: CompilerPhase | null;
	workerStartCount: number;
	workerPreemptCount: number;
	pendingRequestCount: number;
	pendingCacheFlushCount: number;
}

const STALE_COMPILE_PREEMPT_MS = 100;

export class CompilerCancelledError extends Error {
	constructor(message: string) {
		super(message);
		this.name = "CompilerCancelledError";
	}
}

export class Go2RustCompiler {
	private worker: Worker | null = null;
	private workerId: string | null = null;
	private workerUsesThreads: boolean | null = null;
	private workerRequestId: number | null = null;
	private workerPhase: CompilerPhase | null = null;
	private workerStartCount = 0;
	private workerPreemptCount = 0;
	private nextRequestId = 1;
	private disposed = false;
	private staleCompileTimer: ReturnType<typeof setTimeout> | null = null;
	private readonly pending = new Map<number, PendingRequest>();
	private readonly pendingCacheFlushes = new Map<number, PendingCacheFlush>();

	getStats(): CompilerWorkerStats {
		return {
			workerId: this.workerId,
			workerUsesThreads: this.workerUsesThreads,
			workerPhase: this.workerPhase,
			workerStartCount: this.workerStartCount,
			workerPreemptCount: this.workerPreemptCount,
			pendingRequestCount: this.pending.size,
			pendingCacheFlushCount: this.pendingCacheFlushes.size,
		};
	}

	dispose(): void {
		if (this.disposed) return;
		this.disposed = true;
		this.clearStaleCompileTimer();
		this.cancelActive("compiler worker disposed");
		for (const { reject } of this.pendingCacheFlushes.values()) {
			reject(new CompilerCancelledError("compiler worker disposed"));
		}
		this.pendingCacheFlushes.clear();
		this.worker?.terminate();
		this.worker = null;
		this.workerId = null;
		this.workerUsesThreads = null;
		this.workerRequestId = null;
		this.workerPhase = null;
	}

	cancelActive(reason = "compiler request cancelled"): void {
		this.clearStaleCompileTimer();
		const ids = [...this.pending.keys()];
		// Send even an empty cancellation so editor activity can defer a pending
		// background resolver-cache write before the next compile is posted.
		this.worker?.postMessage({ type: "cancel", ids });
		for (const { reject } of this.pending.values()) {
			reject(new CompilerCancelledError(reason));
		}
		this.pending.clear();
	}

	private clearStaleCompileTimer(): void {
		if (!this.staleCompileTimer) return;
		clearTimeout(this.staleCompileTimer);
		this.staleCompileTimer = null;
	}

	private preemptStaleCompile(
		worker: Worker,
		replacementRequestId: number,
	): void {
		this.clearStaleCompileTimer();
		this.staleCompileTimer = setTimeout(() => {
			this.staleCompileTimer = null;
			const replacement = this.pending.get(replacementRequestId);
			if (
				!replacement ||
				replacement.phase !== undefined ||
				this.worker !== worker
			) {
				return;
			}

			// A synchronous Wasm call cannot receive the cancellation message.
			// Replace only the stable controller worker; threaded runtimes own
			// nested Rayon workers and advertise themselves as non-preemptible.
			worker.terminate();
			this.worker = null;
			this.workerId = null;
			this.workerUsesThreads = null;
			this.workerRequestId = null;
			this.workerPhase = null;
			this.workerPreemptCount++;
			for (const { reject } of this.pendingCacheFlushes.values()) {
				reject(
					new CompilerCancelledError(
						"cache flush interrupted by stale compile preemption",
					),
				);
			}
			this.pendingCacheFlushes.clear();

			try {
				this.getWorker().postMessage({
					type: "compile",
					id: replacementRequestId,
					goSource: replacement.goSource,
				});
			} catch (error) {
				this.pending.delete(replacementRequestId);
				replacement.reject(
					error instanceof Error ? error : new Error(String(error)),
				);
			}
		}, STALE_COMPILE_PREEMPT_MS);
	}

	private getWorker(): Worker {
		if (this.disposed) throw new Error("compiler worker disposed");
		if (!this.worker) {
			const worker = new Worker(
				new URL("./go2rust-worker.ts", import.meta.url),
				{
					type: "module",
				},
			);
			this.worker = worker;
			this.workerStartCount++;
			worker.onmessage = ({ data }: MessageEvent<WorkerResponse>) => {
				if (this.worker !== worker) return;
				this.handleWorkerMessage(data);
			};
			worker.onerror = (event) => {
				if (this.worker !== worker) return;
				this.clearStaleCompileTimer();
				for (const { reject } of this.pending.values()) {
					reject(new Error(event.message || "compiler worker error"));
				}
				this.pending.clear();
				for (const { reject } of this.pendingCacheFlushes.values()) {
					reject(new Error(event.message || "compiler worker error"));
				}
				this.pendingCacheFlushes.clear();
				worker.terminate();
				this.worker = null;
				this.workerId = null;
				this.workerUsesThreads = null;
				this.workerRequestId = null;
				this.workerPhase = null;
			};
		}
		return this.worker;
	}

	private handleWorkerMessage(data: WorkerResponse): void {
		if (data.type === "ready") {
			this.workerId = data.workerId;
			this.workerUsesThreads = data.threaded;
			return;
		}

		if (data.type === "cache-flushed") {
			const flush = this.pendingCacheFlushes.get(data.id);
			if (!flush) return;
			this.pendingCacheFlushes.delete(data.id);
			flush.resolve(data.storedBytes);
			return;
		}

		if (data.type === "status") {
			this.workerRequestId = data.phase === "complete" ? null : data.id;
			this.workerPhase = data.phase === "complete" ? null : data.phase;
			this.clearStaleCompileTimer();
		} else if (
			(data.type === "cancelled" || data.type === "result") &&
			this.workerRequestId === data.id
		) {
			this.workerRequestId = null;
			this.workerPhase = null;
			this.clearStaleCompileTimer();
		}

		const pending = this.pending.get(data.id);
		if (!pending) return;

		if (data.type === "status") {
			pending.phase = data.phase;
			pending.onStatus?.({
				requestId: data.id,
				phase: data.phase,
				elapsedMs: data.elapsedMs,
			});
			return;
		}

		if (data.type === "cancelled") {
			this.pending.delete(data.id);
			pending.reject(new CompilerCancelledError(data.reason));
			return;
		}

		this.pending.delete(data.id);
		if (!data.ok) {
			pending.reject(new Error(data.error || "compiler worker failed"));
			return;
		}

		const receivedAt = performance.now();
		const queuedDurationMs = Math.max(
			0,
			receivedAt - pending.startedAt - data.workerDurationMs,
		);
		const timings: CompilerPhaseTiming[] =
			queuedDurationMs >= 0.1
				? [
						{ phase: "queued" as const, durationMs: queuedDurationMs },
						...data.timings,
					]
				: [...data.timings];
		if (!data.result.success) {
			pending.resolve({
				...data.result,
				durationMs: receivedAt - pending.startedAt,
				workerDurationMs: data.workerDurationMs,
				timings,
				cacheHit: data.cacheHit,
				persistentCache: data.persistentCache,
			});
			return;
		}

		const hydrationStartedAt = performance.now();
		const sourceMap = new SourceMapIndex(
			data.result.sourceMap,
			data.result.rustCode,
		);
		const hydrationDurationMs = performance.now() - hydrationStartedAt;
		if (hydrationDurationMs >= 0.1) {
			timings.push({
				phase: "hydrating-source-map",
				durationMs: hydrationDurationMs,
			});
		}
		const metadata = {
			durationMs: performance.now() - pending.startedAt,
			workerDurationMs: data.workerDurationMs,
			timings,
			cacheHit: data.cacheHit,
			persistentCache: data.persistentCache,
		};
		pending.resolve({
			...data.result,
			sourceMap,
			...metadata,
		});
	}

	compile(
		goSource: string,
		onStatus?: (status: CompilerStatus) => void,
		testSynchronousDelayMs = 0,
	): Promise<CompileResult> {
		if (this.disposed) {
			return Promise.reject(new Error("compiler worker disposed"));
		}

		// The playground only needs the newest editor state. Reject stale callers
		// immediately. If stable synchronous Wasm remains unresponsive, restart
		// its worker after a short grace period and restore the persistent cache.
		const activeWorker = this.worker;
		const shouldPreempt =
			activeWorker !== null &&
			this.workerUsesThreads === false &&
			this.workerPhase === "compiling";
		this.cancelActive("superseded by newer compiler input");

		const id = this.nextRequestId++;
		const promise = new Promise<CompileResult>((resolve, reject) => {
			this.pending.set(id, {
				startedAt: performance.now(),
				goSource,
				resolve,
				reject,
				onStatus,
			});
		});
		const worker = this.getWorker();
		worker.postMessage({
			type: "compile",
			id,
			goSource,
			testSynchronousDelayMs,
		});
		if (shouldPreempt && worker === activeWorker) {
			this.preemptStaleCompile(worker, id);
		}
		return promise;
	}

	flushPersistentCache(): Promise<number> {
		if (this.disposed) {
			return Promise.reject(new Error("compiler worker disposed"));
		}

		const id = this.nextRequestId++;
		const promise = new Promise<number>((resolve, reject) => {
			this.pendingCacheFlushes.set(id, { resolve, reject });
		});
		this.getWorker().postMessage({ type: "flush-cache", id });
		return promise;
	}
}
