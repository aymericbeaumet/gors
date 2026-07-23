import rawBootContract from "./v86/boot-contract.json";

export interface V86VmContract {
	readonly autostart: boolean;
	readonly memorySizeBytes: number;
	readonly vgaMemorySizeBytes: number;
	readonly disableKeyboard: boolean;
	readonly disableMouse: boolean;
	readonly bzimageInitrdFromFilesystem: boolean;
	readonly cmdline: string;
	readonly maxSavedStateBytes: number;
}

export interface V86GuestProtocol {
	readonly schemaVersion: 1;
	readonly bootReadyMarker: string;
	readonly readyMarker: string;
	readonly compileDonePrefix: string;
	readonly runDonePrefix: string;
	readonly compileCommand: string;
	readonly runCommand: string;
	readonly jobDirectory: string;
	readonly nonceHexLength: number;
}

export interface V86BootContract {
	readonly schemaVersion: 1;
	readonly vm: V86VmContract;
	readonly guestProtocol: V86GuestProtocol;
}

const CONTRACT_FIELDS = ["schemaVersion", "vm", "guestProtocol"] as const;
const VM_FIELDS = [
	"autostart",
	"memorySizeBytes",
	"vgaMemorySizeBytes",
	"disableKeyboard",
	"disableMouse",
	"bzimageInitrdFromFilesystem",
	"cmdline",
	"maxSavedStateBytes",
] as const;
const PROTOCOL_FIELDS = [
	"schemaVersion",
	"bootReadyMarker",
	"readyMarker",
	"compileDonePrefix",
	"runDonePrefix",
	"compileCommand",
	"runCommand",
	"jobDirectory",
	"nonceHexLength",
] as const;

function isRecord(value: unknown): value is Record<string, unknown> {
	return typeof value === "object" && value !== null && !Array.isArray(value);
}

function hasExactFields(
	value: Record<string, unknown>,
	fields: readonly string[],
): boolean {
	const actual = Object.keys(value).sort();
	const expected = [...fields].sort();
	return (
		actual.length === expected.length &&
		actual.every((field, index) => field === expected[index])
	);
}

function requirePositiveSafeInteger(
	value: unknown,
	field: string,
): asserts value is number {
	if (!Number.isSafeInteger(value) || (value as number) <= 0) {
		throw new Error(`${field} must be a positive safe integer`);
	}
}

function requireBoolean(
	value: unknown,
	field: string,
): asserts value is boolean {
	if (typeof value !== "boolean") throw new Error(`${field} must be boolean`);
}

function requireProtocolText(
	value: unknown,
	field: string,
	suffix = "",
): asserts value is string {
	if (
		typeof value !== "string" ||
		!/^[A-Z][A-Z0-9_:]*$/.test(value) ||
		(suffix !== "" && !value.endsWith(suffix))
	) {
		throw new Error(`${field} is not a canonical guest protocol token`);
	}
}

function requireGuestIdentifier(
	value: unknown,
	field: string,
): asserts value is string {
	if (typeof value !== "string" || !/^[a-z][a-z0-9-]*$/.test(value)) {
		throw new Error(`${field} is not a canonical guest identifier`);
	}
}

export function admitV86BootContract(value: unknown): V86BootContract {
	if (!isRecord(value) || !hasExactFields(value, CONTRACT_FIELDS)) {
		throw new Error("V86 boot contract has an unsupported field set");
	}
	if (value.schemaVersion !== 1) {
		throw new Error("V86 boot contract schema must be 1");
	}
	if (!isRecord(value.vm) || !hasExactFields(value.vm, VM_FIELDS)) {
		throw new Error("V86 VM contract has an unsupported field set");
	}
	if (
		!isRecord(value.guestProtocol) ||
		!hasExactFields(value.guestProtocol, PROTOCOL_FIELDS)
	) {
		throw new Error("V86 guest protocol has an unsupported field set");
	}

	const vm = value.vm;
	requireBoolean(vm.autostart, "vm.autostart");
	requirePositiveSafeInteger(vm.memorySizeBytes, "vm.memorySizeBytes");
	requirePositiveSafeInteger(vm.vgaMemorySizeBytes, "vm.vgaMemorySizeBytes");
	requireBoolean(vm.disableKeyboard, "vm.disableKeyboard");
	requireBoolean(vm.disableMouse, "vm.disableMouse");
	requireBoolean(
		vm.bzimageInitrdFromFilesystem,
		"vm.bzimageInitrdFromFilesystem",
	);
	if (typeof vm.cmdline !== "string" || vm.cmdline.length === 0) {
		throw new Error("vm.cmdline must be non-empty text");
	}
	requirePositiveSafeInteger(vm.maxSavedStateBytes, "vm.maxSavedStateBytes");
	if (vm.maxSavedStateBytes < vm.memorySizeBytes) {
		throw new Error("vm.maxSavedStateBytes must cover guest memory");
	}

	const protocol = value.guestProtocol;
	if (protocol.schemaVersion !== 1) {
		throw new Error("V86 guest protocol schema must be 1");
	}
	requireProtocolText(
		protocol.bootReadyMarker,
		"guestProtocol.bootReadyMarker",
	);
	requireProtocolText(protocol.readyMarker, "guestProtocol.readyMarker");
	requireProtocolText(
		protocol.compileDonePrefix,
		"guestProtocol.compileDonePrefix",
		":",
	);
	requireProtocolText(
		protocol.runDonePrefix,
		"guestProtocol.runDonePrefix",
		":",
	);
	requireGuestIdentifier(
		protocol.compileCommand,
		"guestProtocol.compileCommand",
	);
	requireGuestIdentifier(protocol.runCommand, "guestProtocol.runCommand");
	requireGuestIdentifier(protocol.jobDirectory, "guestProtocol.jobDirectory");
	requirePositiveSafeInteger(
		protocol.nonceHexLength,
		"guestProtocol.nonceHexLength",
	);
	if (protocol.nonceHexLength !== 32) {
		throw new Error("V86 guest nonce length must be 32 hexadecimal characters");
	}

	return {
		schemaVersion: 1,
		vm: {
			autostart: vm.autostart,
			memorySizeBytes: vm.memorySizeBytes,
			vgaMemorySizeBytes: vm.vgaMemorySizeBytes,
			disableKeyboard: vm.disableKeyboard,
			disableMouse: vm.disableMouse,
			bzimageInitrdFromFilesystem: vm.bzimageInitrdFromFilesystem,
			cmdline: vm.cmdline,
			maxSavedStateBytes: vm.maxSavedStateBytes,
		},
		guestProtocol: {
			schemaVersion: 1,
			bootReadyMarker: protocol.bootReadyMarker,
			readyMarker: protocol.readyMarker,
			compileDonePrefix: protocol.compileDonePrefix,
			runDonePrefix: protocol.runDonePrefix,
			compileCommand: protocol.compileCommand,
			runCommand: protocol.runCommand,
			jobDirectory: protocol.jobDirectory,
			nonceHexLength: protocol.nonceHexLength,
		},
	};
}

function freezeBootContract(value: V86BootContract): V86BootContract {
	Object.freeze(value.vm);
	Object.freeze(value.guestProtocol);
	return Object.freeze(value);
}

export const V86_BOOT_CONTRACT = freezeBootContract(
	admitV86BootContract(rawBootContract),
);

export function sameV86BootContract(value: V86BootContract): boolean {
	return JSON.stringify(value) === JSON.stringify(V86_BOOT_CONTRACT);
}
