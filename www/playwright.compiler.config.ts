import { defineConfig, devices } from "@playwright/test";

const webTestPort = process.env.GORS_WEB_COMPILER_TEST_PORT ?? "18081";
const webTestUrl = `http://127.0.0.1:${webTestPort}`;
const threadedWasmPreview = process.env.GORS_WASM_THREADS === "1";
const compilerArtifactsArePrepared =
	process.env.GORS_WEB_COMPILER_PREBUILT === "1";
const serverScript = threadedWasmPreview
	? "serve:e2e:compiler:threads"
	: compilerArtifactsArePrepared
		? "serve:e2e:compiler:prepared"
		: "serve:e2e:compiler";

export default defineConfig({
	testDir: "./tests/compiler",
	timeout: 3 * 60 * 1000,
	expect: {
		timeout: 30 * 1000,
	},
	use: {
		baseURL: webTestUrl,
		trace: "retain-on-failure",
	},
	projects: [
		{
			name: "chromium",
			use: { ...devices["Desktop Chrome"] },
		},
	],
	webServer: {
		command: `npm run ${serverScript} -- --port ${webTestPort}`,
		env: {
			GORS_WEB_LIVE_RELOAD: "0",
			GORS_WASM_THREADS: threadedWasmPreview ? "1" : "0",
		},
		url: webTestUrl,
		reuseExistingServer: process.env.PLAYWRIGHT_REUSE_EXISTING_SERVER === "1",
		timeout: 15 * 60 * 1000,
	},
});
