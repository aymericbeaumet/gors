const crypto = require("crypto");
const fs = require("fs");
const path = require("path");
const { sources, Compilation } = require("webpack");
const CopyWebpackPlugin = require("copy-webpack-plugin");
const FaviconsWebpackPlugin = require("favicons-webpack-plugin");
const HtmlWebpackPlugin = require("html-webpack-plugin");
const MonacoWebpackPlugin = require("monaco-editor-webpack-plugin");
const sveltePreprocess = require("svelte-preprocess");

function contentHash(filePath) {
	const data = fs.readFileSync(filePath);
	return crypto.createHash("sha256").update(data).digest("hex").slice(0, 16);
}

const v86BuildDir = path.resolve(__dirname, "node_modules/v86/build");
const biosDir = path.resolve(__dirname, "v86/bios");

const staticAssets = [
	{ src: path.join(v86BuildDir, "libv86.js"), name: "libv86", ext: ".js" },
	{ src: path.join(v86BuildDir, "v86.wasm"), name: "v86", ext: ".wasm" },
	{ src: path.join(biosDir, "seabios.bin"), name: "seabios", ext: ".bin" },
	{ src: path.join(biosDir, "vgabios.bin"), name: "vgabios", ext: ".bin" },
];

const assetManifest = {};
const copyPatterns = [];

for (const { src, name, ext } of staticAssets) {
	const hash = contentHash(src);
	const hashedName = `${name}-${hash}${ext}`;
	assetManifest[`${name}${ext}`] = hashedName;
	copyPatterns.push({ from: src, to: `assets/${hashedName}` });
}

copyPatterns.push(
	{
		from: "v86/dist/rootfs.json",
		to: "assets/[name][ext]",
		noErrorOnMissing: true,
	},
	{
		from: "v86/dist/rootfs-flat/",
		to: "assets/rootfs-flat/",
		noErrorOnMissing: true,
	},
);

class AssetManifestPlugin {
	apply(compiler) {
		compiler.hooks.thisCompilation.tap("AssetManifestPlugin", (compilation) => {
			compilation.hooks.processAssets.tap(
				{
					name: "AssetManifestPlugin",
					stage: Compilation.PROCESS_ASSETS_STAGE_ADDITIONAL,
				},
				() => {
					const json = JSON.stringify(assetManifest);
					compilation.emitAsset(
						"assets/asset-manifest.json",
						new sources.RawSource(json),
					);
				},
			);
		});
	}
}

module.exports = (_, argv) => {
	const isDev = argv.mode === "development";

	return {
		entry: "./src/main.ts",
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
							hotReload: isDev,
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
					test: /node_modules\/svelte\/.*\.mjs$/,
					resolve: { fullySpecified: false },
				},
			],
		},
		plugins: [
			new CopyWebpackPlugin({ patterns: copyPatterns }),
			new AssetManifestPlugin(),
			new FaviconsWebpackPlugin("./favicon.png"),
			new HtmlWebpackPlugin({ template: "index.html", filename: "index.html" }),
			new HtmlWebpackPlugin({
				template: "index.html",
				filename: "coverage/index.html",
			}),
			new HtmlWebpackPlugin({ template: "index.html", filename: "404.html" }),
			new MonacoWebpackPlugin(),
		],
		devServer: {
			static: {
				directory: path.resolve(__dirname, "dist"),
			},
			compress: true,
			port: 8080,
			headers: {
				"Cross-Origin-Opener-Policy": "same-origin",
				"Cross-Origin-Embedder-Policy": "require-corp",
			},
			historyApiFallback: true,
			hot: true,
		},
		experiments: {
			asyncWebAssembly: true,
		},
		performance: {
			hints: false,
		},
	};
};
