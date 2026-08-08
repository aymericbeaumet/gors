package main

func takeUint8Slice(values []uint8) int { return len(values) }

func takeInt32(value int32) int32 { return value + 1 }

func main() {
	var b byte = 'A'
	var u uint8 = b
	u = 66
	var r rune = 'Z'
	var i int32 = r

	var anyByte any = b
	asUint8, byteIsUint8 := anyByte.(uint8)
	var seven uint8 = 7
	var anyUint8 any = seven
	asByte, uint8IsByte := anyUint8.(byte)
	var anyRune any = r
	asInt32, runeIsInt32 := anyRune.(int32)
	var nine int32 = 9
	var anyInt32 any = nine
	asRune, int32IsRune := anyInt32.(rune)

	bytes := []byte{1, 2, 3}
	if takeUint8Slice(bytes) != 3 {
		panic("[]byte was not usable as []uint8")
	}
	if takeInt32(r) != 91 {
		panic("rune was not usable as int32")
	}
	if !byteIsUint8 || asUint8 != 'A' || !uint8IsByte || asByte != 7 {
		panic("byte/uint8 dynamic types differ")
	}
	if !runeIsInt32 || asInt32 != 'Z' || !int32IsRune || asRune != 9 {
		panic("rune/int32 dynamic types differ")
	}
	println("byte-rune-alias-identity:", u, i)
}
