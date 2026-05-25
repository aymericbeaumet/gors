package main

import (
	"fmt"
	"unicode/utf16"
)

func main() {
	fmt.Println("== utf16/append_rune ==")
	case_utf16_append_rune()
	fmt.Println("== utf16/decode ==")
	case_utf16_decode()
	fmt.Println("== utf16/decode_rune ==")
	case_utf16_decode_rune()
	fmt.Println("== utf16/encode ==")
	case_utf16_encode()
	fmt.Println("== utf16/encode_rune ==")
	case_utf16_encode_rune()
	fmt.Println("== utf16/is_surrogate ==")
	case_utf16_is_surrogate()
	fmt.Println("== utf16/rune_len ==")
	case_utf16_rune_len()
}

func case_utf16_append_rune() {
	out := utf16.AppendRune([]uint16{'g', 'o'}, '😀')
	fmt.Println(out)
	out = utf16.AppendRune(out, -1)
	fmt.Println(out)
}

func case_utf16_decode() {
	runes := utf16.Decode([]uint16{0x0067, 0x006f, 0xd83d, 0xde00})
	fmt.Println(string(runes))
}

func case_utf16_decode_rune() {
	fmt.Println(utf16.DecodeRune(0xd83d, 0xde00))
	fmt.Println(utf16.DecodeRune('g', 'o'))
}

func case_utf16_encode() {
	encoded := utf16.Encode([]rune("go😀"))
	fmt.Println(encoded)
}

func case_utf16_encode_rune() {
	hi, lo := utf16.EncodeRune('😀')
	fmt.Println(hi, lo)
	hi, lo = utf16.EncodeRune('g')
	fmt.Println(hi, lo)
}

func case_utf16_is_surrogate() {
	fmt.Println(utf16.IsSurrogate(0xd83d))
	fmt.Println(utf16.IsSurrogate('g'))
}

func case_utf16_rune_len() {
	fmt.Println(utf16.RuneLen('g'))
	fmt.Println(utf16.RuneLen('😀'))
	fmt.Println(utf16.RuneLen(-1))
}
