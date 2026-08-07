package main

func main() {
	octalByte := "\377"
	hexByte := "\xFF"
	charFF := "ÿ"
	littleU := "\u00FF"
	bigU := "\U000000FF"
	utf8Bytes := "\xc3\xbf"

	if len(octalByte) != 1 || octalByte[0] != 0xFF {
		panic("\\377 must be the single byte 0xFF")
	}
	if octalByte != hexByte {
		panic("\\377 and \\xFF must be identical single-byte strings")
	}
	if len(charFF) != 2 || charFF[0] != 0xC3 || charFF[1] != 0xBF {
		panic("the character U+00FF must contribute its two UTF-8 bytes")
	}
	if charFF != littleU || littleU != bigU || bigU != utf8Bytes {
		panic("U+00FF written as a character, \\u, \\U, or explicit UTF-8 bytes must be identical")
	}
	if octalByte == charFF {
		panic("the byte escape \\377 must differ from the UTF-8 encoding of U+00FF")
	}
	nihongo := "日本語"
	if nihongo != `日本語` || nihongo != "\u65e5\u672c\u8a9e" ||
		nihongo != "\U000065e5\U0000672c\U00008a9e" ||
		nihongo != "\xe6\x97\xa5\xe6\x9c\xac\xe8\xaa\x9e" {
		panic("the five spellings of the same string must be identical")
	}
	mixedOctal := "\000\007\377ok"
	if len(mixedOctal) != 5 || mixedOctal[0] != 0 || mixedOctal[1] != 7 || mixedOctal[2] != 0xFF {
		panic("octal escapes must produce individual bytes")
	}
	println(len(octalByte), len(charFF), len(nihongo), len(mixedOctal))
}
