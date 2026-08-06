const { execFileSync } = require("child_process");
const fs = require("fs");
const path = require("path");
const { sources, Compilation } = require("webpack");
const CopyWebpackPlugin = require("copy-webpack-plugin");
const FaviconsWebpackPlugin = require("favicons-webpack-plugin");
const HtmlWebpackPlugin = require("html-webpack-plugin");
const MonacoWebpackPlugin = require("monaco-editor-webpack-plugin");
const sveltePreprocess = require("svelte-preprocess");
const {
	ASSET_LAYOUT,
	createV86BootManifest,
	validateRootfsPublication,
	verifyEmittedV86BootAssets,
} = require("./v86-boot-manifest-build");

const compilerHarness = process.env.GORS_WEB_COMPILER_HARNESS === "1";

const v86BuildDir = path.resolve(__dirname, "node_modules/v86/build");
const biosDir = path.resolve(__dirname, "v86/bios");
const v86DistDir = path.resolve(__dirname, "v86/dist");
const copyPatterns = [];
const devServerLiveReload = process.env.GORS_WEB_LIVE_RELOAD !== "0";
const webpackSourceMaps = process.env.GORS_WEB_SOURCE_MAPS === "1";
let bootManifest = null;

if (!compilerHarness) {
	const assetPaths = {
		libv86: path.join(v86BuildDir, ASSET_LAYOUT.libv86.sourceName),
		v86Wasm: path.join(v86BuildDir, ASSET_LAYOUT.v86Wasm.sourceName),
		seabios: path.join(biosDir, ASSET_LAYOUT.seabios.sourceName),
		vgabios: path.join(biosDir, ASSET_LAYOUT.vgabios.sourceName),
	};
	const rootfsManifestPath = path.join(v86DistDir, "manifest.json");
	const rootfsProviderPath = path.join(v86DistDir, "runtime-provider.json");
	const rootfsIndexPath = path.join(v86DistDir, "rootfs.json");
	const rootfsBlobDirectory = path.join(v86DistDir, "rootfs-flat");
	const rootfsPublication = JSON.parse(
		fs.readFileSync(rootfsManifestPath, "utf8"),
	);
	const bootContract = JSON.parse(
		fs.readFileSync(path.resolve(__dirname, "v86/boot-contract.json"), "utf8"),
	);
	validateRootfsPublication(rootfsPublication);
	execFileSync(
		"python3",
		[
			path.resolve(__dirname, "v86/tools/verify-manifest.py"),
			rootfsPublication.inputDigest,
			rootfsManifestPath,
			rootfsProviderPath,
			rootfsIndexPath,
			rootfsBlobDirectory,
			"verbose",
		],
		{ stdio: "inherit" },
	);
	bootManifest = createV86BootManifest({
		assetPaths,
		bootContract,
		rootfsPublication,
	});
	for (const key of Object.keys(assetPaths)) {
		copyPatterns.push({
			from: assetPaths[key],
			to: `assets/${bootManifest.assets[key].file}`,
			// These content-addressed inputs are already final boot assets. In
			// particular, production Terser must not rewrite libv86.js after its
			// filename and manifest hash have been computed.
			info: { minimized: true },
		});
	}
	copyPatterns.push(
		{
			from: rootfsIndexPath,
			to: `assets/${bootManifest.rootfs.indexFile}`,
		},
		{
			from: rootfsBlobDirectory,
			to: "assets/rootfs-flat/",
		},
	);
}

class BootManifestPlugin {
	apply(compiler) {
		compiler.hooks.thisCompilation.tap("BootManifestPlugin", (compilation) => {
			compilation.hooks.processAssets.tap(
				{
					name: "BootManifestPlugin",
					stage: Compilation.PROCESS_ASSETS_STAGE_SUMMARIZE,
				},
				() => {
					if (!bootManifest) {
						throw new Error("V86 boot manifest is unavailable");
					}
					const requiredAssets = new Set([
						...Object.values(bootManifest.assets).map(
							(asset) => `assets/${asset.file}`,
						),
						`assets/${bootManifest.rootfs.indexFile}`,
					]);
					const emittedAssets = new Map(
						compilation
							.getAssets()
							.filter(
								({ name }) =>
									requiredAssets.has(name) ||
									name.startsWith("assets/rootfs-flat/"),
							)
							.map(({ name, source }) => [name, source.buffer()]),
					);
					verifyEmittedV86BootAssets(bootManifest, emittedAssets);
					const json = `${JSON.stringify(bootManifest, null, 2)}\n`;
					compilation.emitAsset(
						"assets/boot-manifest.json",
						new sources.RawSource(json),
					);
				},
			);
		});
	}
}

module.exports = () => {
	const plugins = compilerHarness
		? [
				new HtmlWebpackPlugin({
					template: "tests/compiler/harness.html",
					filename: "index.html",
				}),
			]
		: [
				new CopyWebpackPlugin({ patterns: copyPatterns }),
				new BootManifestPlugin(),
				new FaviconsWebpackPlugin("./favicon.png"),
				new HtmlWebpackPlugin({
					template: "index.html",
					filename: "index.html",
				}),
				new HtmlWebpackPlugin({
					template: "index.html",
					filename: "conformance/index.html",
				}),
				new HtmlWebpackPlugin({
					template: "index.html",
					filename: "playground/index.html",
				}),
				new HtmlWebpackPlugin({ template: "index.html", filename: "404.html" }),
				new MonacoWebpackPlugin({
					languages: ["go", "rust"],
				}),
			];

	return {
		entry: compilerHarness ? "./tests/compiler/harness.ts" : "./src/main.ts",
		devtool: webpackSourceMaps ? "source-map" : false,
		output: {
			filename: "bundle-[contenthash:16].js",
			path: path.resolve(__dirname, "dist"),
			clean: true,
		},
		resolve: {
			extensions: [".mjs", ".ts", ".js", ".svelte"],
			mainFields: ["svelte", "browser", "module", "main"],
			conditionNames: ["svelte", "browser", "import"],
			fallback: {
				fs: false,
				path: false,
			},
		},
		module: {
			rules: [
				{
					test: /\.ts$/,
					exclude: /node_modules/,
					use: {
						loader: "ts-loader",
						options: {
							transpileOnly: true,
						},
					},
				},
				{
					test: /\.svelte$/,
					use: {
						loader: "svelte-loader",
						options: {
							emitCss: false,
							hotReload: false,
							preprocess: sveltePreprocess(),
						},
					},
				},
				{
					test: /\.css$/i,
					use: ["style-loader", "css-loader"],
				},
				{
					test: /\.(ttf|woff2?)$/,
					type: "asset/resource",
				},
				{
					test: /\.go$/,
					type: "asset/source",
				},
				{
					test: /\.bin\.gz$/,
					type: "asset/resource",
					generator: {
						filename: "assets/[name]-[contenthash:16][ext]",
					},
				},
				{
					test: /node_modules\/svelte\/.*\.mjs$/,
					resolve: { fullySpecified: false },
				},
			],
		},
		plugins,
		devServer: {
			allowedHosts: ["127.0.0.1", "localhost"],
			static: {
				directory: path.resolve(__dirname, "dist"),
				watch: devServerLiveReload,
			},
			compress: true,
			port: 8080,
			historyApiFallback: true,
			hot: false,
			liveReload: devServerLiveReload,
			watchFiles: devServerLiveReload ? ["src/**/*", "index.html"] : [],
		},
		experiments: {
			asyncWebAssembly: true,
		},
		performance: {
			hints: false,
		},
	};
};
