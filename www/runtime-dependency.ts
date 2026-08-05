export const RUNTIME_DEPENDENCY_SCHEMA_VERSION = 1 as const;

const CONTRACT_IDENTITY_PATTERN = /^[0-9a-f]{64}$/;
const RUNTIME_DEPENDENCY_KEYS = [
	"contractIdentity",
	"operationIds",
	"schemaVersion",
] as const;

/** Target-neutral runtime requirements emitted by the Wasm compiler. */
export interface RuntimeDependency {
	readonly schemaVersion: typeof RUNTIME_DEPENDENCY_SCHEMA_VERSION;
	readonly contractIdentity: string;
	readonly operationIds: Uint16Array;
}

export class RuntimeDependencyProtocolError extends Error {
	constructor(boundary: string, detail: string) {
		super(`${boundary}: invalid runtime dependency: ${detail}`);
		this.name = "RuntimeDependencyProtocolError";
	}
}

function isRecord(value: unknown): value is Record<string, unknown> {
	return typeof value === "object" && value !== null && !Array.isArray(value);
}

function hasExactKeys(value: Record<string, unknown>): boolean {
	const keys = Object.keys(value).sort();
	return (
		keys.length === RUNTIME_DEPENDENCY_KEYS.length &&
		RUNTIME_DEPENDENCY_KEYS.every((key, index) => keys[index] === key)
	);
}

/**
 * Validate and copy a dependency crossing a browser trust boundary.
 *
 * No schema coercion, contract default, operation sorting, or duplicate
 * removal is permitted here. A producer using another schema must be rebuilt.
 */
export function admitRuntimeDependency(
	value: unknown,
	boundary: string,
): RuntimeDependency {
	if (!isRecord(value) || !hasExactKeys(value)) {
		throw new RuntimeDependencyProtocolError(
			boundary,
			"expected exactly schemaVersion, contractIdentity, and operationIds",
		);
	}
	if (value.schemaVersion !== RUNTIME_DEPENDENCY_SCHEMA_VERSION) {
		throw new RuntimeDependencyProtocolError(
			boundary,
			`unsupported schema ${String(value.schemaVersion)}`,
		);
	}
	if (
		typeof value.contractIdentity !== "string" ||
		!CONTRACT_IDENTITY_PATTERN.test(value.contractIdentity)
	) {
		throw new RuntimeDependencyProtocolError(
			boundary,
			"contractIdentity must be 64 lowercase hexadecimal characters",
		);
	}
	if (!(value.operationIds instanceof Uint16Array)) {
		throw new RuntimeDependencyProtocolError(
			boundary,
			"operationIds must be a Uint16Array",
		);
	}
	for (let index = 1; index < value.operationIds.length; index++) {
		if (value.operationIds[index - 1] >= value.operationIds[index]) {
			throw new RuntimeDependencyProtocolError(
				boundary,
				"operationIds must be strictly increasing",
			);
		}
	}

	return {
		schemaVersion: RUNTIME_DEPENDENCY_SCHEMA_VERSION,
		contractIdentity: value.contractIdentity,
		operationIds: value.operationIds.slice(),
	};
}

/** Exact cache identity for one source revision and runtime dependency. */
export function runtimeDependencyCacheIdentity(
	goSource: string,
	value: unknown,
): string {
	const dependency = admitRuntimeDependency(value, "compiler cache identity");
	const runtimeIdentity = `${dependency.schemaVersion}:${dependency.contractIdentity}:${Array.from(dependency.operationIds).join(",")}`;
	return `${runtimeIdentity.length}:${runtimeIdentity}${goSource.length}:${goSource}`;
}

/** Exact JSON request written beside generated Rust for the V86 provider. */
export function runtimeDependencyRequestJson(value: unknown): string {
	const dependency = admitRuntimeDependency(value, "V86 compile request");
	return `${JSON.stringify({
		schema_version: dependency.schemaVersion,
		contract: dependency.contractIdentity,
		operation_ids: Array.from(dependency.operationIds),
	})}\n`;
}
