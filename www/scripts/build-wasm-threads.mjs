import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import { gzipSync } from "node:zlib";

const TOOLCHAIN = "nightly-2026-07-01";
const RUSTFLAGS = [
	"-C target-feature=+atomics,+bulk-memory",
	"-C link-arg=--shared-memory",
	"-C link-arg=--max-memory=1073741824",
	"-C link-arg=--import-memory",
	"-C link-arg=--export=__heap_base",
	"-C link-arg=--export=__wasm_init_tls",
	"-C link-arg=--export=__tls_size",
	"-C link-arg=--export=__tls_align",
	"-C link-arg=--export=__tls_base",
].join(" ");
const resolverSeedUrl = new URL(
	"../generated/resolver-cache-seed-v1.bin.gz",
	import.meta.url,
);

function run(command, args, options = {}) {
	const result = spawnSync(command, args, {
		cwd: new URL("..", import.meta.url),
		env: process.env,
		stdio: "inherit",
		...options,
	});
	if (result.error) throw result.error;
	if (result.status !== 0) {
		throw new Error(`${command} exited with status ${result.status}`);
	}
}

function installedRustupItems(kind) {
	const result = spawnSync(
		"rustup",
		[kind, "list", "--toolchain", TOOLCHAIN, "--installed"],
		{
			cwd: new URL("..", import.meta.url),
			encoding: "utf8",
		},
	);
	if (result.error) throw result.error;
	if (result.status !== 0) {
		throw new Error(`rustup ${kind} list exited with status ${result.status}`);
	}
	return result.stdout.split(/\r?\n/);
}

if (
	!installedRustupItems("component").some((component) =>
		component.startsWith("rust-src"),
	)
) {
	throw new Error(
		`missing rust-src; run: rustup component add rust-src --toolchain ${TOOLCHAIN}`,
	);
}
if (
	!installedRustupItems("target").some((target) =>
		target.startsWith("wasm32-unknown-unknown"),
	)
) {
	throw new Error(
		`missing wasm target; run: rustup target add wasm32-unknown-unknown --toolchain ${TOOLCHAIN}`,
	);
}

run("rustup", ["run", TOOLCHAIN, "rustc", "--version"]);
run(
	"rustup",
	[
		"run",
		TOOLCHAIN,
		"wasm-pack",
		"build",
		"--release",
		"--target",
		"web",
		"--out-dir",
		"pkg-threads",
		"wasm",
		"--",
		"--features",
		"threads",
		"-Z",
		"build-std=panic_abort,std",
	],
	{
		env: {
			...process.env,
			RUSTFLAGS,
		},
	},
);
run(process.execPath, [
	"scripts/normalize-wasm-package-json.mjs",
	"pkg-threads",
]);

// Webpack resolves the optional production seed at build time. Stable and
// threaded artifacts intentionally share its scheduling-independent resolver
// ABI. Keep a tiny invalid fallback only for standalone threaded-package builds
// that have not prepared the stable seed yet.
if (!existsSync(resolverSeedUrl)) {
	mkdirSync(new URL("../generated/", import.meta.url), { recursive: true });
	writeFileSync(resolverSeedUrl, gzipSync("{}"));
}
