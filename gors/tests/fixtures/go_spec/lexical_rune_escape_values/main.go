package main

func main() {
	escapes := []rune{'\a', '\b', '\f', '\n', '\r', '\t', '\v', '\\', '\''}
	want := []rune{7, 8, 12, 10, 13, 9, 11, 92, 39}
	for index, value := range escapes {
		if value != want[index] {
			panic("single-character rune escapes changed")
		}
	}
	if '\000' != 0 || '\007' != 7 || '\377' != 255 {
		panic("octal rune escapes changed")
	}
	if '\x07' != 7 || '\xff' != 255 {
		panic("hex byte rune escapes changed")
	}
	if '\u12e4' != 0x12e4 || '\U00101234' != 0x101234 {
		panic("\\u and \\U rune escapes changed")
	}
	if 'ä' != 0xE4 || '本' != 0x672C {
		panic("multi-byte source characters must be single rune values")
	}
	if '\xff' != '\u00ff' {
		panic("in rune literals \\xff and \\u00ff are the same integer value 255")
	}
	if string('\377') != "\u00ff" || len(string('\377')) != 2 {
		panic("converting rune 255 to string must yield its two-byte UTF-8 encoding")
	}
	println('\377', '\u12e4', '\U00101234', 'ä')
}
