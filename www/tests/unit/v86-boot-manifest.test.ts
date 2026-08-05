import { createHash } from "node:crypto";
import { createRequire } from "node:module";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import {
	V86_BOOT_CONTRACT,
	admitV86BootContract,
} from "../../v86-boot-contract";
import {
	admitV86BootManifest,
	fetchV86BootManifest,
	type V86BootManifest,
} from "../../v86-boot-manifest";

interface BootManifestBuilder {
	createV86BootManifest(input: {
		assetPaths: Record<string, string>;
		bootContract: unknown;
		rootfsPublication: unknown;
	}): unknown;
	rootfsBlobSetIdentity(blobNames: string[]): string;
	verifyEmittedV86BootAssets(
		manifest: unknown,
		emittedAssets: Map<string, Buffer>,
	): void;
}

const require = createRequire(import.meta.url);
const builder =
	require("../../v86-boot-manifest-build.js") as BootManifestBuilder;
const temporaryDirectories: string[] = [];

function sha(byte: string): string {
	return byte.repeat(64);
}

function contentSha256(content: Buffer): string {
	return createHash("sha256").update(content).digest("hex");
}

function rootfsPublication() {
	return {
		inputDigest: sha("1"),
		rootfs: {
			blobCount: 3,
			blobSetIdentity: sha("2"),
			indexSha256: sha("3"),
			schemaVersion: 1,
		},
		runtimeProvider: {},
		runtimeProviderSha256: sha("4"),
		schemaVersion: 1,
		type: "9p",
	};
}

async function buildManifest(
	assetSuffix = "",
): Promise<{ admitted: V86BootManifest; raw: Record<string, unknown> }> {
	const raw = await buildRawManifest(assetSuffix);
	return {
		admitted: await admitV86BootManifest(raw),
		raw,
	};
}

async function buildRawManifest(
	assetSuffix = "",
	bootContract: unknown = V86_BOOT_CONTRACT,
	publication: unknown = rootfsPublication(),
	emptyAsset?: "libv86" | "v86Wasm" | "seabios" | "vgabios",
): Promise<Record<string, unknown>> {
	const directory = await mkdtemp(path.join(tmpdir(), "gors-v86-boot-"));
	temporaryDirectories.push(directory);
	const assetPaths = {
		libv86: path.join(directory, "libv86.js"),
		v86Wasm: path.join(directory, "v86.wasm"),
		seabios: path.join(directory, "seabios.bin"),
		vgabios: path.join(directory, "vgabios.bin"),
	};
	await Promise.all(
		Object.entries(assetPaths).map(([key, file]) =>
			writeFile(file, key === emptyAsset ? "" : `${key}:${assetSuffix}`),
		),
	);
	const raw = builder.createV86BootManifest({
		assetPaths,
		bootContract,
		rootfsPublication: publication,
	}) as Record<string, unknown>;
	return raw;
}

afterEach(async () => {
	await Promise.all(
		temporaryDirectories
			.splice(0)
			.map((directory) => rm(directory, { force: true, recursive: true })),
	);
});

describe("V86 boot manifest", () => {
	it("binds full content identities for rootfs, V86, BIOS, VM, and protocol", async () => {
		const first = await buildManifest();
		const second = await buildManifest();
		const changed = await buildManifest("changed");
		const changedVmContract = {
			...V86_BOOT_CONTRACT,
			vm: {
				...V86_BOOT_CONTRACT.vm,
				memorySizeBytes: V86_BOOT_CONTRACT.vm.memorySizeBytes + 1,
			},
		};
		const changedVm = await buildRawManifest("", changedVmContract);
		const changedProtocolContract = {
			...V86_BOOT_CONTRACT,
			guestProtocol: {
				...V86_BOOT_CONTRACT.guestProtocol,
				readyMarker: "GORS_READY_V2",
			},
		};
		const changedProtocol = await buildRawManifest("", changedProtocolContract);
		const changedRootfs = rootfsPublication();
		changedRootfs.rootfs.blobCount += 1;
		const changedRootfsManifest = await buildRawManifest(
			"",
			V86_BOOT_CONTRACT,
			changedRootfs,
		);

		expect(first.admitted.bootIdentity).toMatch(/^[0-9a-f]{64}$/);
		expect(second.admitted.bootIdentity).toBe(first.admitted.bootIdentity);
		expect(changed.admitted.bootIdentity).not.toBe(first.admitted.bootIdentity);
		expect(changedVm.bootIdentity).not.toBe(first.admitted.bootIdentity);
		expect(changedProtocol.bootIdentity).not.toBe(first.admitted.bootIdentity);
		expect(changedRootfsManifest.bootIdentity).not.toBe(
			first.admitted.bootIdentity,
		);
		expect(first.admitted.rootfs.indexFile).toBe(`rootfs-${sha("3")}.json`);
		for (const asset of Object.values(first.admitted.assets)) {
			expect(asset.sha256).toMatch(/^[0-9a-f]{64}$/);
			expect(asset.file).toContain(asset.sha256);
		}
	});

	it("rejects truncated identities, fixed asset fallbacks, and extra fields", async () => {
		const { raw } = await buildManifest();
		const truncated = structuredClone(raw);
		truncated.bootIdentity = "a".repeat(16);
		await expect(admitV86BootManifest(truncated)).rejects.toThrow(
			"lowercase SHA-256",
		);

		const fallback = structuredClone(raw) as {
			assets: { libv86: { file: string } };
		};
		fallback.assets.libv86.file = "libv86.js";
		await expect(admitV86BootManifest(fallback)).rejects.toThrow(
			"content-addressed asset",
		);

		const extra = { ...raw, legacyVersion: "fixed" };
		await expect(admitV86BootManifest(extra)).rejects.toThrow(
			"unsupported field set",
		);
		await expect(
			buildRawManifest("", V86_BOOT_CONTRACT, rootfsPublication(), "libv86"),
		).rejects.toThrow("asset libv86 must not be empty");
	});

	it("rejects any evidence or checked-in protocol mismatch", async () => {
		const { raw } = await buildManifest();
		const evidenceMismatch = structuredClone(raw) as {
			rootfs: { evidence: { blobCount: number } };
		};
		evidenceMismatch.rootfs.evidence.blobCount += 1;
		await expect(admitV86BootManifest(evidenceMismatch)).rejects.toThrow(
			"identity does not match",
		);

		const protocolMismatch = structuredClone(raw) as {
			contract: { guestProtocol: { readyMarker: string } };
		};
		protocolMismatch.contract.guestProtocol.readyMarker = "GORS_OTHER_READY";
		await expect(admitV86BootManifest(protocolMismatch)).rejects.toThrow(
			"does not match this web build",
		);
	});

	it("rejects an empty rootfs in both build-time and runtime admission", async () => {
		const emptyPublication = rootfsPublication();
		emptyPublication.rootfs.blobCount = 0;
		await expect(
			buildRawManifest("", V86_BOOT_CONTRACT, emptyPublication),
		).rejects.toThrow("rootfs evidence values are invalid");

		const { raw } = await buildManifest();
		const emptyManifest = structuredClone(raw) as {
			rootfs: { evidence: { blobCount: number } };
		};
		emptyManifest.rootfs.evidence.blobCount = 0;
		await expect(admitV86BootManifest(emptyManifest)).rejects.toThrow(
			"rootfs.blobCount must be positive",
		);
	});

	it("keeps build-time and TypeScript guest-protocol grammar in parity", async () => {
		const invalidMarker = {
			...V86_BOOT_CONTRACT,
			guestProtocol: {
				...V86_BOOT_CONTRACT.guestProtocol,
				readyMarker: "gors_ready",
			},
		};
		expect(() => admitV86BootContract(invalidMarker)).toThrow(
			"canonical guest protocol token",
		);
		await expect(buildRawManifest("", invalidMarker)).rejects.toThrow(
			"guest protocol readyMarker is not canonical",
		);

		const invalidCommand = {
			...V86_BOOT_CONTRACT,
			guestProtocol: {
				...V86_BOOT_CONTRACT.guestProtocol,
				compileCommand: "GORS_COMPILE",
			},
		};
		expect(() => admitV86BootContract(invalidCommand)).toThrow(
			"canonical guest identifier",
		);
		await expect(buildRawManifest("", invalidCommand)).rejects.toThrow(
			"guest protocol compileCommand is not canonical",
		);
	});

	it("verifies the exact final webpack buffers before emitting the commit manifest", async () => {
		const rootfsIndex = Buffer.from('{"fsroot":[],"size":0,"version":3}\n');
		const blobs = [Buffer.from("first blob"), Buffer.from("second blob")];
		const blobNames = blobs.map((content) => `${contentSha256(content)}.bin`);
		const publication = rootfsPublication();
		publication.rootfs = {
			blobCount: blobNames.length,
			blobSetIdentity: builder.rootfsBlobSetIdentity(blobNames),
			indexSha256: contentSha256(rootfsIndex),
			schemaVersion: 1,
		};
		const manifest = (await buildRawManifest(
			"",
			V86_BOOT_CONTRACT,
			publication,
		)) as unknown as V86BootManifest;
		const emitted = new Map<string, Buffer>();
		for (const [key, asset] of Object.entries(manifest.assets)) {
			emitted.set(`assets/${asset.file}`, Buffer.from(`${key}:`));
		}
		emitted.set(`assets/${manifest.rootfs.indexFile}`, rootfsIndex);
		for (let index = 0; index < blobs.length; index += 1) {
			const blobName = blobNames[index];
			const blob = blobs[index];
			if (!blobName || !blob) throw new Error("missing rootfs blob fixture");
			emitted.set(`assets/rootfs-flat/${blobName}`, blob);
		}

		expect(() =>
			builder.verifyEmittedV86BootAssets(manifest, emitted),
		).not.toThrow();

		const corruptAsset = new Map(emitted);
		corruptAsset.set(
			`assets/${manifest.assets.v86Wasm.file}`,
			Buffer.from("mutated"),
		);
		expect(() =>
			builder.verifyEmittedV86BootAssets(manifest, corruptAsset),
		).toThrow("does not match its manifest");

		const missingBlob = new Map(emitted);
		const firstBlobName = blobNames[0];
		if (!firstBlobName) throw new Error("missing rootfs blob name fixture");
		missingBlob.delete(`assets/rootfs-flat/${firstBlobName}`);
		expect(() =>
			builder.verifyEmittedV86BootAssets(manifest, missingBlob),
		).toThrow("blob set does not match");

		const corruptIndex = new Map(emitted);
		corruptIndex.set(
			`assets/${manifest.rootfs.indexFile}`,
			Buffer.from("mutated"),
		);
		expect(() =>
			builder.verifyEmittedV86BootAssets(manifest, corruptIndex),
		).toThrow("index does not match");
	});

	it("fast-rejects an oversized Content-Length before reading the body", async () => {
		let bodyRead = false;
		let bodyCancelled = false;
		let fetchCache: RequestCache | undefined;
		let fetchSignal: AbortSignal | null | undefined;
		const response = {
			body: {
				cancel: async () => {
					bodyCancelled = true;
				},
				getReader: () => {
					bodyRead = true;
					throw new Error("oversized body must not be read");
				},
			},
			headers: new Headers({ "content-length": String(128 * 1024 + 1) }),
			ok: true,
			status: 200,
		} as unknown as Response;

		await expect(
			fetchV86BootManifest(
				"https://example.test/assets/boot-manifest.json",
				new AbortController().signal,
				async (_input, init) => {
					fetchCache = init?.cache;
					fetchSignal = init?.signal;
					return response;
				},
			),
		).rejects.toThrow("exceeds the configured size limit");
		expect(bodyRead).toBe(false);
		expect(bodyCancelled).toBe(true);
		expect(fetchCache).toBe("no-store");
		expect(fetchSignal?.aborted).toBe(true);
	});

	it("aborts and cancels a streaming body as soon as it crosses the cap", async () => {
		let bodyCancelled = false;
		let fetchSignal: AbortSignal | null | undefined;
		const body = new ReadableStream<Uint8Array>({
			start(controller) {
				controller.enqueue(new Uint8Array(96 * 1024));
				controller.enqueue(new Uint8Array(33 * 1024));
			},
			cancel() {
				bodyCancelled = true;
			},
		});

		await expect(
			fetchV86BootManifest(
				"https://example.test/assets/boot-manifest.json",
				new AbortController().signal,
				async (_input, init) => {
					fetchSignal = init?.signal;
					return new Response(body);
				},
			),
		).rejects.toThrow("exceeds the configured size limit");
		expect(bodyCancelled).toBe(true);
		expect(fetchSignal?.aborted).toBe(true);
	});

	it("admits a valid manifest delivered in bounded streaming chunks", async () => {
		const { admitted, raw } = await buildManifest();
		const bytes = new TextEncoder().encode(JSON.stringify(raw));
		const midpoint = Math.floor(bytes.byteLength / 2);
		const body = new ReadableStream<Uint8Array>({
			start(controller) {
				controller.enqueue(bytes.slice(0, midpoint));
				controller.enqueue(bytes.slice(midpoint));
				controller.close();
			},
		});

		await expect(
			fetchV86BootManifest(
				"https://example.test/assets/boot-manifest.json",
				new AbortController().signal,
				async () => new Response(body),
			),
		).resolves.toEqual(admitted);
	});
});
