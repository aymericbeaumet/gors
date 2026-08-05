const DATABASE_NAME = "gors-vm";
const DATABASE_VERSION = 2;
const STORE_NAME = "state";
const STATE_KEY = "vm-state";
const SHA256_PATTERN = /^[0-9a-f]{64}$/;

export interface SavedV86StateRecord {
	readonly schemaVersion: 2;
	readonly bootIdentity: string;
	readonly stateByteLength: number;
	readonly stateSha256: string;
	readonly state: ArrayBuffer;
}

export interface V86StateStore {
	get(): Promise<unknown>;
	put(value: SavedV86StateRecord): Promise<void>;
	delete(): Promise<void>;
}

function openStateDatabase(): Promise<IDBDatabase> {
	return new Promise((resolve, reject) => {
		const request = indexedDB.open(DATABASE_NAME, DATABASE_VERSION);
		let settled = false;
		request.onupgradeneeded = () => {
			const database = request.result;
			if (database.objectStoreNames.contains(STORE_NAME)) {
				database.deleteObjectStore(STORE_NAME);
			}
			database.createObjectStore(STORE_NAME);
		};
		request.onsuccess = () => {
			if (settled) {
				request.result.close();
				return;
			}
			settled = true;
			resolve(request.result);
		};
		request.onerror = () => {
			if (settled) return;
			settled = true;
			reject(request.error ?? new Error("failed to open V86 state database"));
		};
		request.onblocked = () => {
			if (settled) return;
			settled = true;
			reject(new Error("V86 state database upgrade is blocked"));
		};
	});
}

async function withDatabase<T>(
	action: (database: IDBDatabase) => Promise<T>,
): Promise<T> {
	const database = await openStateDatabase();
	try {
		return await action(database);
	} finally {
		database.close();
	}
}

function readState(database: IDBDatabase): Promise<unknown> {
	return new Promise((resolve, reject) => {
		const transaction = database.transaction(STORE_NAME, "readonly");
		const request = transaction.objectStore(STORE_NAME).get(STATE_KEY);
		request.onsuccess = () => resolve(request.result);
		request.onerror = () =>
			reject(request.error ?? new Error("failed to read V86 state"));
		transaction.onabort = () =>
			reject(transaction.error ?? new Error("V86 state read aborted"));
	});
}

function mutateState(
	database: IDBDatabase,
	action: (store: IDBObjectStore) => void,
): Promise<void> {
	return new Promise((resolve, reject) => {
		const transaction = database.transaction(STORE_NAME, "readwrite");
		action(transaction.objectStore(STORE_NAME));
		transaction.oncomplete = () => resolve();
		transaction.onerror = () =>
			reject(transaction.error ?? new Error("V86 state mutation failed"));
		transaction.onabort = () =>
			reject(transaction.error ?? new Error("V86 state mutation aborted"));
	});
}

export class IndexedDbV86StateStore implements V86StateStore {
	get(): Promise<unknown> {
		return withDatabase(readState);
	}

	put(value: SavedV86StateRecord): Promise<void> {
		return withDatabase((database) =>
			mutateState(database, (store) => store.put(value, STATE_KEY)),
		);
	}

	delete(): Promise<void> {
		return withDatabase((database) =>
			mutateState(database, (store) => store.delete(STATE_KEY)),
		);
	}
}

function isRecord(value: unknown): value is Record<string, unknown> {
	return typeof value === "object" && value !== null && !Array.isArray(value);
}

function hasExactRecordFields(value: Record<string, unknown>): boolean {
	const actual = Object.keys(value).sort();
	const expected = [
		"bootIdentity",
		"schemaVersion",
		"state",
		"stateByteLength",
		"stateSha256",
	].sort();
	return (
		actual.length === expected.length &&
		actual.every((field, index) => field === expected[index])
	);
}

function isValidStateBound(maxStateBytes: number): boolean {
	return Number.isSafeInteger(maxStateBytes) && maxStateBytes > 0;
}

function isAdmissibleRecord(
	value: unknown,
	bootIdentity: string,
	maxStateBytes: number,
): value is SavedV86StateRecord {
	if (
		!SHA256_PATTERN.test(bootIdentity) ||
		!isValidStateBound(maxStateBytes) ||
		!isRecord(value) ||
		!hasExactRecordFields(value)
	) {
		return false;
	}
	if (
		value.schemaVersion !== 2 ||
		value.bootIdentity !== bootIdentity ||
		typeof value.stateSha256 !== "string" ||
		!SHA256_PATTERN.test(value.stateSha256) ||
		!(value.state instanceof ArrayBuffer) ||
		!Number.isSafeInteger(value.stateByteLength) ||
		(value.stateByteLength as number) <= 0 ||
		(value.stateByteLength as number) > maxStateBytes
	) {
		return false;
	}
	return value.state.byteLength === value.stateByteLength;
}

function bytesToHex(buffer: ArrayBuffer): string {
	return Array.from(new Uint8Array(buffer), (byte) =>
		byte.toString(16).padStart(2, "0"),
	).join("");
}

export async function sha256ArrayBuffer(state: ArrayBuffer): Promise<string> {
	return bytesToHex(await crypto.subtle.digest("SHA-256", state));
}

async function deleteIgnoringFailure(store: V86StateStore): Promise<void> {
	try {
		await store.delete();
	} catch {
		// IndexedDB is an optimization boundary. A failed deletion remains a miss.
	}
}

export async function loadSavedV86State(
	store: V86StateStore,
	bootIdentity: string,
	maxStateBytes: number,
): Promise<ArrayBuffer | null> {
	if (!SHA256_PATTERN.test(bootIdentity) || !isValidStateBound(maxStateBytes)) {
		return null;
	}
	let value: unknown;
	try {
		value = await store.get();
	} catch {
		return null;
	}
	if (!isAdmissibleRecord(value, bootIdentity, maxStateBytes)) {
		if (value !== undefined) await deleteIgnoringFailure(store);
		return null;
	}
	try {
		if ((await sha256ArrayBuffer(value.state)) !== value.stateSha256) {
			await deleteIgnoringFailure(store);
			return null;
		}
	} catch {
		await deleteIgnoringFailure(store);
		return null;
	}
	return value.state;
}

export async function saveV86State(
	store: V86StateStore,
	bootIdentity: string,
	state: ArrayBuffer,
	maxStateBytes: number,
): Promise<void> {
	if (
		!SHA256_PATTERN.test(bootIdentity) ||
		!isValidStateBound(maxStateBytes) ||
		state.byteLength === 0 ||
		state.byteLength > maxStateBytes
	) {
		await deleteIgnoringFailure(store);
		return;
	}
	try {
		const record: SavedV86StateRecord = {
			bootIdentity,
			schemaVersion: 2,
			state,
			stateByteLength: state.byteLength,
			stateSha256: await sha256ArrayBuffer(state),
		};
		await store.put(record);
	} catch {
		// VM startup succeeds even when browser persistence is unavailable.
	}
}

export async function deleteSavedV86State(store: V86StateStore): Promise<void> {
	await deleteIgnoringFailure(store);
}
