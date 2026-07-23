export interface V86Emulator {
	add_listener(
		event: "serial0-output-byte",
		callback: (byte: number) => void,
	): void;
	add_listener(
		event: "download-error",
		callback: (event: { file_name?: unknown }) => void,
	): void;
	remove_listener(
		event: "serial0-output-byte",
		callback: (byte: number) => void,
	): void;
	remove_listener(
		event: "download-error",
		callback: (event: { file_name?: unknown }) => void,
	): void;
	create_file(path: string, data: Uint8Array): Promise<void>;
	destroy(): Promise<void>;
	read_file(path: string): Promise<Uint8Array>;
	save_state(): Promise<ArrayBuffer>;
	serial0_send(data: string): void;
	stop(): Promise<void>;
}

export interface V86Constructor {
	new (options: Record<string, unknown>): V86Emulator;
}

declare global {
	const V86: V86Constructor;
}
