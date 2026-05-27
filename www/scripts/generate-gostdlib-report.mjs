import { access, readdir, readFile, stat, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(scriptDir, "..", "..");
const fixtureRoot = path.join(
	repoRoot,
	"gors",
	"tests",
	"fixtures",
	"go_stdlib",
);
const outputFile = path.join(scriptDir, "..", "src", "gostdlib-coverage.ts");

const goosNames = new Set([
	"aix",
	"android",
	"darwin",
	"dragonfly",
	"freebsd",
	"hurd",
	"illumos",
	"ios",
	"js",
	"linux",
	"netbsd",
	"openbsd",
	"plan9",
	"solaris",
	"wasip1",
	"windows",
]);
const goarchNames = new Set([
	"386",
	"amd64",
	"arm",
	"arm64",
	"loong64",
	"mips",
	"mips64",
	"mips64le",
	"mipsle",
	"ppc64",
	"ppc64le",
	"riscv64",
	"s390x",
	"wasm",
	"gors",
]);
const builtinNames = [
	"any",
	"append",
	"cap",
	"clear",
	"close",
	"complex",
	"copy",
	"delete",
	"imag",
	"len",
	"make",
	"max",
	"min",
	"new",
	"panic",
	"print",
	"println",
	"real",
	"recover",
];

function defaultImportName(importPath) {
	return importPath.slice(importPath.lastIndexOf("/") + 1);
}

function escapeRegExp(value) {
	return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

async function pathExists(candidate) {
	try {
		await access(candidate);
		return true;
	} catch {
		return false;
	}
}

async function readGoVersion() {
	return (await readFile(path.join(repoRoot, ".go-version"), "utf8")).trim();
}

function hostGoos() {
	if (process.env.GOOS) return process.env.GOOS;
	return process.platform === "darwin" ? "darwin" : process.platform;
}

function hostGoarch() {
	if (process.env.GOARCH) return process.env.GOARCH;
	if (process.arch === "x64") return "amd64";
	return process.arch;
}

async function findNamedDirs(root, name, depth = 0) {
	if (depth > 6 || !(await pathExists(root))) return [];
	const result = [];
	for (const entry of await readdir(root, { withFileTypes: true })) {
		if (!entry.isDirectory()) continue;
		const candidate = path.join(root, entry.name);
		if (entry.name === name) {
			result.push(candidate);
			continue;
		}
		result.push(...(await findNamedDirs(candidate, name, depth + 1)));
	}
	return result;
}

async function findStdlibSourceRoot(goVersion) {
	if (process.env.GORS_STDLIB_SRC_PATH) {
		return {
			path: process.env.GORS_STDLIB_SRC_PATH,
			filteredByBuild: false,
		};
	}

	const candidates = await findNamedDirs(
		path.join(repoRoot, "target"),
		"go_stdlib_src",
	);
	const ranked = [];
	for (const candidate of candidates) {
		const markerPath = path.join(path.dirname(candidate), "go_stdlib.version");
		const marker = (await pathExists(markerPath))
			? await readFile(markerPath, "utf8")
			: "";
		const candidateStat = await stat(candidate);
		ranked.push({
			path: candidate,
			filteredByBuild: true,
			matchesVersion: marker.includes(`gostdlib${goVersion}`),
			mtimeMs: candidateStat.mtimeMs,
		});
	}
	ranked.sort((left, right) => {
		if (left.matchesVersion !== right.matchesVersion) {
			return left.matchesVersion ? -1 : 1;
		}
		return right.mtimeMs - left.mtimeMs;
	});
	if (ranked[0]) return ranked[0];

	const cargoHome = process.env.CARGO_HOME ?? path.join(os.homedir(), ".cargo");
	const sdkSource = path.join(
		cargoHome,
		"gors-cache",
		`go${goVersion}.${hostGoos()}-${hostGoarch()}`,
		"go",
		"src",
	);
	if (await pathExists(sdkSource)) {
		return {
			path: sdkSource,
			filteredByBuild: false,
		};
	}

	throw new Error(
		"Cannot find go_stdlib_src. Run `cargo build -p gors` before generating the report.",
	);
}

function parseImports(source) {
	const imports = [];
	const importBlockPattern = /import\s*\(([\s\S]*?)\)/g;
	let sourceWithoutBlocks = source;

	for (const match of source.matchAll(importBlockPattern)) {
		const block = match[1];
		sourceWithoutBlocks = sourceWithoutBlocks.replace(match[0], "");
		for (const line of block.split("\n")) {
			const cleaned = line.replace(/\/\/.*$/, "").trim();
			const importMatch = cleaned.match(/^(?:(\w+|\.|_)\s+)?"([^"]+)"/);
			if (!importMatch) continue;
			const importPath = importMatch[2];
			imports.push({
				name: importMatch[1] ?? defaultImportName(importPath),
				path: importPath,
			});
		}
	}

	const singleImportPattern = /import\s+(?:(\w+|\.|_)\s+)?"([^"]+)"/g;
	for (const match of sourceWithoutBlocks.matchAll(singleImportPattern)) {
		const importPath = match[2];
		imports.push({
			name: match[1] ?? defaultImportName(importPath),
			path: importPath,
		});
	}

	return imports.filter((item) => item.name !== "_" && item.name !== ".");
}

function isExported(name) {
	return /^[A-Z]/.test(name);
}

function receiverTypeName(receiver) {
	const parts = receiver.trim().split(/\s+/);
	const rawType = (parts.length === 1 ? parts[0] : parts.at(-1)) ?? "";
	return rawType
		.replace(/^\*/, "")
		.replace(/\[.*\]$/, "")
		.replace(/^.*\./, "");
}

function addSymbol(symbolsByPackage, packagePath, name, kind) {
	if (!name) return;
	let symbols = symbolsByPackage.get(packagePath);
	if (!symbols) {
		symbols = new Map();
		symbolsByPackage.set(packagePath, symbols);
	}
	const existing = symbols.get(name);
	if (existing) {
		if (existing.kind === "usage" && kind !== "usage") existing.kind = kind;
		return;
	}
	symbols.set(name, {
		name,
		kind,
		fixtures: new Set(),
	});
}

function parseExportedSymbols(source) {
	const symbols = [];
	let groupKind = null;

	for (const rawLine of source.split("\n")) {
		const line = rawLine.replace(/\/\/.*$/, "").trim();
		if (!line) continue;

		if (groupKind) {
			if (line === ")") {
				groupKind = null;
				continue;
			}
			const match = line.match(/^([A-Z][A-Za-z0-9_]*)\b/);
			if (match) symbols.push({ name: match[1], kind: groupKind });
			continue;
		}

		const groupStart = line.match(/^(const|var)\s*\($/);
		if (groupStart) {
			groupKind = groupStart[1];
			continue;
		}

		const methodMatch = line.match(
			/^func\s+\(([^)]*)\)\s+([A-Z][A-Za-z0-9_]*)(?:\[[^\]]+\])?\s*\(/,
		);
		if (methodMatch) {
			const receiverType = receiverTypeName(methodMatch[1]);
			if (isExported(receiverType)) {
				symbols.push({
					name: `${receiverType}.${methodMatch[2]}`,
					kind: "method",
				});
			}
			continue;
		}

		const funcMatch = line.match(
			/^func\s+([A-Z][A-Za-z0-9_]*)(?:\[[^\]]+\])?\s*\(/,
		);
		if (funcMatch) {
			symbols.push({ name: funcMatch[1], kind: "func" });
			continue;
		}

		const typeMatch = line.match(/^type\s+([A-Z][A-Za-z0-9_]*)\b/);
		if (typeMatch) {
			symbols.push({ name: typeMatch[1], kind: "type" });
			continue;
		}

		const constMatch = line.match(/^const\s+([A-Z][A-Za-z0-9_]*)\b/);
		if (constMatch) {
			symbols.push({ name: constMatch[1], kind: "const" });
			continue;
		}

		const varMatch = line.match(/^var\s+([A-Z][A-Za-z0-9_]*)\b/);
		if (varMatch) symbols.push({ name: varMatch[1], kind: "var" });
	}

	return symbols;
}

async function collectStdlibSourceFiles(root, goVersion, filteredByBuild) {
	const targetGoos = hostGoos();
	const result = new Map();

	async function walk(dir) {
		for (const entry of await readdir(dir, { withFileTypes: true })) {
			const candidate = path.join(dir, entry.name);
			if (entry.isDirectory()) {
				if (
					entry.name === "testdata" ||
					entry.name === "vendor" ||
					entry.name === "cmd" ||
					entry.name.startsWith(".")
				) {
					continue;
				}
				await walk(candidate);
				continue;
			}
			if (!entry.name.endsWith(".go") || entry.name.endsWith("_test.go")) {
				continue;
			}
			const packagePath = path
				.relative(root, path.dirname(candidate))
				.split(path.sep)
				.join("/");
			if (!packagePath) continue;
			const source = await readFile(candidate, "utf8");
			if (
				!filteredByBuild &&
				!shouldCompileFile(entry.name, source, goVersion, targetGoos)
			) {
				continue;
			}
			const files = result.get(packagePath) ?? [];
			files.push(source);
			result.set(packagePath, files);
		}
	}

	await walk(root);
	return result;
}

function addStdlibSymbols(symbolsByPackage, packageSources) {
	for (const [packagePath, sources] of packageSources) {
		if (!symbolsByPackage.has(packagePath)) {
			symbolsByPackage.set(packagePath, new Map());
		}
		for (const source of sources) {
			for (const symbol of parseExportedSymbols(source)) {
				addSymbol(symbolsByPackage, packagePath, symbol.name, symbol.kind);
			}
		}
	}
	for (const builtin of builtinNames) {
		addSymbol(symbolsByPackage, "builtin", builtin, "builtin");
	}
}

function markTested(symbolsByPackage, packagePath, symbolName, fixtureName) {
	addSymbol(symbolsByPackage, packagePath, symbolName, "usage");
	const symbol = symbolsByPackage.get(packagePath)?.get(symbolName);
	if (symbol) symbol.fixtures.add(fixtureName);
}

function addImportedPackageUsage(
	symbolsByPackage,
	source,
	fixtureName,
	imports,
) {
	for (const item of imports) {
		const pointerMethodPattern = new RegExp(
			`\\(\\s*\\*\\s*${escapeRegExp(item.name)}\\.([A-Z][A-Za-z0-9_]*)\\s*\\)\\.([A-Z][A-Za-z0-9_]*)\\s*\\(`,
			"g",
		);
		for (const match of source.matchAll(pointerMethodPattern)) {
			markTested(
				symbolsByPackage,
				item.path,
				`${match[1]}.${match[2]}`,
				fixtureName,
			);
		}

		const valueMethodPattern = new RegExp(
			`\\b${escapeRegExp(item.name)}\\.([A-Z][A-Za-z0-9_]*)\\.([A-Z][A-Za-z0-9_]*)\\s*\\(`,
			"g",
		);
		for (const match of source.matchAll(valueMethodPattern)) {
			markTested(
				symbolsByPackage,
				item.path,
				`${match[1]}.${match[2]}`,
				fixtureName,
			);
		}

		const selectorPattern = new RegExp(
			`\\b${escapeRegExp(item.name)}\\.([A-Za-z_][A-Za-z0-9_]*)\\b`,
			"g",
		);
		for (const match of source.matchAll(selectorPattern)) {
			markTested(symbolsByPackage, item.path, match[1], fixtureName);
		}
	}
}

function addBuiltinUsage(symbolsByPackage, source, fixtureName) {
	for (const builtin of builtinNames) {
		const pattern =
			builtin === "any"
				? /\bany\b/g
				: new RegExp(`\\b${escapeRegExp(builtin)}\\s*\\(`, "g");
		if (pattern.test(source)) {
			markTested(symbolsByPackage, "builtin", builtin, fixtureName);
		}
	}
}

async function addFixtureUsage(symbolsByPackage) {
	const fixtureNames = await findRunnableFixtures(fixtureRoot);
	const runnableFixtureNames = [];

	for (const fixtureName of fixtureNames) {
		const mainPath = path.join(fixtureRoot, fixtureName, "main.go");
		let source;
		try {
			source = await readFile(mainPath, "utf8");
		} catch (error) {
			if (error && error.code === "ENOENT") continue;
			throw error;
		}
		runnableFixtureNames.push(fixtureName);
		addImportedPackageUsage(
			symbolsByPackage,
			source,
			fixtureName,
			parseImports(source),
		);
		if (fixtureName === "builtin") {
			addBuiltinUsage(symbolsByPackage, source, fixtureName);
		}
	}

	return runnableFixtureNames;
}

async function findRunnableFixtures(root, relative = "") {
	const entries = await readdir(path.join(root, relative), {
		withFileTypes: true,
	});
	const fixtures = [];
	for (const entry of entries) {
		if (!entry.isDirectory() || entry.name.startsWith("_")) continue;
		const nextRelative = relative ? `${relative}/${entry.name}` : entry.name;
		const mainPath = path.join(root, nextRelative, "main.go");
		if (await pathExists(mainPath)) {
			fixtures.push(nextRelative);
		}
		fixtures.push(...(await findRunnableFixtures(root, nextRelative)));
	}
	return fixtures.sort();
}

function buildReport(symbolsByPackage, fixtureNames) {
	const packages = Array.from(symbolsByPackage.entries())
		.map(([packagePath, symbols]) => {
			const packageFixtures = new Set();
			const symbolEntries = Array.from(symbols.values())
				.map((symbol) => {
					for (const fixture of symbol.fixtures) packageFixtures.add(fixture);
					return {
						name: symbol.name,
						kind: symbol.kind,
						tested: symbol.fixtures.size > 0,
						fixtures: Array.from(symbol.fixtures).sort(),
					};
				})
				.sort((left, right) => left.name.localeCompare(right.name));
			const testedSymbolCount = symbolEntries.filter(
				(symbol) => symbol.tested,
			).length;

			return {
				packagePath,
				tested: testedSymbolCount > 0,
				fixtures: Array.from(packageFixtures).sort(),
				symbolCount: symbolEntries.length,
				testedSymbolCount,
				symbols: symbolEntries,
			};
		})
		.sort((left, right) => left.packagePath.localeCompare(right.packagePath));
	const symbolCount = packages.reduce(
		(total, item) => total + item.symbolCount,
		0,
	);
	const testedSymbolCount = packages.reduce(
		(total, item) => total + item.testedSymbolCount,
		0,
	);
	const testedPackageCount = packages.filter((item) => item.tested).length;

	return {
		packages,
		summary: {
			fixtureCount: fixtureNames.length,
			packageCount: packages.length,
			testedPackageCount,
			untestedPackageCount: packages.length - testedPackageCount,
			symbolCount,
			testedSymbolCount,
			untestedSymbolCount: symbolCount - testedSymbolCount,
		},
	};
}

function emitTypescript(report) {
	return `// Generated by scripts/generate-gostdlib-report.mjs.
// Run \`npm run generate:gostdlib-report\` from www/ after editing gors/tests/fixtures/go_stdlib.

export type GostdlibCoverageKind =
\t| "builtin"
\t| "const"
\t| "func"
\t| "method"
\t| "type"
\t| "usage"
\t| "var";

export interface GostdlibCoverageSymbol {
\treadonly name: string;
\treadonly kind: GostdlibCoverageKind;
\treadonly tested: boolean;
\treadonly fixtures: readonly string[];
}

export interface GostdlibCoveragePackage {
\treadonly packagePath: string;
\treadonly tested: boolean;
\treadonly fixtures: readonly string[];
\treadonly symbolCount: number;
\treadonly testedSymbolCount: number;
\treadonly symbols: readonly GostdlibCoverageSymbol[];
}

export const gostdlibCoverage: readonly GostdlibCoveragePackage[] = ${JSON.stringify(report.packages, null, "\t")};

export const gostdlibCoverageSummary = ${JSON.stringify(report.summary, null, "\t")} as const;
`;
}

function shouldCompileFile(filename, content, goVersion, targetGoos) {
	return (
		fileNameMatchesTarget(filename, targetGoos) &&
		buildConstraintMatches(content, goVersion, targetGoos)
	);
}

function fileNameMatchesTarget(filename, targetGoos) {
	const stem = filename.replace(/\.go$/, "");
	const parts = stem.split("_");
	const last = parts.at(-1);
	if (goarchNames.has(last)) {
		if (last !== "gors") return false;
		const osPart = parts.at(-2);
		return !(goosNames.has(osPart) && osPart !== targetGoos);
	}
	return !goosNames.has(last) || last === targetGoos;
}

function tokenizeBuildExpr(expression) {
	return expression.match(/[A-Za-z0-9_.]+|&&|\|\||[!()]/g) ?? [];
}

class BuildExprParser {
	constructor(expression, goVersion, targetGoos) {
		this.tokens = tokenizeBuildExpr(expression);
		this.position = 0;
		this.goVersion = goVersion;
		this.targetGoos = targetGoos;
	}

	parse() {
		return this.parseOr();
	}

	parseOr() {
		let value = this.parseAnd();
		while (this.peek() === "||") {
			this.position += 1;
			value = this.parseAnd() || value;
		}
		return value;
	}

	parseAnd() {
		let value = this.parseUnary();
		while (this.peek() === "&&") {
			this.position += 1;
			value = this.parseUnary() && value;
		}
		return value;
	}

	parseUnary() {
		if (this.peek() === "!") {
			this.position += 1;
			return !this.parseUnary();
		}
		return this.parsePrimary();
	}

	parsePrimary() {
		const token = this.next();
		if (!token) return true;
		if (token === "(") {
			const value = this.parseOr();
			if (this.peek() === ")") this.position += 1;
			return value;
		}
		return buildTagMatches(token, this.goVersion, this.targetGoos);
	}

	peek() {
		return this.tokens[this.position];
	}

	next() {
		const token = this.peek();
		this.position += 1;
		return token;
	}
}

function buildConstraintMatches(content, goVersion, targetGoos) {
	for (const line of content.split("\n")) {
		const trimmed = line.trim();
		if (trimmed.startsWith("//go:build ")) {
			return new BuildExprParser(
				trimmed.slice("//go:build ".length),
				goVersion,
				targetGoos,
			).parse();
		}
		if (trimmed.startsWith("//") || trimmed === "") continue;
		break;
	}
	return true;
}

function buildTagMatches(tag, goVersion, targetGoos) {
	if (tag === targetGoos || tag === "gors") return true;
	if (tag === "unix") return isUnixGoos(targetGoos);
	if (tag.startsWith("go1.")) {
		const minor = Number.parseInt(tag.slice("go1.".length), 10);
		return Number.isInteger(minor) && minor <= goVersionMinor(goVersion);
	}
	return tag === "gc";
}

function goVersionMinor(version) {
	const parts = version.split(".");
	return parts[0] === "1" ? Number.parseInt(parts[1], 10) : 0;
}

function isUnixGoos(goos) {
	return [
		"aix",
		"android",
		"darwin",
		"dragonfly",
		"freebsd",
		"hurd",
		"illumos",
		"ios",
		"linux",
		"netbsd",
		"openbsd",
		"solaris",
	].includes(goos);
}

const goVersion = await readGoVersion();
const stdlibSourceRoot = await findStdlibSourceRoot(goVersion);
const symbolsByPackage = new Map();
addStdlibSymbols(
	symbolsByPackage,
	await collectStdlibSourceFiles(
		stdlibSourceRoot.path,
		goVersion,
		stdlibSourceRoot.filteredByBuild,
	),
);
const fixtureNames = await addFixtureUsage(symbolsByPackage);
await writeFile(
	outputFile,
	emitTypescript(buildReport(symbolsByPackage, fixtureNames)),
);
