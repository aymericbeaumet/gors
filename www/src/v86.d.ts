interface V86Emulator {
	add_listener(
		event: "serial0-output-byte",
		callback: (byte: number) => void,
	): void;
	create_file(path: string, data: Uint8Array): Promise<void>;
	read_file(path: string): Promise<Uint8Array>;
	save_state(): Promise<ArrayBuffer>;
	serial0_send(data: string): void;
}

interface V86Constructor {
	new (options: Record<string, unknown>): V86Emulator;
}

declare const V86: V86Constructor;
