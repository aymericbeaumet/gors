import { describe, expect, it } from "vitest";
import {
	admitRuntimeDependency,
	runtimeDependencyCacheIdentity,
	runtimeDependencyRequestJson,
} from "../../runtime-dependency";

const contractIdentity = "ab".repeat(32);

function dependency(operationIds = new Uint16Array([2, 14, 16])) {
	return {
		schemaVersion: 1,
		contractIdentity,
		operationIds,
	};
}

describe("runtime dependency protocol", () => {
	it("admits the exact current schema and copies operation IDs", () => {
		const input = dependency();
		const admitted = admitRuntimeDependency(input, "unit test");

		expect(admitted).toEqual(input);
		expect(admitted.operationIds).not.toBe(input.operationIds);
		input.operationIds[0] = 1;
		expect(Array.from(admitted.operationIds)).toEqual([2, 14, 16]);
	});

	it.each([
		[{ ...dependency(), schemaVersion: 0 }, "unsupported schema 0"],
		[{ ...dependency(), schemaVersion: 2 }, "unsupported schema 2"],
		[
			{ ...dependency(), contractIdentity: contractIdentity.toUpperCase() },
			"64 lowercase hexadecimal",
		],
		[
			{ ...dependency(), contractIdentity: contractIdentity.slice(2) },
			"64 lowercase hexadecimal",
		],
		[{ ...dependency(), operationIds: [2, 14, 16] }, "must be a Uint16Array"],
		[dependency(new Uint16Array([2, 2, 16])), "strictly increasing"],
		[dependency(new Uint16Array([16, 14, 2])), "strictly increasing"],
		[{ ...dependency(), target: "i686-linux" }, "expected exactly"],
	] as const)("rejects invalid dependency %#", (value, message) => {
		expect(() => admitRuntimeDependency(value, "unit boundary")).toThrow(
			message,
		);
	});

	it("includes the dependency in cache identity", () => {
		const first = runtimeDependencyCacheIdentity("package main", dependency());
		const changedOperations = runtimeDependencyCacheIdentity(
			"package main",
			dependency(new Uint16Array([14, 16])),
		);
		const changedSource = runtimeDependencyCacheIdentity(
			"package main\n",
			dependency(),
		);

		expect(changedOperations).not.toBe(first);
		expect(changedSource).not.toBe(first);
	});

	it("writes the exact target-neutral V86 request", () => {
		expect(JSON.parse(runtimeDependencyRequestJson(dependency()))).toEqual({
			schema_version: 1,
			contract: contractIdentity,
			operation_ids: [2, 14, 16],
		});
	});
});
