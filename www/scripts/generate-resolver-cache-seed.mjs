import { Buffer } from "node:buffer";
import { createHash } from "node:crypto";
import { mkdir, readFile, rename, stat, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { performance } from "node:perf_hooks";
import { gzipSync } from "node:zlib";

const SEED_FORMAT_VERSION = 1;
const MAX_ARCHIVE_BYTES = 64 * 1024 * 1024;
const scriptDirectory = dirname(fileURLToPath(import.meta.url));
const wwwDirectory = resolve(scriptDirectory, "..");
const wasmDirectory = resolve(wwwDirectory, "wasm", "pkg");
const wasmPath = resolve(wasmDirectory, "gors_bg.wasm");
const bindingsPath = resolve(wasmDirectory, "gors_bg.js");
const sourcePath = resolve(wwwDirectory, "default-playground.go");
const generatedDirectory = resolve(wwwDirectory, "generated");
const seedPath = resolve(
	generatedDirectory,
	`resolver-cache-seed-v${SEED_FORMAT_VERSION}.bin`,
);
const packagedSeedPath = `${seedPath}.gz`;
const metadataPath = resolve(
	generatedDirectory,
	`resolver-cache-seed-v${SEED_FORMAT_VERSION}.json`,
);

function sha256(bytes) {
	return createHash("sha256").update(bytes).digest("hex");
}

function compressArchive(bytes) {
	const compressed = gzipSync(bytes, { level: 9 });
	// Normalize the gzip OS byte so identical archives package identically on
	// Linux and macOS builders.
	compressed[9] = 0xff;
	return compressed;
}

async function readExistingMetadata() {
	try {
		return JSON.parse(await readFile(metadataPath, "utf8"));
	} catch {
		return null;
	}
}

async function reusableArchive(wasmSha256, sourceSha256) {
	const metadata = await readExistingMetadata();
	if (
		metadata?.formatVersion !== SEED_FORMAT_VERSION ||
		metadata.wasmSha256 !== wasmSha256 ||
		metadata.sourceSha256 !== sourceSha256
	) {
		return null;
	}

	try {
		const seedStats = await stat(seedPath);
		if (
			seedStats.size !== metadata.archiveBytes ||
			seedStats.size > MAX_ARCHIVE_BYTES
		) {
			return null;
		}
		const bytes = await readFile(seedPath);
		return sha256(bytes) === metadata.archiveSha256
			? { bytes, metadata }
			: null;
	} catch {
		return null;
	}
}

async function instantiateCompiler(wasmBytes) {
	const bindings = await import(
		`${pathToFileURL(bindingsPath).href}?seed-format=${SEED_FORMAT_VERSION}`
	);
	const { instance } = await WebAssembly.instantiate(wasmBytes, {
		"./gors_bg.js": bindings,
	});
	bindings.__wbg_set_wasm(instance.exports);
	instance.exports.__wbindgen_start?.();
	return bindings;
}

async function writeAtomically(path, bytes) {
	const temporaryPath = `${path}.${process.pid}.tmp`;
	await writeFile(temporaryPath, bytes);
	await rename(temporaryPath, path);
}

const startedAt = performance.now();
const [wasmBytes, source] = await Promise.all([
	readFile(wasmPath),
	readFile(sourcePath, "utf8"),
]);
const sampleSource = source.trimEnd();
const wasmSha256 = sha256(wasmBytes);
const sourceSha256 = sha256(sampleSource);

await mkdir(generatedDirectory, { recursive: true });
const reusable = await reusableArchive(wasmSha256, sourceSha256);
if (reusable) {
	const compressed = compressArchive(reusable.bytes);
	const compressedSha256 = sha256(compressed);
	let packagedIsCurrent = false;
	try {
		const packagedStats = await stat(packagedSeedPath);
		packagedIsCurrent =
			packagedStats.size === reusable.metadata.compressedBytes &&
			reusable.metadata.compressedSha256 === compressedSha256 &&
			sha256(await readFile(packagedSeedPath)) === compressedSha256;
	} catch {
		// The raw archive is still reusable; only the packaged asset is missing.
	}
	if (!packagedIsCurrent) await writeAtomically(packagedSeedPath, compressed);
	const metadata = {
		...reusable.metadata,
		compressedSha256,
		compressedBytes: compressed.byteLength,
	};
	await writeAtomically(metadataPath, `${JSON.stringify(metadata, null, 2)}\n`);
	console.log(
		`resolver cache seed is current (${metadata.compressedBytes} compressed / ${metadata.archiveBytes} bytes, ${metadata.entries} modules, ${metadata.typeEnvs} type environments)`,
	);
	process.exit(0);
}

const compiler = await instantiateCompiler(wasmBytes);
const compileStartedAt = performance.now();
const result = compiler.build_rust(sampleSource);
const compileMs = performance.now() - compileStartedAt;
try {
	if (!result.success) {
		throw new Error(
			`default playground sample did not compile: ${result.error_message}`,
		);
	}
} finally {
	result.free();
}

const archive = compiler.export_resolver_cache();
if (archive.byteLength === 0) {
	throw new Error("resolver cache seed is empty");
}
if (archive.byteLength > MAX_ARCHIVE_BYTES) {
	throw new Error(
		`resolver cache seed is ${archive.byteLength} bytes, exceeding the ${MAX_ARCHIVE_BYTES}-byte production cap`,
	);
}

const decoded = JSON.parse(new TextDecoder().decode(archive));
if (
	!Number.isInteger(decoded.schema) ||
	decoded.schema <= 0 ||
	!Array.isArray(decoded.entries) ||
	!Array.isArray(decoded.typeEnvs) ||
	decoded.entries.length === 0
) {
	throw new Error("resolver cache seed has an unexpected archive shape");
}

const archiveBytes = Buffer.from(
	archive.buffer,
	archive.byteOffset,
	archive.byteLength,
);
const compressed = compressArchive(archiveBytes);
const metadata = {
	formatVersion: SEED_FORMAT_VERSION,
	archiveSchema: decoded.schema,
	wasmSha256,
	sourceSha256,
	archiveSha256: sha256(archiveBytes),
	archiveBytes: archiveBytes.byteLength,
	compressedSha256: sha256(compressed),
	compressedBytes: compressed.byteLength,
	entries: decoded.entries.length,
	typeEnvs: decoded.typeEnvs.length,
	goVersion: decoded.goVersion,
	stdlibVersion: decoded.stdlibVersion,
	compilerFingerprint: decoded.compilerFingerprint,
};

await writeAtomically(seedPath, archiveBytes);
await writeAtomically(packagedSeedPath, compressed);
await writeAtomically(metadataPath, `${JSON.stringify(metadata, null, 2)}\n`);

console.log(
	[
		`generated ${packagedSeedPath}`,
		`${compressed.byteLength} compressed / ${archiveBytes.byteLength} bytes`,
		`${metadata.entries} modules`,
		`${metadata.typeEnvs} type environments`,
		`compile ${(compileMs / 1_000).toFixed(2)}s`,
		`total ${((performance.now() - startedAt) / 1_000).toFixed(2)}s`,
	].join(" | "),
);
