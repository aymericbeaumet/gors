package main

type myString string
type myRune rune

func main() {
	n := -1
	if string(rune(n)) != "\uFFFD" {
		panic("negative integer to string conversion changed")
	}
	surrogate := 0xD800
	if string(rune(surrogate)) != "\uFFFD" {
		panic("surrogate code point to string conversion changed")
	}
	tooBig := 0x110000
	if string(rune(tooBig)) != "\uFFFD" {
		panic("out-of-range code point to string conversion changed")
	}
	earth := 0x1F30E
	encoded := string(rune(earth))
	if encoded != "\U0001F30E" || len(encoded) != 4 {
		panic("multi-byte UTF-8 encoding of converted integer changed")
	}
	single := 'a'
	if string(single) != "a" || len(string(single)) != 1 {
		panic("single byte rune to string conversion changed")
	}
	runes := []rune("h\xff\u00e9")
	if len(runes) != 3 || runes[0] != 'h' || runes[1] != '\uFFFD' || runes[2] != 0xE9 {
		panic("invalid UTF-8 byte did not decode as U+FFFD in []rune conversion")
	}
	if string(runes) != "h\uFFFD\u00e9" {
		panic("round trip through []rune must replace invalid bytes, not preserve them")
	}
	if len("h\xff\u00e9") != 4 || len(string(runes)) != 6 {
		panic("byte lengths of original and rune-round-tripped strings changed")
	}
	if len([]rune("")) != 0 {
		panic("empty string to rune slice conversion changed")
	}
	ms := myString([]myRune{0x266B, 0x1F30D})
	if ms != "\u266B\U0001F30D" {
		panic("named rune slice to named string conversion changed")
	}
	back := []myRune(ms)
	if len(back) != 2 || back[0] != 0x266B || back[1] != 0x1F30D {
		panic("named string to named rune slice conversion changed")
	}
}
