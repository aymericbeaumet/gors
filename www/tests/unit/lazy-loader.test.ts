import { describe, expect, it, vi } from "vitest";
import { createRetryableLazyLoader } from "../../lazy-loader";

describe("createRetryableLazyLoader", () => {
	it("shares concurrent attempts and retries after a transient rejection", async () => {
		const expected = { compiler: "ready" };
		const load = vi
			.fn<() => Promise<typeof expected>>()
			.mockRejectedValueOnce(new Error("transient Wasm load failure"))
			.mockResolvedValue(expected);
		const loadOnceReady = createRetryableLazyLoader(load);

		const first = loadOnceReady();
		const concurrent = loadOnceReady();
		expect(first).toBe(concurrent);
		await expect(first).rejects.toThrow("transient Wasm load failure");

		const retry = await loadOnceReady();
		const admitted = await loadOnceReady();
		expect(retry).toBe(expected);
		expect(admitted).toBe(expected);
		expect(load).toHaveBeenCalledTimes(2);
	});
});
