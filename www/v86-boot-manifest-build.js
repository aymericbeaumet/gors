const crypto = require("crypto");
const fs = require("fs");

const BOOT_IDENTITY_DOMAIN = "gors-v86-boot-manifest-v1\0";
const BLOB_SET_IDENTITY_DOMAIN = "gors-v86-rootfs-blob-set-v1\0";
const SHA256_PATTERN = /^[0-9a-f]{64}$/;
const ROOTFS_BLOB_PATTERN = /^([0-9a-f]{64})\.bin$/;
const ASSET_LAYOUT = Object.freeze({
	libv86: { sourceName: "libv86.js", stem: "libv86", extension: ".js" },
	v86Wasm: { sourceName: "v86.wasm", stem: "v86", extension: ".wasm" },
	seabios: { sourceName: "seabios.bin", stem: "seabios", extension: ".bin" },
	vgabios: { sourceName: "vgabios.bin", stem: "vgabios", extension: ".bin" },
});

function isRecord(value) {
	return typeof value === "object" && value !== null && !Array.isArray(value);
}

function requireExactFields(value, fields, label) {
	if (!isRecord(value)) throw new Error(`${label} must be an object`);
	const actual = Object.keys(value).sort();
	const expected = [...fields].sort();
	if (
		actual.length !== expected.length ||
		!actual.every((field, index) => field === expected[index])
	) {
		throw new Error(`${label} has an unsupported field set`);
	}
}

function requireSha256(value, label) {
	if (typeof value !== "string" || !SHA256_PATTERN.test(value)) {
		throw new Error(`${label} must be lowercase SHA-256`);
	}
}

function requirePositiveSafeInteger(value, label) {
	if (!Number.isSafeInteger(value) || value <= 0) {
		throw new Error(`${label} must be a positive safe integer`);
	}
}

function validateRootfsPublication(publication) {
	requireExactFields(
		publication,
		[
			"inputDigest",
			"rootfs",
			"runtimeProvider",
			"runtimeProviderSha256",
			"schemaVersion",
			"type",
		],
		"V86 rootfs publication",
	);
	if (publication.schemaVersion !== 1 || publication.type !== "9p") {
		throw new Error("V86 rootfs publication schema or type is unsupported");
	}
	requireSha256(publication.inputDigest, "rootfs inputDigest");
	requireSha256(
		publication.runtimeProviderSha256,
		"rootfs runtimeProviderSha256",
	);
	if (!isRecord(publication.runtimeProvider)) {
		throw new Error("rootfs runtimeProvider must be an object");
	}
	requireExactFields(
		publication.rootfs,
		["blobCount", "blobSetIdentity", "indexSha256", "schemaVersion"],
		"V86 rootfs evidence",
	);
	if (
		publication.rootfs.schemaVersion !== 1 ||
		!Number.isSafeInteger(publication.rootfs.blobCount) ||
		publication.rootfs.blobCount <= 0
	) {
		throw new Error("V86 rootfs evidence values are invalid");
	}
	requireSha256(publication.rootfs.blobSetIdentity, "rootfs blobSetIdentity");
	requireSha256(publication.rootfs.indexSha256, "rootfs indexSha256");
}

function validateBootContract(contract) {
	requireExactFields(
		contract,
		["schemaVersion", "vm", "guestProtocol"],
		"V86 boot contract",
	);
	if (contract.schemaVersion !== 1) {
		throw new Error("V86 boot contract schema must be 1");
	}
	requireExactFields(
		contract.vm,
		[
			"autostart",
			"memorySizeBytes",
			"vgaMemorySizeBytes",
			"disableKeyboard",
			"disableMouse",
			"bzimageInitrdFromFilesystem",
			"cmdline",
			"maxSavedStateBytes",
		],
		"V86 VM contract",
	);
	requireExactFields(
		contract.guestProtocol,
		[
			"schemaVersion",
			"bootReadyMarker",
			"readyMarker",
			"compileDonePrefix",
			"runDonePrefix",
			"compileCommand",
			"runCommand",
			"jobDirectory",
			"nonceHexLength",
		],
		"V86 guest protocol",
	);
	if (
		contract.guestProtocol.schemaVersion !== 1 ||
		contract.guestProtocol.nonceHexLength !== 32
	) {
		throw new Error("V86 guest protocol values are unsupported");
	}
	for (const field of [
		"autostart",
		"disableKeyboard",
		"disableMouse",
		"bzimageInitrdFromFilesystem",
	]) {
		if (typeof contract.vm[field] !== "boolean") {
			throw new Error(`V86 VM ${field} must be boolean`);
		}
	}
	requirePositiveSafeInteger(
		contract.vm.memorySizeBytes,
		"V86 VM memorySizeBytes",
	);
	requirePositiveSafeInteger(
		contract.vm.vgaMemorySizeBytes,
		"V86 VM vgaMemorySizeBytes",
	);
	requirePositiveSafeInteger(
		contract.vm.maxSavedStateBytes,
		"V86 VM maxSavedStateBytes",
	);
	if (
		typeof contract.vm.cmdline !== "string" ||
		contract.vm.cmdline.length === 0 ||
		contract.vm.maxSavedStateBytes < contract.vm.memorySizeBytes
	) {
		throw new Error("V86 VM command line or saved-state bound is invalid");
	}
	requireProtocolToken(
		contract.guestProtocol.bootReadyMarker,
		"bootReadyMarker",
	);
	requireProtocolToken(contract.guestProtocol.readyMarker, "readyMarker");
	requireProtocolToken(
		contract.guestProtocol.compileDonePrefix,
		"compileDonePrefix",
		":",
	);
	requireProtocolToken(
		contract.guestProtocol.runDonePrefix,
		"runDonePrefix",
		":",
	);
	for (const field of ["compileCommand", "runCommand", "jobDirectory"]) {
		const value = contract.guestProtocol[field];
		if (typeof value !== "string" || !/^[a-z][a-z0-9-]*$/.test(value)) {
			throw new Error(`V86 guest protocol ${field} is not canonical`);
		}
	}
}

function requireProtocolToken(value, field, suffix = "") {
	if (
		typeof value !== "string" ||
		!/^[A-Z][A-Z0-9_:]*$/.test(value) ||
		(suffix !== "" && !value.endsWith(suffix))
	) {
		throw new Error(`V86 guest protocol ${field} is not canonical`);
	}
}

function canonicalJson(value) {
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
		return `[${value.map(canonicalJson).join(",")}]`;
	}
	if (isRecord(value)) {
		return `{${Object.keys(value)
			.sort()
			.map((key) => `${JSON.stringify(key)}:${canonicalJson(value[key])}`)
			.join(",")}}`;
	}
	throw new Error("boot identity contains a non-JSON value");
}

function sha256(content) {
	return crypto.createHash("sha256").update(content).digest("hex");
}

function rootfsBlobSetIdentity(blobNames) {
	const digest = crypto.createHash("sha256");
	digest.update(BLOB_SET_IDENTITY_DOMAIN, "utf8");
	for (const name of [...blobNames].sort()) {
		if (!ROOTFS_BLOB_PATTERN.test(name)) {
			throw new Error(`V86 rootfs blob name is not content addressed: ${name}`);
		}
		const encoded = Buffer.from(name, "ascii");
		const length = Buffer.alloc(8);
		length.writeBigUInt64LE(BigInt(encoded.length));
		digest.update(length);
		digest.update(encoded);
	}
	return digest.digest("hex");
}

function requireEmittedBuffer(emittedAssets, name) {
	const content = emittedAssets.get(name);
	if (!Buffer.isBuffer(content)) {
		throw new Error(`V86 boot publication is missing emitted asset ${name}`);
	}
	return content;
}

function verifyEmittedV86BootAssets(manifest, emittedAssets) {
	if (!(emittedAssets instanceof Map)) {
		throw new Error("V86 emitted assets must be provided as a Map");
	}
	for (const key of Object.keys(ASSET_LAYOUT)) {
		const asset = manifest.assets[key];
		const name = `assets/${asset.file}`;
		const content = requireEmittedBuffer(emittedAssets, name);
		if (
			content.length !== asset.byteLength ||
			sha256(content) !== asset.sha256
		) {
			throw new Error(`emitted V86 asset does not match its manifest: ${name}`);
		}
	}

	const indexName = `assets/${manifest.rootfs.indexFile}`;
	const index = requireEmittedBuffer(emittedAssets, indexName);
	if (sha256(index) !== manifest.rootfs.evidence.indexSha256) {
		throw new Error("emitted V86 rootfs index does not match its manifest");
	}

	const prefix = "assets/rootfs-flat/";
	const blobNames = [];
	for (const [name, content] of emittedAssets) {
		if (!name.startsWith(prefix)) continue;
		const blobName = name.slice(prefix.length);
		const match = ROOTFS_BLOB_PATTERN.exec(blobName);
		if (!match || !Buffer.isBuffer(content) || sha256(content) !== match[1]) {
			throw new Error(
				`emitted V86 rootfs blob is not content addressed: ${name}`,
			);
		}
		blobNames.push(blobName);
	}
	const evidence = manifest.rootfs.evidence;
	if (
		blobNames.length !== evidence.blobCount ||
		rootfsBlobSetIdentity(blobNames) !== evidence.blobSetIdentity
	) {
		throw new Error("emitted V86 rootfs blob set does not match its manifest");
	}
}

function createAssetRecord(key, sourcePath) {
	const layout = ASSET_LAYOUT[key];
	if (!layout) throw new Error(`unsupported V86 asset ${key}`);
	const bytes = fs.readFileSync(sourcePath);
	if (bytes.length === 0) {
		throw new Error(`V86 asset ${key} must not be empty`);
	}
	const contentSha256 = sha256(bytes);
	return {
		byteLength: bytes.length,
		file: `${layout.stem}-${contentSha256}${layout.extension}`,
		sha256: contentSha256,
	};
}

function createV86BootManifest({
	assetPaths,
	bootContract,
	rootfsPublication,
}) {
	validateRootfsPublication(rootfsPublication);
	validateBootContract(bootContract);
	requireExactFields(assetPaths, Object.keys(ASSET_LAYOUT), "V86 asset paths");

	const assets = {};
	for (const key of Object.keys(ASSET_LAYOUT).sort()) {
		assets[key] = createAssetRecord(key, assetPaths[key]);
	}
	const identityInput = {
		assets,
		contract: bootContract,
		rootfs: {
			blobBaseUrl: "rootfs-flat/",
			evidence: rootfsPublication.rootfs,
			indexFile: `rootfs-${rootfsPublication.rootfs.indexSha256}.json`,
		},
		schemaVersion: 1,
	};
	const bootIdentity = sha256(
		Buffer.concat([
			Buffer.from(BOOT_IDENTITY_DOMAIN, "utf8"),
			Buffer.from(canonicalJson(identityInput), "utf8"),
		]),
	);
	return { ...identityInput, bootIdentity };
}

module.exports = {
	ASSET_LAYOUT,
	BOOT_IDENTITY_DOMAIN,
	canonicalJson,
	createV86BootManifest,
	rootfsBlobSetIdentity,
	validateRootfsPublication,
	verifyEmittedV86BootAssets,
};
