import { defineConfig, devices } from "@playwright/test";

export default defineConfig({
	testDir: "./tests/e2e",
	timeout: 10 * 60 * 1000,
	expect: {
		timeout: 60 * 1000,
	},
	use: {
		baseURL: "http://127.0.0.1:8080",
		trace: "retain-on-failure",
	},
	projects: [
		{
			name: "chromium",
			use: { ...devices["Desktop Chrome"] },
		},
	],
	webServer: {
		command: "npm run serve:e2e",
		url: "http://127.0.0.1:8080",
		reuseExistingServer: !process.env.CI,
		timeout: 5 * 60 * 1000,
	},
});
