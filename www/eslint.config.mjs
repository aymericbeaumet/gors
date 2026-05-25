import js from "@eslint/js";
import tseslint from "typescript-eslint";

export default [
	js.configs.recommended,
	...tseslint.configs.recommended,
	{
		ignores: [
			"dist/",
			"playwright-report/",
			"test-results/",
			"wasm/pkg/",
			"wasm/target/",
			"v86/",
			"src/*.svelte",
		],
	},
	{
		files: ["webpack.config.js"],
		languageOptions: {
			sourceType: "commonjs",
			globals: {
				require: "readonly",
				module: "writable",
				exports: "writable",
				__dirname: "readonly",
				__filename: "readonly",
				process: "readonly",
			},
		},
		rules: {
			"@typescript-eslint/no-require-imports": "off",
		},
	},
	{
		files: ["*.config.ts", "scripts/**/*.mjs"],
		languageOptions: {
			globals: {
				process: "readonly",
				console: "readonly",
				URL: "readonly",
			},
		},
	},
	{
		languageOptions: {
			ecmaVersion: "latest",
			sourceType: "module",
			globals: {
				window: "readonly",
				document: "readonly",
				console: "readonly",
				URLSearchParams: "readonly",
				URL: "readonly",
				history: "readonly",
				location: "readonly",
				setTimeout: "readonly",
				clearTimeout: "readonly",
				requestAnimationFrame: "readonly",
				ResizeObserver: "readonly",
				MutationObserver: "readonly",
				HTMLElement: "readonly",
				Event: "readonly",
				CustomEvent: "readonly",
				fetch: "readonly",
				AbortController: "readonly",
				navigator: "readonly",
				WebAssembly: "readonly",
				Worker: "readonly",
				TextDecoder: "readonly",
				atob: "readonly",
				btoa: "readonly",
				setInterval: "readonly",
				clearInterval: "readonly",
				unescape: "readonly",
				encodeURIComponent: "readonly",
				Blob: "readonly",
				Response: "readonly",
				DecompressionStream: "readonly",
				indexedDB: "readonly",
				performance: "readonly",
				self: "readonly",
				V86: "readonly",
				crypto: "readonly",
				TextEncoder: "readonly",
				Uint8Array: "readonly",
				ArrayBuffer: "readonly",
			},
		},
		rules: {
			"no-use-before-define": [
				"error",
				{ functions: false, classes: true, variables: true },
			],
			"no-unused-vars": [
				"error",
				{ argsIgnorePattern: "^_", varsIgnorePattern: "^_" },
			],
		},
	},
	{
		files: ["**/*.ts"],
		rules: {
			"no-unused-vars": "off",
			"@typescript-eslint/no-unused-vars": [
				"error",
				{ argsIgnorePattern: "^_", varsIgnorePattern: "^_" },
			],
			"@typescript-eslint/no-explicit-any": "off",
			"@typescript-eslint/no-empty-object-type": "off",
		},
	},
];
