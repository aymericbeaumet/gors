import { afterEach, describe, expect, it, vi } from "vitest";
import { Go2RustCompiler } from "../../go2rust-compiler";
import type {
	CompileRequest,
	PackedSourceMap,
	WorkerResponse,
} from "../../go2rust-protocol";

const contractIdentity = "ab".repeat(32);

class FakeWorker {
	static instances: FakeWorker[] = [];

	onmessage: ((event: MessageEvent<WorkerResponse>) => void) | null = null;
	onerror: ((event: ErrorEvent) => void) | null = null;
	readonly messages: CompileRequest[] = [];
	terminated = false;

	constructor() {
		FakeWorker.instances.push(this);
	}

	postMessage(message: CompileRequest): void {
		this.messages.push(message);
	}

	terminate(): void {
		this.terminated = true;
	}

	respond(response: WorkerResponse): void {
		this.onmessage?.({ data: response } as MessageEvent<WorkerResponse>);
	}
}

async function dispatchedMessage(
	workerIndex: number,
	messageIndex: number,
): Promise<{ worker: FakeWorker; request: CompileRequest }> {
	await vi.waitFor(() => {
		expect(
			FakeWorker.instances[workerIndex]?.messages[messageIndex],
		).toBeDefined();
	});
	const worker = FakeWorker.instances[workerIndex];
	const request = worker?.messages[messageIndex];
	if (!worker || !request)
		throw new Error("fake worker request was not dispatched");
	return { worker, request };
}

function successResponse(
	id: number,
	sourceMap: PackedSourceMap,
): WorkerResponse {
	return {
		type: "result",
		id,
		ok: true,
		result: {
			success: true,
			rustCode: "fn main() {}",
			runtimeDependency: {
				schemaVersion: 1,
				contractIdentity,
				operationIds: new Uint16Array([14, 16]),
			},
			sourceMap,
			error: null,
		},
		workerDurationMs: 1,
		timings: [],
		cacheHit: false,
	};
}

afterEach(() => {
	vi.unstubAllGlobals();
	FakeWorker.instances = [];
});

describe("Go2RustCompiler result admission", () => {
	it("rejects hydration failures and releases the retained worker for retry", async () => {
		vi.stubGlobal("Worker", FakeWorker);
		const compiler = new Go2RustCompiler();

		try {
			const first = compiler.compile("package main");
			const { worker, request } = await dispatchedMessage(0, 0);
			const malformedSourceMap = {
				success: true,
				get positions(): Uint32Array {
					throw new Error("source map hydration failed");
				},
				names: [],
			};
			worker.respond(
				successResponse(
					request.id,
					malformedSourceMap as unknown as PackedSourceMap,
				),
			);

			await expect(first).rejects.toThrow("source map hydration failed");
			expect(compiler.getStats()).toMatchObject({
				activeRequestId: null,
				pendingRequestCount: 0,
			});

			const retry = compiler.compile("package main");
			const retried = await dispatchedMessage(0, 1);
			retried.worker.respond(
				successResponse(retried.request.id, {
					success: true,
					positions: new Uint32Array(),
					names: [],
				}),
			);

			await expect(retry).resolves.toMatchObject({
				success: true,
				rustCode: "fn main() {}",
			});
			expect(compiler.getStats()).toMatchObject({
				workerStartCount: 1,
				activeRequestId: null,
				pendingRequestCount: 0,
			});
		} finally {
			compiler.dispose();
		}
	});
});
