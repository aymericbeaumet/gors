import { defineConfig, devices } from "@playwright/test";

const webTestPort = process.env.GORS_WEB_TEST_PORT ?? "18080";
const webTestUrl = `http://127.0.0.1:${webTestPort}`;

export default defineConfig({
	testDir: "./tests/e2e",
	timeout: 10 * 60 * 1000,
	expect: {
		timeout: 60 * 1000,
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
		command: `npm run serve:e2e -- --port ${webTestPort}`,
		env: {
			GORS_WEB_LIVE_RELOAD: "0",
		},
		url: webTestUrl,
		reuseExistingServer: process.env.PLAYWRIGHT_REUSE_EXISTING_SERVER === "1",
		timeout: 15 * 60 * 1000,
	},
});
