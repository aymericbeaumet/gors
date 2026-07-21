const DATABASE_NAME = "gors-compiler-cache";
const DATABASE_VERSION = 1;
const STORE_NAME = "snapshots";
const RESOLVER_CACHE_KEY = "resolved-modules-v1";

export const MAX_PERSISTED_RESOLVER_CACHE_BYTES = 64 * 1024 * 1024;

function openDatabase(): Promise<IDBDatabase> {
	return new Promise((resolve, reject) => {
		const request = indexedDB.open(DATABASE_NAME, DATABASE_VERSION);
		request.onupgradeneeded = () => {
			if (!request.result.objectStoreNames.contains(STORE_NAME)) {
				request.result.createObjectStore(STORE_NAME);
			}
		};
		request.onsuccess = () => resolve(request.result);
		request.onerror = () => reject(request.error);
	});
}

function asBytes(value: unknown): Uint8Array | null {
	if (value instanceof ArrayBuffer) return new Uint8Array(value);
	if (value instanceof Uint8Array) return value;
	return null;
}

export async function loadResolverCacheSnapshot(): Promise<Uint8Array | null> {
	const database = await openDatabase();
	return new Promise((resolve, reject) => {
		const transaction = database.transaction(STORE_NAME, "readonly");
		const request = transaction.objectStore(STORE_NAME).get(RESOLVER_CACHE_KEY);
		request.onsuccess = () => {
			database.close();
			const bytes = asBytes(request.result);
			if (!bytes || bytes.byteLength > MAX_PERSISTED_RESOLVER_CACHE_BYTES) {
				resolve(null);
				return;
			}
			resolve(bytes);
		};
		request.onerror = () => {
			database.close();
			reject(request.error);
		};
	});
}

export async function storeResolverCacheSnapshot(
	bytes: Uint8Array,
): Promise<boolean> {
	if (bytes.byteLength > MAX_PERSISTED_RESOLVER_CACHE_BYTES) {
		await deleteResolverCacheSnapshot();
		return false;
	}

	const database = await openDatabase();
	const storedBytes = bytes.slice().buffer;
	return new Promise((resolve, reject) => {
		const transaction = database.transaction(STORE_NAME, "readwrite");
		transaction.objectStore(STORE_NAME).put(storedBytes, RESOLVER_CACHE_KEY);
		transaction.oncomplete = () => {
			database.close();
			resolve(true);
		};
		transaction.onerror = () => {
			database.close();
			reject(transaction.error);
		};
		transaction.onabort = () => {
			database.close();
			reject(transaction.error);
		};
	});
}

export async function deleteResolverCacheSnapshot(): Promise<void> {
	const database = await openDatabase();
	return new Promise((resolve, reject) => {
		const transaction = database.transaction(STORE_NAME, "readwrite");
		transaction.objectStore(STORE_NAME).delete(RESOLVER_CACHE_KEY);
		transaction.oncomplete = () => {
			database.close();
			resolve();
		};
		transaction.onerror = () => {
			database.close();
			reject(transaction.error);
		};
		transaction.onabort = () => {
			database.close();
			reject(transaction.error);
		};
	});
}
