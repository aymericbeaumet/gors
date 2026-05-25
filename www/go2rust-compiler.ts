import {
	type StructuredSourceMap,
	SourceMapIndex,
} from "./src/source-map-index";

const MAX_CACHE_ENTRIES = 32;

interface CompilerError {
	message: string;
	kind: string;
	line: number;
	column: number;
	endColumn: number;
}

interface WorkerSuccessResult {
	success: true;
	rustCode: string;
	sourceMap: StructuredSourceMap;
	error: null;
}

interface WorkerErrorResult {
	success: false;
	rustCode: "";
	sourceMap: null;
	error: CompilerError;
}

type WorkerCompileResult = WorkerSuccessResult | WorkerErrorResult;

export type CompileResult =
	| (Omit<WorkerSuccessResult, "sourceMap"> & {
			sourceMap: SourceMapIndex;
			durationMs: number;
			cacheHit: boolean;
	  })
	| (WorkerErrorResult & {
			durationMs: number;
			cacheHit: boolean;
	  });

type PendingRequest = {
	goSource: string;
	resolve: (result: CompileResult) => void;
	reject: (error: Error) => void;
};

type WorkerResponse =
	| {
			id: number;
			type: "status";
			phase: string;
	  }
	| {
			id: number;
			ok: true;
			result: WorkerCompileResult;
			durationMs: number;
			cacheHit: boolean;
	  }
	| {
			id: number;
			ok: false;
			error: string;
	  };

function withCacheMetadata(
	result: CompileResult,
	durationMs: number,
	cacheHit: boolean,
): CompileResult {
	return { ...result, durationMs, cacheHit };
}

export class CompilerCancelledError extends Error {
	constructor(message: string) {
		super(message);
		this.name = "CompilerCancelledError";
	}
}

export class Go2RustCompiler {
	private readonly cache = new Map<string, CompileResult>();
	private worker: Worker | null = null;
	private nextRequestId = 1;
	private readonly pending = new Map<number, PendingRequest>();

	dispose(): void {
		this.cancelActive("compiler worker disposed");
	}

	cancelActive(reason = "compiler request cancelled"): void {
		if (this.worker) {
			this.worker.terminate();
			this.worker = null;
		}
		for (const { reject } of this.pending.values()) {
			reject(new CompilerCancelledError(reason));
		}
		this.pending.clear();
	}

	private getWorker(): Worker {
		if (!this.worker) {
			this.worker = new Worker(
				new URL("./go2rust-worker.ts", import.meta.url),
				{
					type: "module",
				},
			);
			this.worker.onmessage = ({ data }: MessageEvent<WorkerResponse>) =>
				this.handleWorkerMessage(data);
			this.worker.onerror = (event) => {
				for (const { reject } of this.pending.values()) {
					reject(new Error(event.message || "compiler worker error"));
				}
				this.pending.clear();
				this.worker?.terminate();
				this.worker = null;
			};
		}
		return this.worker;
	}

	private handleWorkerMessage(data: WorkerResponse): void {
		const pending = this.pending.get(data?.id);
		if (!pending) return;

		if ("type" in data) return;

		this.pending.delete(data.id);

		if (!data.ok) {
			pending.reject(new Error(data.error || "compiler worker failed"));
			return;
		}

		const result = this.hydrateResult(
			data.result,
			data.durationMs,
			data.cacheHit,
		);
		this.touchCache(pending.goSource, result);
		pending.resolve(result);
	}

	private hydrateResult(
		result: WorkerCompileResult,
		durationMs: number,
		cacheHit: boolean,
	): CompileResult {
		if (!result.success) return { ...result, durationMs, cacheHit };
		return {
			...result,
			sourceMap: new SourceMapIndex(result.sourceMap, result.rustCode),
			durationMs,
			cacheHit,
		};
	}

	private touchCache(goSource: string, result: CompileResult): void {
		this.cache.delete(goSource);
		this.cache.set(goSource, result);
		if (this.cache.size > MAX_CACHE_ENTRIES) {
			const oldest = this.cache.keys().next().value;
			if (oldest !== undefined) this.cache.delete(oldest);
		}
	}

	compile(goSource: string): Promise<CompileResult> {
		if (this.cache.has(goSource)) {
			const cached = this.cache.get(goSource);
			if (cached) {
				const result = withCacheMetadata(cached, 0, true);
				this.touchCache(goSource, result);
				return Promise.resolve(result);
			}
		}

		const id = this.nextRequestId++;
		const promise = new Promise<CompileResult>((resolve, reject) => {
			this.pending.set(id, { goSource, resolve, reject });
		});
		this.getWorker().postMessage({ type: "compile", id, goSource });
		return promise;
	}
}
