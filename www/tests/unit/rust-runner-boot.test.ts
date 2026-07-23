import { describe, expect, it } from "vitest";
import type { RuntimeDependency } from "../../runtime-dependency";
import { V86_BOOT_CONTRACT } from "../../v86-boot-contract";
import {
	computeV86BootIdentity,
	type V86BootManifest,
} from "../../v86-boot-manifest";
import {
	RustRunner,
	RustRunnerDisposedError,
	RustRunnerDownloadError,
	State,
	v86DeploymentRootUrl,
	type RustRunnerDependencies,
} from "../../rust-runner";
import {
	saveV86State,
	type SavedV86StateRecord,
	type V86StateStore,
} from "../../v86-state-cache";

function hash(character: string): string {
	return character.repeat(64);
}

const runtimeDependency: RuntimeDependency = {
	schemaVersion: 1,
	contractIdentity: "ab".repeat(32),
	operationIds: new Uint16Array([14, 16]),
};

async function bootManifest(): Promise<V86BootManifest> {
	const identityInput: Omit<V86BootManifest, "bootIdentity"> = {
		assets: {
			libv86: {
				byteLength: 1,
				file: `libv86-${hash("1")}.js`,
				sha256: hash("1"),
			},
			v86Wasm: {
				byteLength: 2,
				file: `v86-${hash("2")}.wasm`,
				sha256: hash("2"),
			},
			seabios: {
				byteLength: 3,
				file: `seabios-${hash("3")}.bin`,
				sha256: hash("3"),
			},
			vgabios: {
				byteLength: 4,
				file: `vgabios-${hash("4")}.bin`,
				sha256: hash("4"),
			},
		},
		contract: V86_BOOT_CONTRACT,
		rootfs: {
			blobBaseUrl: "rootfs-flat/",
			evidence: {
				blobCount: 4,
				blobSetIdentity: hash("5"),
				indexSha256: hash("6"),
				schemaVersion: 1,
			},
			indexFile: `rootfs-${hash("6")}.json`,
		},
		schemaVersion: 1,
	};
	return {
		...identityInput,
		bootIdentity: await computeV86BootIdentity(identityInput),
	};
}

class FakeStateStore implements V86StateStore {
	deletes = 0;
	value: unknown;

	async get(): Promise<unknown> {
		return this.value;
	}

	async put(value: SavedV86StateRecord): Promise<void> {
		this.value = value;
	}

	async delete(): Promise<void> {
		this.deletes += 1;
		this.value = undefined;
	}
}

type SerialListener = (byte: number) => void;
type DownloadListener = (event: { file_name?: unknown }) => void;

class FakeBootEmulator {
	readonly commands: string[] = [];
	destroyError: Error | null = null;
	private readonly downloadListeners: DownloadListener[] = [];
	private readonly serialListeners: SerialListener[] = [];
	destroyCount = 0;
	stopError: Error | null = null;
	stopCount = 0;

	constructor(
		readonly options: Record<string, unknown>,
		private readonly behavior:
			| "ready"
			| "download-error"
			| "silent"
			| "command-error",
		private readonly recordLifecycle: (event: string) => void,
	) {
		if (behavior === "ready") {
			const filesystem = options.filesystem as Record<string, unknown>;
			if (Object.hasOwn(filesystem, "basefs")) {
				queueMicrotask(() =>
					this.emitSerialLine(V86_BOOT_CONTRACT.guestProtocol.bootReadyMarker),
				);
			}
		} else if (behavior === "download-error") {
			queueMicrotask(() => {
				for (const listener of this.downloadListeners) {
					listener({ file_name: "rootfs-index" });
				}
			});
		}
	}

	add_listener(
		event: "serial0-output-byte",
		callback: (byte: number) => void,
	): void;
	add_listener(
		event: "download-error",
		callback: (event: { file_name?: unknown }) => void,
	): void;
	add_listener(
		event: "serial0-output-byte" | "download-error",
		callback: SerialListener | DownloadListener,
	): void {
		if (event === "serial0-output-byte") {
			this.serialListeners.push(callback as SerialListener);
		} else {
			this.downloadListeners.push(callback as DownloadListener);
		}
	}

	remove_listener(
		event: "serial0-output-byte",
		callback: (byte: number) => void,
	): void;
	remove_listener(
		event: "download-error",
		callback: (event: { file_name?: unknown }) => void,
	): void;
	remove_listener(
		event: "serial0-output-byte" | "download-error",
		callback: SerialListener | DownloadListener,
	): void {
		const listeners =
			event === "serial0-output-byte"
				? this.serialListeners
				: this.downloadListeners;
		const index = listeners.indexOf(callback as never);
		if (index !== -1) listeners.splice(index, 1);
	}

	async create_file(): Promise<void> {}

	async read_file(): Promise<Uint8Array> {
		throw new Error("unused");
	}

	async save_state(): Promise<ArrayBuffer> {
		return new Uint8Array([7, 8, 9]).buffer;
	}

	serial0_send(command: string): void {
		this.commands.push(command);
		if (this.behavior === "command-error") {
			throw new Error("restore command failed");
		}
		if (this.behavior !== "ready") return;
		const marker = V86_BOOT_CONTRACT.guestProtocol.readyMarker;
		if (command.includes(`echo "${marker}"`)) {
			queueMicrotask(() => this.emitSerialLine(marker));
		}
	}

	async stop(): Promise<void> {
		this.recordLifecycle("stop:start");
		await Promise.resolve();
		this.stopCount += 1;
		this.recordLifecycle("stop:done");
		if (this.stopError) throw this.stopError;
	}

	async destroy(): Promise<void> {
		this.recordLifecycle("destroy:start");
		await Promise.resolve();
		this.destroyCount += 1;
		this.recordLifecycle("destroy:done");
		if (this.destroyError) throw this.destroyError;
	}

	emitDownloadError(fileName = "lazy-rootfs-blob"): void {
		for (const listener of [...this.downloadListeners]) {
			listener({ file_name: fileName });
		}
	}

	private emitSerialLine(line: string): void {
		for (const byte of new TextEncoder().encode(`${line}\n`)) {
			for (const listener of this.serialListeners) listener(byte);
		}
	}
}

class RunnerHarness {
	readonly creationOptions: Array<Record<string, unknown>> = [];
	readonly emulators: FakeBootEmulator[] = [];
	readonly fetchedUrls: string[] = [];
	readonly lifecycleEvents: string[] = [];
	readonly loadedScripts: string[] = [];
	readonly stateStore = new FakeStateStore();
	creationBehaviors: Array<
		"ready" | "download-error" | "silent" | "command-error" | Error
	> = [];
	manifestStatus = 200;

	constructor(readonly manifest: V86BootManifest) {}

	dependencies(): RustRunnerDependencies {
		return {
			baseUrl: "https://example.test/playground",
			createEmulator: (options) => {
				this.creationOptions.push(options);
				const behavior = this.creationBehaviors.shift() ?? "ready";
				if (behavior instanceof Error) throw behavior;
				const index = this.emulators.length;
				this.lifecycleEvents.push(`vm${index}:create`);
				const emulator = new FakeBootEmulator(options, behavior, (event) =>
					this.lifecycleEvents.push(`vm${index}:${event}`),
				);
				this.emulators.push(emulator);
				return emulator;
			},
			fetcher: async (input) => {
				const url = String(input);
				this.fetchedUrls.push(url);
				return new Response(
					this.manifestStatus === 200
						? JSON.stringify(this.manifest)
						: "unavailable",
					{ status: this.manifestStatus },
				);
			},
			loadScript: async (url) => {
				this.loadedScripts.push(url);
			},
			stateStore: this.stateStore,
		};
	}

	rootfsIndexRequests(): number {
		return this.creationOptions.filter((options) => {
			const filesystem = options.filesystem as Record<string, unknown>;
			return Object.hasOwn(filesystem, "basefs");
		}).length;
	}
}

async function waitForEmulator(harness: RunnerHarness): Promise<void> {
	for (let attempt = 0; attempt < 50; attempt += 1) {
		if (harness.emulators.length > 0) return;
		await new Promise((resolve) => setTimeout(resolve, 0));
	}
	throw new Error("emulator was not created");
}

async function waitForDestroyed(emulator: FakeBootEmulator): Promise<void> {
	for (let attempt = 0; attempt < 50; attempt += 1) {
		if (emulator.destroyCount > 0) return;
		await new Promise((resolve) => setTimeout(resolve, 0));
	}
	throw new Error("emulator was not destroyed");
}

async function waitForCommand(
	emulator: FakeBootEmulator,
	command: string,
): Promise<void> {
	for (let attempt = 0; attempt < 50; attempt += 1) {
		if (emulator.commands.some((value) => value.startsWith(command))) return;
		await Promise.resolve();
	}
	throw new Error(`guest command was not sent: ${command}`);
}

describe("RustRunner V86 boot", () => {
	it("anchors default V86 assets at the deployment root, not the active route", () => {
		expect(v86DeploymentRootUrl("https://example.test/playground/")).toBe(
			"https://example.test/",
		);
		expect(
			v86DeploymentRootUrl("https://example.test:8443/conformance/?view=all"),
		).toBe("https://example.test:8443/");
	});

	it("uses one rootfs-index request on cold boot and persists schema v2", async () => {
		const harness = new RunnerHarness(await bootManifest());
		const runner = new RustRunner({ dependencies: harness.dependencies() });

		await runner.start();

		expect(runner.state).toBe(State.READY);
		expect(harness.fetchedUrls).toEqual([
			"https://example.test/assets/boot-manifest.json",
		]);
		expect(harness.rootfsIndexRequests()).toBe(1);
		expect(harness.loadedScripts[0]).toContain(hash("1"));
		expect(harness.stateStore.value).toMatchObject({
			bootIdentity: harness.manifest.bootIdentity,
			schemaVersion: 2,
			stateByteLength: 3,
		});
	});

	it("performs zero rootfs-index requests for a valid warm state", async () => {
		const harness = new RunnerHarness(await bootManifest());
		const state = new Uint8Array([1, 2, 3]).buffer;
		await saveV86State(
			harness.stateStore,
			harness.manifest.bootIdentity,
			state,
			V86_BOOT_CONTRACT.vm.maxSavedStateBytes,
		);
		const runner = new RustRunner({ dependencies: harness.dependencies() });

		await runner.start();

		expect(harness.rootfsIndexRequests()).toBe(0);
		expect(harness.creationOptions).toHaveLength(1);
		expect(harness.creationOptions[0]).toMatchObject({
			initial_state: { buffer: state },
		});
		const filesystem = harness.creationOptions[0]?.filesystem as Record<
			string,
			unknown
		>;
		expect(Object.hasOwn(filesystem, "basefs")).toBe(false);
	});

	it("deletes a failed restore and performs exactly one cold retry", async () => {
		const harness = new RunnerHarness(await bootManifest());
		await saveV86State(
			harness.stateStore,
			harness.manifest.bootIdentity,
			new Uint8Array([1]).buffer,
			V86_BOOT_CONTRACT.vm.maxSavedStateBytes,
		);
		harness.creationBehaviors = ["command-error", "ready"];
		const runner = new RustRunner({ dependencies: harness.dependencies() });

		await runner.start();

		expect(harness.creationOptions).toHaveLength(2);
		expect(harness.rootfsIndexRequests()).toBe(1);
		expect(harness.stateStore.deletes).toBe(1);
		expect(harness.emulators[0]?.stopCount).toBe(1);
		expect(harness.emulators[0]?.destroyCount).toBe(1);
		expect(harness.emulators[1]?.stopCount).toBe(0);
		expect(harness.emulators[1]?.destroyCount).toBe(0);
		expect(harness.lifecycleEvents).toEqual([
			"vm0:create",
			"vm0:stop:start",
			"vm0:stop:done",
			"vm0:destroy:start",
			"vm0:destroy:done",
			"vm1:create",
		]);
	});

	it("times out a silent restore, deletes it, and gives cold boot a fresh deadline", async () => {
		const harness = new RunnerHarness(await bootManifest());
		const staleState = new Uint8Array([1]).buffer;
		await saveV86State(
			harness.stateStore,
			harness.manifest.bootIdentity,
			staleState,
			V86_BOOT_CONTRACT.vm.maxSavedStateBytes,
		);
		harness.creationBehaviors = ["silent", "ready"];
		const runner = new RustRunner({
			dependencies: harness.dependencies(),
			bootTimeoutMs: 20,
		});

		await runner.start();

		expect(runner.state).toBe(State.READY);
		expect(harness.creationOptions).toHaveLength(2);
		expect(harness.rootfsIndexRequests()).toBe(1);
		expect(harness.stateStore.deletes).toBe(1);
		expect(harness.emulators[0]?.stopCount).toBe(1);
		expect(harness.emulators[0]?.destroyCount).toBe(1);
		expect(harness.stateStore.value).toMatchObject({
			bootIdentity: harness.manifest.bootIdentity,
			schemaVersion: 2,
			stateByteLength: 3,
		});
		expect((harness.stateStore.value as SavedV86StateRecord).state).not.toBe(
			staleState,
		);
	});

	it("never loops after both the restore and one cold attempt fail", async () => {
		const harness = new RunnerHarness(await bootManifest());
		await saveV86State(
			harness.stateStore,
			harness.manifest.bootIdentity,
			new Uint8Array([1]).buffer,
			V86_BOOT_CONTRACT.vm.maxSavedStateBytes,
		);
		harness.creationBehaviors = [
			new Error("restore rejected"),
			new Error("cold rejected"),
		];
		const runner = new RustRunner({ dependencies: harness.dependencies() });

		await expect(runner.start()).rejects.toThrow("cold rejected");
		expect(harness.creationOptions).toHaveLength(2);
		expect(runner.state).toBe(State.ERROR);
	});

	it("fails promptly on boot-manifest and emulator download errors", async () => {
		const unavailable = new RunnerHarness(await bootManifest());
		unavailable.manifestStatus = 503;
		const manifestFailure = new RustRunner({
			dependencies: unavailable.dependencies(),
			bootTimeoutMs: 10_000,
		});
		await expect(manifestFailure.start()).rejects.toThrow("HTTP 503");
		expect(unavailable.creationOptions).toHaveLength(0);

		const download = new RunnerHarness(await bootManifest());
		download.creationBehaviors = ["download-error"];
		const assetFailure = new RustRunner({
			dependencies: download.dependencies(),
			bootTimeoutMs: 10_000,
		});
		await expect(assetFailure.start()).rejects.toBeInstanceOf(
			RustRunnerDownloadError,
		);
		expect(download.creationOptions).toHaveLength(1);
		expect(download.emulators[0]?.stopCount).toBe(1);
		expect(download.emulators[0]?.destroyCount).toBe(1);
	});

	it("invalidates and destroys a ready emulator on a lazy download failure", async () => {
		const harness = new RunnerHarness(await bootManifest());
		const runner = new RustRunner({
			dependencies: harness.dependencies(),
			bootTimeoutMs: 10_000,
		});
		await runner.start();
		const emulator = harness.emulators[0];
		if (!emulator) throw new Error("missing ready emulator");

		emulator.emitDownloadError();
		await waitForDestroyed(emulator);

		expect(runner.state).toBe(State.ERROR);
		expect(emulator.stopCount).toBe(1);
		expect(emulator.destroyCount).toBe(1);
		expect(() => runner.sendSerial("stale input")).not.toThrow();
	});

	it("destroys a cancelled live generation and requires a fresh start", async () => {
		const harness = new RunnerHarness(await bootManifest());
		const runner = new RustRunner({
			dependencies: harness.dependencies(),
			bootTimeoutMs: 10_000,
		});
		await runner.start();
		const stale = harness.emulators[0];
		if (!stale) throw new Error("missing ready emulator");

		const compiling = runner.compile("fn main() {}", runtimeDependency);
		await waitForCommand(stale, V86_BOOT_CONTRACT.guestProtocol.compileCommand);
		runner.cancelActive("replace generation");
		await expect(compiling).rejects.toThrow("replace generation");
		expect(runner.state).toBe(State.ERROR);
		await runner.start();

		expect(runner.state).toBe(State.READY);
		expect(harness.emulators).toHaveLength(2);
		expect(harness.emulators[1]).not.toBe(stale);
		expect(harness.lifecycleEvents.indexOf("vm0:destroy:done")).toBeLessThan(
			harness.lifecycleEvents.indexOf("vm1:create"),
		);
	});

	it("disposes an active boot exactly once", async () => {
		const harness = new RunnerHarness(await bootManifest());
		harness.creationBehaviors = ["silent"];
		const runner = new RustRunner({
			dependencies: harness.dependencies(),
			bootTimeoutMs: 10_000,
		});
		const starting = runner.start();
		await waitForEmulator(harness);

		const disposed = runner.dispose();
		expect(runner.dispose()).toBe(disposed);

		await expect(starting).rejects.toBeInstanceOf(RustRunnerDisposedError);
		await disposed;
		expect(harness.emulators[0]?.stopCount).toBe(1);
		expect(harness.emulators[0]?.destroyCount).toBe(1);
		expect(runner.state).toBe(State.ERROR);
	});

	it("attempts destroy after a rejected stop and contains terminal teardown errors", async () => {
		const harness = new RunnerHarness(await bootManifest());
		const runner = new RustRunner({ dependencies: harness.dependencies() });
		await runner.start();
		const emulator = harness.emulators[0];
		if (!emulator) throw new Error("missing ready emulator");
		emulator.stopError = new Error("stop failed");
		emulator.destroyError = new Error("destroy failed");

		await expect(runner.dispose()).resolves.toBeUndefined();
		expect(emulator.stopCount).toBe(1);
		expect(emulator.destroyCount).toBe(1);
	});
});
