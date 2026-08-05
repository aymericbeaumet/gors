import { SourceMapIndex } from "./src/source-map-index";
import type {
	CompilerError,
	CompilerPhase,
	CompilerPhaseTiming,
	CompilerStatus,
	WorkerResponse,
} from "./go2rust-protocol";
import {
	admitRuntimeDependency,
	type RuntimeDependency,
} from "./runtime-dependency";

export type CompileResult =
	| {
			success: true;
			rustCode: string;
			runtimeDependency: RuntimeDependency;
			sourceMap: SourceMapIndex;
			error: null;
			durationMs: number;
			workerDurationMs: number;
			timings: CompilerPhaseTiming[];
			cacheHit: boolean;
	  }
	| {
			success: false;
			rustCode: "";
			runtimeDependency: null;
			sourceMap: null;
			error: CompilerError;
			durationMs: number;
			workerDurationMs: number;
			timings: CompilerPhaseTiming[];
			cacheHit: boolean;
	  };

type CompilerRequest = {
	id: number;
	startedAt: number;
	goSource: string;
	testSynchronousDelayMs: number;
	testWasmLoadDelayMs: number;
	resolve: (result: CompileResult) => void;
	reject: (error: Error) => void;
	onStatus?: (status: CompilerStatus) => void;
};

export interface CompilerWorkerStats {
	workerId: string | null;
	workerPhase: CompilerPhase | null;
	workerStartCount: number;
	workerPreemptCount: number;
	pendingRequestCount: number;
	activeRequestId: number | null;
	queuedRequestId: number | null;
}

const DEFAULT_WORKER_LIVENESS_TIMEOUT_MS = 30_000;

export class CompilerCancelledError extends Error {
	constructor(message: string) {
		super(message);
		this.name = "CompilerCancelledError";
	}
}

export class CompilerWorkerTimeoutError extends Error {
	constructor(message: string) {
		super(message);
		this.name = "CompilerWorkerTimeoutError";
	}
}

export class Go2RustCompiler {
	private worker: Worker | null = null;
	private workerId: string | null = null;
	private workerPhase: CompilerPhase | null = null;
	private workerStartCount = 0;
	private workerPreemptCount = 0;
	private nextRequestId = 1;
	private disposed = false;
	private dispatchTimer: ReturnType<typeof setTimeout> | null = null;
	private livenessTimer: ReturnType<typeof setTimeout> | null = null;
	private activeRequest: CompilerRequest | null = null;
	private queuedRequest: CompilerRequest | null = null;

	constructor(
		private readonly workerLivenessTimeoutMs = DEFAULT_WORKER_LIVENESS_TIMEOUT_MS,
	) {
		if (
			!Number.isFinite(workerLivenessTimeoutMs) ||
			workerLivenessTimeoutMs <= 0
		) {
			throw new Error("compiler worker liveness timeout must be positive");
		}
	}

	getStats(): CompilerWorkerStats {
		return {
			workerId: this.workerId,
			workerPhase: this.workerPhase,
			workerStartCount: this.workerStartCount,
			workerPreemptCount: this.workerPreemptCount,
			pendingRequestCount:
				Number(this.activeRequest !== null) +
				Number(this.queuedRequest !== null),
			activeRequestId: this.activeRequest?.id ?? null,
			queuedRequestId: this.queuedRequest?.id ?? null,
		};
	}

	dispose(): void {
		if (this.disposed) return;
		this.disposed = true;
		this.rejectOutstanding("compiler worker disposed", false);
		this.terminateWorker(false);
	}

	cancelActive(reason = "compiler request cancelled"): void {
		this.rejectOutstanding(reason, true);
	}

	private rejectOutstanding(reason: string, preemptActive: boolean): void {
		this.clearDispatchTimer();
		const active = this.activeRequest;
		const queued = this.queuedRequest;
		this.activeRequest = null;
		this.queuedRequest = null;
		if (active) this.terminateWorker(preemptActive);
		active?.reject(new CompilerCancelledError(reason));
		queued?.reject(new CompilerCancelledError(reason));
	}

	private clearDispatchTimer(): void {
		if (!this.dispatchTimer) return;
		clearTimeout(this.dispatchTimer);
		this.dispatchTimer = null;
	}

	private clearLivenessTimer(): void {
		if (!this.livenessTimer) return;
		clearTimeout(this.livenessTimer);
		this.livenessTimer = null;
	}

	private terminateWorker(countAsPreemption: boolean): void {
		this.clearLivenessTimer();
		const worker = this.worker;
		if (worker) {
			worker.terminate();
			if (countAsPreemption) this.workerPreemptCount++;
		}
		this.worker = null;
		this.workerId = null;
		this.workerPhase = null;
	}

	private armLivenessWatchdog(worker: Worker, request: CompilerRequest): void {
		this.clearLivenessTimer();
		this.livenessTimer = setTimeout(() => {
			this.livenessTimer = null;
			if (this.worker !== worker || this.activeRequest?.id !== request.id) {
				return;
			}

			const phase = this.workerPhase ?? "startup";
			this.activeRequest = null;
			this.terminateWorker(true);
			request.reject(
				new CompilerWorkerTimeoutError(
					`compiler worker made no progress during ${phase}`,
				),
			);
			this.scheduleDispatch();
		}, this.workerLivenessTimeoutMs);
	}

	private scheduleDispatch(): void {
		if (this.dispatchTimer || this.activeRequest || !this.queuedRequest) return;
		this.dispatchTimer = setTimeout(() => {
			this.dispatchTimer = null;
			this.dispatchQueued();
		}, 0);
	}

	private dispatchQueued(): void {
		if (this.disposed || this.activeRequest || !this.queuedRequest) return;
		const request = this.queuedRequest;
		this.queuedRequest = null;
		this.activeRequest = request;
		this.workerPhase = "queued";

		try {
			const worker = this.getWorker();
			this.armLivenessWatchdog(worker, request);
			worker.postMessage({
				type: "compile",
				id: request.id,
				goSource: request.goSource,
				testSynchronousDelayMs: request.testSynchronousDelayMs,
				testWasmLoadDelayMs: request.testWasmLoadDelayMs,
			});
		} catch (error) {
			this.activeRequest = null;
			this.terminateWorker(false);
			request.reject(error instanceof Error ? error : new Error(String(error)));
			this.scheduleDispatch();
		}
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
				this.handleWorkerMessage(worker, data);
			};
			worker.onerror = (event) => {
				if (this.worker !== worker) return;
				const active = this.activeRequest;
				this.activeRequest = null;
				this.terminateWorker(false);
				active?.reject(new Error(event.message || "compiler worker error"));
				this.scheduleDispatch();
			};
		}
		return this.worker;
	}

	private handleWorkerMessage(worker: Worker, data: WorkerResponse): void {
		if (data.type === "ready") {
			this.workerId = data.workerId;
			if (this.activeRequest) {
				this.armLivenessWatchdog(worker, this.activeRequest);
			}
			return;
		}

		const pending = this.activeRequest;
		if (!pending || pending.id !== data.id) return;

		if (data.type === "status") {
			this.workerPhase = data.phase === "complete" ? null : data.phase;
			this.armLivenessWatchdog(worker, pending);
			pending.onStatus?.({
				requestId: data.id,
				phase: data.phase,
				elapsedMs: data.elapsedMs,
			});
			return;
		}

		if (data.type === "cancelled") {
			this.finishActiveRequest(pending);
			pending.reject(new CompilerCancelledError(data.reason));
			return;
		}

		try {
			if (!data.ok) {
				throw new Error(data.error || "compiler worker failed");
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
				if (data.result.runtimeDependency !== null) {
					throw new Error(
						"compiler worker error result carried a runtime dependency",
					);
				}
				pending.resolve({
					...data.result,
					durationMs: receivedAt - pending.startedAt,
					workerDurationMs: data.workerDurationMs,
					timings,
					cacheHit: data.cacheHit,
				});
				return;
			}

			const runtimeDependency = admitRuntimeDependency(
				data.result.runtimeDependency,
				"compiler worker response",
			);
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
			};
			pending.resolve({
				...data.result,
				runtimeDependency,
				sourceMap,
				...metadata,
			});
		} catch (error) {
			pending.reject(error instanceof Error ? error : new Error(String(error)));
		} finally {
			// Keep the identity and liveness guard installed until the complete
			// response has been admitted and hydrated. Any protocol or hydration
			// exception must still settle this request and release the next one.
			this.finishActiveRequest(pending);
		}
	}

	private finishActiveRequest(request: CompilerRequest): void {
		if (this.activeRequest?.id !== request.id) return;
		this.clearLivenessTimer();
		this.activeRequest = null;
		this.workerPhase = null;
		this.scheduleDispatch();
	}

	compile(
		goSource: string,
		onStatus?: (status: CompilerStatus) => void,
		testSynchronousDelayMs = 0,
		testWasmLoadDelayMs = 0,
	): Promise<CompileResult> {
		if (this.disposed) {
			return Promise.reject(new Error("compiler worker disposed"));
		}

		// The playground only needs the newest editor state. A queued request can
		// be replaced without touching the retained worker. Once dispatched, the
		// worker generation is the cancellation boundary: destroying it is the
		// only way to prove synchronous Wasm cannot publish a stale cache entry.
		this.cancelActive("superseded by newer compiler input");

		const id = this.nextRequestId++;
		return new Promise<CompileResult>((resolve, reject) => {
			this.queuedRequest = {
				id,
				startedAt: performance.now(),
				goSource,
				testSynchronousDelayMs,
				testWasmLoadDelayMs,
				resolve,
				reject,
				onStatus,
			};
			this.scheduleDispatch();
		});
	}
}
