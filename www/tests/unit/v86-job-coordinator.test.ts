import { describe, expect, it } from "vitest";
import type { RuntimeDependency } from "../../runtime-dependency";
import {
	RustRunnerBusyError,
	RustRunnerCancelledError,
	RustRunnerDisposedError,
	RustRunnerProtocolError,
	V86JobCoordinator,
	type V86GuestEmulator,
} from "../../v86-job-coordinator";

const runtimeDependency: RuntimeDependency = {
	schemaVersion: 1,
	contractIdentity: "ab".repeat(32),
	operationIds: new Uint16Array([14, 16]),
};

class FakeEmulator implements V86GuestEmulator {
	readonly commands: string[] = [];
	readonly files = new Map<string, Uint8Array>();

	async create_file(path: string, data: Uint8Array): Promise<void> {
		this.files.set(path, data.slice());
	}

	async read_file(path: string): Promise<Uint8Array> {
		const data = this.files.get(path);
		if (!data) throw new Error("ENOENT");
		return data.slice();
	}

	serial0_send(data: string): void {
		this.commands.push(data);
	}

	publish(path: string, text: string): void {
		this.files.set(path, new TextEncoder().encode(text));
	}
}

function nonce(value: number): string {
	return value.toString(16).padStart(32, "0");
}

function createHarness(options?: {
	compileTimeoutMs?: number;
	runTimeoutMs?: number;
}) {
	const emulator = new FakeEmulator();
	let nextNonce = 1;
	const phases: Array<"compiling" | "running" | null> = [];
	const coordinator = new V86JobCoordinator(emulator, {
		compileTimeoutMs: options?.compileTimeoutMs ?? 500,
		runTimeoutMs: options?.runTimeoutMs ?? 500,
		nonceFactory: () => nonce(nextNonce++),
		onPhaseChange: (phase) => phases.push(phase),
	});
	return { coordinator, emulator, phases };
}

function emitLine(coordinator: V86JobCoordinator, line: string): void {
	for (const byte of new TextEncoder().encode(`${line}\n`)) {
		coordinator.acceptSerialByte(byte);
	}
}

async function commandAt(
	emulator: FakeEmulator,
	index: number,
): Promise<string[]> {
	for (let attempt = 0; attempt < 20; attempt++) {
		if (emulator.commands[index]) break;
		await Promise.resolve();
	}
	const command = emulator.commands[index];
	if (!command) throw new Error("guest command was not sent");
	return command.trimEnd().split(" ");
}

function publishCompile(
	emulator: FakeEmulator,
	jobId: string,
	status = "0",
	stderr = "",
): void {
	emulator.publish(`tmp/${jobId}.compile.status`, status);
	emulator.publish(`tmp/${jobId}.compile.err`, stderr);
}

describe("V86JobCoordinator", () => {
	it("holds one flight across compile and run and rejects every overlap", async () => {
		const { coordinator, emulator, phases } = createHarness();
		const first = coordinator.run("fn main() {}", runtimeDependency);
		const [, jobId] = await commandAt(emulator, 0);

		await expect(coordinator.runJob(jobId)).rejects.toBeInstanceOf(
			RustRunnerBusyError,
		);

		publishCompile(emulator, jobId);
		emitLine(coordinator, `GORS_COMPILE_DONE:${jobId}`);
		const [, runJobId, runNonce] = await commandAt(emulator, 1);
		expect(runJobId).toBe(jobId);
		expect(runNonce).toBe(jobId);
		await expect(
			coordinator.compile("fn main() {}", runtimeDependency),
		).rejects.toBeInstanceOf(RustRunnerBusyError);

		const prefix = `tmp/${jobId}.${runNonce}.run`;
		emulator.publish(`${prefix}.status`, "0");
		emulator.publish(`${prefix}.out`, "done\n");
		emulator.publish(`${prefix}.err`, "");
		emitLine(coordinator, `GORS_RUN_DONE:${runNonce}`);
		await expect(first).resolves.toMatchObject({
			cancelled: false,
			compile: { success: true, stderr: "" },
			run: { exitCode: 0, stdout: "done\n", stderr: "" },
		});
		expect(phases).toEqual(["compiling", "running", null]);
	});

	it("ignores a cancelled job's stale marker and recovers with a new nonce", async () => {
		const { coordinator, emulator } = createHarness();
		const first = coordinator.compile("fn main() {}", runtimeDependency);
		const [, staleNonce] = await commandAt(emulator, 0);
		coordinator.cancelActive("superseded");
		await expect(first).rejects.toEqual(
			expect.objectContaining<RustRunnerCancelledError>({
				name: "RustRunnerCancelledError",
				message: "superseded",
			}),
		);

		const retry = coordinator.compile("fn main() {}", runtimeDependency);
		const [, retryNonce] = await commandAt(emulator, 1);
		let retrySettled = false;
		void retry.then(
			() => {
				retrySettled = true;
			},
			() => {
				retrySettled = true;
			},
		);

		emitLine(coordinator, `GORS_COMPILE_DONE:${staleNonce}`);
		await new Promise((resolve) => setTimeout(resolve, 5));
		expect(retrySettled).toBe(false);

		publishCompile(emulator, retryNonce);
		emitLine(coordinator, `GORS_COMPILE_DONE:${retryNonce}`);
		await expect(retry).resolves.toMatchObject({
			jobId: retryNonce,
			compile: { success: true },
		});
	});

	it("rejects a malformed compile marker and admits a successful retry", async () => {
		const { coordinator, emulator } = createHarness({
			compileTimeoutMs: 20,
		});
		const malformed = coordinator.compile("fn main() {}", runtimeDependency);
		const [, staleNonce] = await commandAt(emulator, 0);
		emitLine(coordinator, `GORS_COMPILE_DONE:${staleNonce}:malformed`);
		await expect(malformed).rejects.toThrow("malformed V86 completion marker");

		const retry = coordinator.compile("fn main() {}", runtimeDependency);
		const [, retryNonce] = await commandAt(emulator, 1);
		emitLine(coordinator, `GORS_COMPILE_DONE:${staleNonce}`);
		publishCompile(emulator, retryNonce);
		emitLine(coordinator, `GORS_COMPILE_DONE:${retryNonce}`);
		await expect(retry).resolves.toMatchObject({
			jobId: retryNonce,
			compile: { success: true },
		});
	});

	it("fails closed on missing or malformed guest status and then recovers", async () => {
		const { coordinator, emulator } = createHarness();
		const missing = coordinator.compile("fn main() {}", runtimeDependency);
		const [, missingNonce] = await commandAt(emulator, 0);
		emulator.publish(`tmp/${missingNonce}.compile.err`, "");
		emitLine(coordinator, `GORS_COMPILE_DONE:${missingNonce}`);
		await expect(missing).rejects.toBeInstanceOf(RustRunnerProtocolError);

		const malformed = coordinator.compile("fn main() {}", runtimeDependency);
		const [, malformedNonce] = await commandAt(emulator, 1);
		publishCompile(emulator, malformedNonce, "0\n");
		emitLine(coordinator, `GORS_COMPILE_DONE:${malformedNonce}`);
		await expect(malformed).rejects.toThrow("malformed exit status");

		const recovered = coordinator.compile("fn main() {}", runtimeDependency);
		const [, recoveredNonce] = await commandAt(emulator, 2);
		publishCompile(emulator, recoveredNonce);
		emitLine(coordinator, `GORS_COMPILE_DONE:${recoveredNonce}`);
		await expect(recovered).resolves.toMatchObject({
			jobId: recoveredNonce,
			compile: { success: true },
		});
	});

	it("uses a fresh run nonce and preserves output text byte-for-byte", async () => {
		const { coordinator, emulator } = createHarness();
		const compiled = coordinator.compile("fn main() {}", runtimeDependency);
		const [, jobId] = await commandAt(emulator, 0);
		publishCompile(emulator, jobId, "0", " compiler stderr stays exact \n");
		emitLine(coordinator, `GORS_COMPILE_DONE:${jobId}`);
		await expect(compiled).resolves.toMatchObject({
			compile: { stderr: " compiler stderr stays exact \n" },
		});

		const running = coordinator.runJob(jobId);
		const [, runJobId, runNonce] = await commandAt(emulator, 1);
		expect(runJobId).toBe(jobId);
		expect(runNonce).not.toBe(jobId);
		const prefix = `tmp/${jobId}.${runNonce}.run`;
		emulator.publish(`${prefix}.status`, "7");
		emulator.publish(`${prefix}.out`, " \nstdout\n\n");
		emulator.publish(`${prefix}.err`, "\u001b[31mstderr\u001b[0m\n");
		emitLine(coordinator, `GORS_RUN_DONE:${runNonce}`);

		await expect(running).resolves.toEqual({
			cancelled: false,
			run: {
				exitCode: 7,
				stdout: " \nstdout\n\n",
				stderr: "\u001b[31mstderr\u001b[0m\n",
			},
		});
	});

	it("times out a run with no marker and rejects work after disposal", async () => {
		const { coordinator, emulator } = createHarness({ runTimeoutMs: 20 });
		const running = coordinator.runJob(nonce(99));
		await commandAt(emulator, 0);
		await expect(running).rejects.toEqual(
			expect.objectContaining({
				name: "RustRunnerTimeoutError",
				phase: "running",
			}),
		);

		const pending = coordinator.compile("fn main() {}", runtimeDependency);
		const [, pendingNonce] = await commandAt(emulator, 1);
		coordinator.dispose();
		await expect(pending).rejects.toBeInstanceOf(RustRunnerDisposedError);
		emitLine(coordinator, `GORS_COMPILE_DONE:${pendingNonce}`);
		await expect(
			coordinator.compile("fn main() {}", runtimeDependency),
		).rejects.toBeInstanceOf(RustRunnerDisposedError);
	});
});
