import {
	V86_BOOT_CONTRACT,
	admitV86BootContract,
	sameV86BootContract,
	type V86BootContract,
} from "./v86-boot-contract";

const BOOT_IDENTITY_DOMAIN = "gors-v86-boot-manifest-v1\0";
const MAX_BOOT_MANIFEST_BYTES = 128 * 1024;
const SHA256_PATTERN = /^[0-9a-f]{64}$/;
const ASSET_LAYOUT = {
	libv86: { stem: "libv86", extension: ".js" },
	v86Wasm: { stem: "v86", extension: ".wasm" },
	seabios: { stem: "seabios", extension: ".bin" },
	vgabios: { stem: "vgabios", extension: ".bin" },
} as const;

export type V86AssetKey = keyof typeof ASSET_LAYOUT;
export type V86ManifestFetcher = (
	input: string | URL | Request,
	init?: RequestInit,
) => Promise<Response>;

export interface V86BootAsset {
	readonly byteLength: number;
	readonly file: string;
	readonly sha256: string;
}

export interface V86RootfsEvidence {
	readonly blobCount: number;
	readonly blobSetIdentity: string;
	readonly indexSha256: string;
	readonly schemaVersion: 1;
}

export interface V86BootManifest {
	readonly schemaVersion: 1;
	readonly bootIdentity: string;
	readonly assets: Readonly<Record<V86AssetKey, V86BootAsset>>;
	readonly rootfs: {
		readonly blobBaseUrl: string;
		readonly evidence: V86RootfsEvidence;
		readonly indexFile: string;
	};
	readonly contract: V86BootContract;
}

type BootIdentityInput = Omit<V86BootManifest, "bootIdentity">;

function isRecord(value: unknown): value is Record<string, unknown> {
	return typeof value === "object" && value !== null && !Array.isArray(value);
}

function hasExactFields(
	value: Record<string, unknown>,
	fields: readonly string[],
): boolean {
	const actual = Object.keys(value).sort();
	const expected = [...fields].sort();
	return (
		actual.length === expected.length &&
		actual.every((field, index) => field === expected[index])
	);
}

function requireRecord(
	value: unknown,
	fields: readonly string[],
	label: string,
): asserts value is Record<string, unknown> {
	if (!isRecord(value) || !hasExactFields(value, fields)) {
		throw new Error(`${label} has an unsupported field set`);
	}
}

function requireSha256(value: unknown, label: string): asserts value is string {
	if (typeof value !== "string" || !SHA256_PATTERN.test(value)) {
		throw new Error(`${label} must be lowercase SHA-256`);
	}
}

function requireNonNegativeSafeInteger(
	value: unknown,
	label: string,
): asserts value is number {
	if (!Number.isSafeInteger(value) || (value as number) < 0) {
		throw new Error(`${label} must be a non-negative safe integer`);
	}
}

function admitAsset(value: unknown, key: V86AssetKey): V86BootAsset {
	requireRecord(value, ["byteLength", "file", "sha256"], `V86 ${key} asset`);
	requireNonNegativeSafeInteger(value.byteLength, `${key}.byteLength`);
	if (value.byteLength === 0)
		throw new Error(`${key}.byteLength must be positive`);
	requireSha256(value.sha256, `${key}.sha256`);
	const layout = ASSET_LAYOUT[key];
	const expectedFile = `${layout.stem}-${value.sha256}${layout.extension}`;
	if (value.file !== expectedFile) {
		throw new Error(`${key}.file is not the content-addressed asset name`);
	}
	return {
		byteLength: value.byteLength,
		file: expectedFile,
		sha256: value.sha256,
	};
}

function admitRootfsEvidence(value: unknown): V86RootfsEvidence {
	requireRecord(
		value,
		["blobCount", "blobSetIdentity", "indexSha256", "schemaVersion"],
		"V86 rootfs evidence",
	);
	if (value.schemaVersion !== 1) {
		throw new Error("V86 rootfs evidence schema must be 1");
	}
	requireNonNegativeSafeInteger(value.blobCount, "rootfs.blobCount");
	if (value.blobCount === 0) {
		throw new Error("rootfs.blobCount must be positive");
	}
	requireSha256(value.blobSetIdentity, "rootfs.blobSetIdentity");
	requireSha256(value.indexSha256, "rootfs.indexSha256");
	return {
		blobCount: value.blobCount,
		blobSetIdentity: value.blobSetIdentity,
		indexSha256: value.indexSha256,
		schemaVersion: 1,
	};
}

function admitBootIdentityInput(
	value: Record<string, unknown>,
): BootIdentityInput {
	if (value.schemaVersion !== 1) {
		throw new Error("V86 boot manifest schema must be 1");
	}
	requireRecord(
		value.assets,
		Object.keys(ASSET_LAYOUT),
		"V86 boot manifest assets",
	);
	const assets = {
		libv86: admitAsset(value.assets.libv86, "libv86"),
		v86Wasm: admitAsset(value.assets.v86Wasm, "v86Wasm"),
		seabios: admitAsset(value.assets.seabios, "seabios"),
		vgabios: admitAsset(value.assets.vgabios, "vgabios"),
	};
	requireRecord(
		value.rootfs,
		["blobBaseUrl", "evidence", "indexFile"],
		"V86 boot manifest rootfs",
	);
	const evidence = admitRootfsEvidence(value.rootfs.evidence);
	if (value.rootfs.blobBaseUrl !== "rootfs-flat/") {
		throw new Error("V86 rootfs blobBaseUrl must be rootfs-flat/");
	}
	const indexFile = `rootfs-${evidence.indexSha256}.json`;
	if (value.rootfs.indexFile !== indexFile) {
		throw new Error("V86 rootfs indexFile is not content addressed");
	}
	const contract = admitV86BootContract(value.contract);
	if (!sameV86BootContract(contract)) {
		throw new Error("V86 boot manifest contract does not match this web build");
	}
	return {
		assets,
		contract,
		rootfs: {
			blobBaseUrl: "rootfs-flat/",
			evidence,
			indexFile,
		},
		schemaVersion: 1,
	};
}

export function canonicalV86BootJson(value: unknown): string {
	if (
		value === null ||
		typeof value === "boolean" ||
		typeof value === "string"
	) {
		return JSON.stringify(value);
	}
	if (typeof value === "number") {
		if (!Number.isSafeInteger(value)) {
			throw new Error("boot identity contains a non-canonical number");
		}
		return String(value);
	}
	if (Array.isArray(value)) {
		return `[${value.map(canonicalV86BootJson).join(",")}]`;
	}
	if (isRecord(value)) {
		return `{${Object.keys(value)
			.sort()
			.map(
				(key) => `${JSON.stringify(key)}:${canonicalV86BootJson(value[key])}`,
			)
			.join(",")}}`;
	}
	throw new Error("boot identity contains a non-JSON value");
}

function toHex(bytes: ArrayBuffer): string {
	return Array.from(new Uint8Array(bytes), (byte) =>
		byte.toString(16).padStart(2, "0"),
	).join("");
}

export async function computeV86BootIdentity(
	value: BootIdentityInput,
): Promise<string> {
	const bytes = new TextEncoder().encode(
		BOOT_IDENTITY_DOMAIN + canonicalV86BootJson(value),
	);
	return toHex(await crypto.subtle.digest("SHA-256", bytes));
}

export async function admitV86BootManifest(
	value: unknown,
): Promise<V86BootManifest> {
	requireRecord(
		value,
		["assets", "bootIdentity", "contract", "rootfs", "schemaVersion"],
		"V86 boot manifest",
	);
	requireSha256(value.bootIdentity, "V86 bootIdentity");
	const identityInput = admitBootIdentityInput(value);
	const expectedIdentity = await computeV86BootIdentity(identityInput);
	if (value.bootIdentity !== expectedIdentity) {
		throw new Error("V86 boot manifest identity does not match its contents");
	}
	return { ...identityInput, bootIdentity: expectedIdentity };
}

export async function fetchV86BootManifest(
	url: string,
	signal: AbortSignal,
	fetcher: V86ManifestFetcher = fetch,
): Promise<V86BootManifest> {
	const download = new AbortController();
	const forwardAbort = () => download.abort(signal.reason);
	if (signal.aborted) forwardAbort();
	else signal.addEventListener("abort", forwardAbort, { once: true });
	try {
		const response = await fetcher(url, {
			cache: "no-store",
			signal: download.signal,
		});
		if (!response.ok) {
			const error = new Error(
				`failed to download V86 boot manifest: HTTP ${response.status}`,
			);
			await abortResponse(download, response, error);
			throw error;
		}
		const bytes = await readBoundedManifestBody(response, download);
		let text: string;
		try {
			text = new TextDecoder("utf-8", { fatal: true }).decode(bytes);
		} catch {
			throw new Error("V86 boot manifest is not valid UTF-8");
		}
		let parsed: unknown;
		try {
			parsed = JSON.parse(text);
		} catch {
			throw new Error("V86 boot manifest is not valid JSON");
		}
		return admitV86BootManifest(parsed);
	} finally {
		signal.removeEventListener("abort", forwardAbort);
	}
}

function manifestSizeError(): Error {
	return new Error("V86 boot manifest exceeds the configured size limit");
}

async function abortResponse(
	controller: AbortController,
	response: Response,
	error: Error,
): Promise<void> {
	controller.abort(error);
	try {
		await response.body?.cancel(error);
	} catch {
		// Aborting the fetch is authoritative; cancellation is best-effort cleanup.
	}
}

function declaredContentLength(response: Response): number | null {
	const contentLength = response.headers.get("content-length");
	if (contentLength === null) return null;
	if (!/^(?:0|[1-9][0-9]*)$/.test(contentLength)) {
		throw new Error("V86 boot manifest has an invalid Content-Length");
	}
	const length = Number(contentLength);
	if (!Number.isSafeInteger(length)) {
		throw new Error("V86 boot manifest has an invalid Content-Length");
	}
	return length;
}

async function readBoundedManifestBody(
	response: Response,
	download: AbortController,
): Promise<Uint8Array> {
	let contentLength: number | null;
	try {
		contentLength = declaredContentLength(response);
	} catch (error) {
		const admittedError =
			error instanceof Error ? error : new Error(String(error));
		await abortResponse(download, response, admittedError);
		throw admittedError;
	}
	if (contentLength !== null && contentLength > MAX_BOOT_MANIFEST_BYTES) {
		const error = manifestSizeError();
		await abortResponse(download, response, error);
		throw error;
	}
	if (!response.body) return new Uint8Array();

	const reader = response.body.getReader();
	const chunks: Uint8Array[] = [];
	let byteLength = 0;
	try {
		while (true) {
			const { done, value } = await reader.read();
			if (done) break;
			if (byteLength + value.byteLength > MAX_BOOT_MANIFEST_BYTES) {
				const error = manifestSizeError();
				download.abort(error);
				try {
					await reader.cancel(error);
				} catch {
					// The controller abort above already terminated the fetch.
				}
				throw error;
			}
			chunks.push(value);
			byteLength += value.byteLength;
		}
	} finally {
		reader.releaseLock();
	}

	const body = new Uint8Array(byteLength);
	let offset = 0;
	for (const chunk of chunks) {
		body.set(chunk, offset);
		offset += chunk.byteLength;
	}
	return body;
}

export function v86BootAssetUrl(
	baseUrl: string,
	manifest: V86BootManifest,
	key: V86AssetKey,
): string {
	return new URL(`assets/${manifest.assets[key].file}`, baseUrl).href;
}

export function v86RootfsIndexUrl(
	baseUrl: string,
	manifest: V86BootManifest,
): string {
	return new URL(`assets/${manifest.rootfs.indexFile}`, baseUrl).href;
}

export function v86RootfsBlobBaseUrl(
	baseUrl: string,
	manifest: V86BootManifest,
): string {
	return new URL(`assets/${manifest.rootfs.blobBaseUrl}`, baseUrl).href;
}

export { V86_BOOT_CONTRACT };
