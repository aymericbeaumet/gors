import { describe, expect, it } from "vitest";
import {
	loadSavedV86State,
	saveV86State,
	type SavedV86StateRecord,
	type V86StateStore,
} from "../../v86-state-cache";

const bootIdentity = "a".repeat(64);
const maxStateBytes = 1024;

class FakeStateStore implements V86StateStore {
	deletes = 0;
	getError: Error | null = null;
	putError: Error | null = null;
	value: unknown;

	async get(): Promise<unknown> {
		if (this.getError) throw this.getError;
		return this.value;
	}

	async put(value: SavedV86StateRecord): Promise<void> {
		if (this.putError) throw this.putError;
		this.value = value;
	}

	async delete(): Promise<void> {
		this.deletes += 1;
		this.value = undefined;
	}
}

function bytes(...values: number[]): ArrayBuffer {
	return new Uint8Array(values).buffer;
}

describe("V86 saved-state cache", () => {
	it("writes and admits only a bounded schema-v2 checksummed record", async () => {
		const store = new FakeStateStore();
		const state = bytes(1, 2, 3, 4);

		await saveV86State(store, bootIdentity, state, maxStateBytes);
		const record = store.value as SavedV86StateRecord;
		expect(record).toMatchObject({
			bootIdentity,
			schemaVersion: 2,
			stateByteLength: 4,
		});
		expect(record.stateSha256).toMatch(/^[0-9a-f]{64}$/);
		await expect(
			loadSavedV86State(store, bootIdentity, maxStateBytes),
		).resolves.toBe(state);
		expect(store.deletes).toBe(0);
	});

	it("deletes v1, identity-mismatched, oversized, and structurally corrupt records", async () => {
		const cases: unknown[] = [
			{ version: bootIdentity, state: bytes(1) },
			{
				bootIdentity: "b".repeat(16),
				schemaVersion: 2,
				state: bytes(1),
				stateByteLength: 1,
				stateSha256: "c".repeat(64),
			},
			{
				bootIdentity: "b".repeat(64),
				schemaVersion: 2,
				state: bytes(1),
				stateByteLength: 1,
				stateSha256: "c".repeat(64),
			},
			{
				bootIdentity,
				schemaVersion: 2,
				state: bytes(1),
				stateByteLength: maxStateBytes + 1,
				stateSha256: "c".repeat(64),
			},
			{
				bootIdentity,
				extra: true,
				schemaVersion: 2,
				state: bytes(1),
				stateByteLength: 1,
				stateSha256: "c".repeat(64),
			},
		];

		for (const value of cases) {
			const store = new FakeStateStore();
			store.value = value;
			await expect(
				loadSavedV86State(store, bootIdentity, maxStateBytes),
			).resolves.toBeNull();
			expect(store.deletes).toBe(1);
		}
	});

	it("detects payload corruption and deletes the record", async () => {
		const store = new FakeStateStore();
		await saveV86State(store, bootIdentity, bytes(1, 2, 3), maxStateBytes);
		const record = store.value as SavedV86StateRecord;
		new Uint8Array(record.state)[0] ^= 0xff;

		await expect(
			loadSavedV86State(store, bootIdentity, maxStateBytes),
		).resolves.toBeNull();
		expect(store.deletes).toBe(1);
	});

	it("treats every IndexedDB read or write failure as a cold-cache miss", async () => {
		const readFailure = new FakeStateStore();
		readFailure.getError = new Error("IDB unavailable");
		await expect(
			loadSavedV86State(readFailure, bootIdentity, maxStateBytes),
		).resolves.toBeNull();

		const writeFailure = new FakeStateStore();
		writeFailure.putError = new Error("quota exceeded");
		await expect(
			saveV86State(writeFailure, bootIdentity, bytes(1, 2, 3), maxStateBytes),
		).resolves.toBeUndefined();
	});

	it("refuses empty and oversized state before persistence", async () => {
		for (const state of [
			new ArrayBuffer(0),
			new ArrayBuffer(maxStateBytes + 1),
		]) {
			const store = new FakeStateStore();
			await saveV86State(store, bootIdentity, state, maxStateBytes);
			expect(store.value).toBeUndefined();
			expect(store.deletes).toBe(1);
		}
	});

	it("rejects invalid identities and state bounds before persistence", async () => {
		for (const [identity, bound] of [
			["a".repeat(16), maxStateBytes],
			[bootIdentity, 0],
		] as const) {
			const store = new FakeStateStore();
			await saveV86State(store, identity, bytes(1), bound);
			expect(store.value).toBeUndefined();
			expect(store.deletes).toBe(1);
		}
	});
});
