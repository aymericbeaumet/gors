package main

import "unsafe"

func main() {
	text := "\xffa\u00ff"
	bytes := []byte(text)
	reconstructed := string([]byte{0xff, 'a'})
	appended := append([]byte{7}, text...)
	unsafeBytes := []byte{0xff, 'b'}
	unsafeString := unsafe.String(unsafe.SliceData(unsafeBytes), len(unsafeBytes))
	invalidRunes := []rune{-1, 0xd800, 0x110000, 'c'}
	replacedRunes := string(invalidRunes)
	escapedRunes := string([]rune{0x10ffff, 0xe080})
	lowerRawByte := "\x80"
	higherUTF8 := "\u00ff"

	rangeIndexes := []int{}
	rangeRunes := []rune{}
	for index, value := range text {
		rangeIndexes = append(rangeIndexes, index)
		rangeRunes = append(rangeRunes, value)
	}

	if len(text) != 4 || text[0] != 0xff || text[1] != 'a' || text[2] != 0xc3 || text[3] != 0xbf {
		panic("string literal byte sequence changed")
	}
	if len(bytes) != 4 || bytes[0] != 0xff || bytes[3] != 0xbf {
		panic("string-to-byte-slice conversion changed")
	}
	if len(reconstructed) != 2 || reconstructed[0] != 0xff || reconstructed[1] != 'a' {
		panic("byte-slice-to-string conversion changed")
	}
	if len(appended) != 5 || appended[0] != 7 || appended[1] != 0xff || appended[2] != 'a' || appended[3] != 0xc3 || appended[4] != 0xbf {
		panic("append of invalid string bytes changed")
	}
	if len(unsafeString) != 2 || unsafeString[0] != 0xff || unsafeString[1] != 'b' {
		panic("unsafe.String invalid bytes changed")
	}
	if replacedRunes != "\uFFFD\uFFFD\uFFFDc" {
		panic("invalid rune string conversion changed")
	}
	if escapedRunes != "\U0010FFFF\uE080" {
		panic("valid rune string conversion changed")
	}
	if !(lowerRawByte < higherUTF8) {
		panic("string comparison did not use Go byte ordering")
	}
	if len(rangeIndexes) != 3 || rangeIndexes[0] != 0 || rangeIndexes[1] != 1 || rangeIndexes[2] != 2 {
		panic("string range byte indexes changed")
	}
	if len(rangeRunes) != 3 || rangeRunes[0] != '\uFFFD' || rangeRunes[1] != 'a' || rangeRunes[2] != '\u00FF' {
		panic("string range rune decoding changed")
	}
	print(text, "\n")
	println("string-bytes-and-invalid-utf8: ok")
}
