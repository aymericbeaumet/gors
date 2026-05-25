package main

import (
	"fmt"
	"unicode/utf8"
)

func main() {
	fmt.Println("== utf8/append_rune ==")
	case_utf8_append_rune()
	fmt.Println("== utf8/constants ==")
	case_utf8_constants()
	fmt.Println("== utf8/decode_last_rune ==")
	case_utf8_decode_last_rune()
	fmt.Println("== utf8/decode_last_rune_in_string ==")
	case_utf8_decode_last_rune_in_string()
	fmt.Println("== utf8/decode_rune ==")
	case_utf8_decode_rune()
	fmt.Println("== utf8/decode_rune_in_string ==")
	case_utf8_decode_rune_in_string()
	fmt.Println("== utf8/encode_rune ==")
	case_utf8_encode_rune()
	fmt.Println("== utf8/full_rune ==")
	case_utf8_full_rune()
	fmt.Println("== utf8/full_rune_in_string ==")
	case_utf8_full_rune_in_string()
	fmt.Println("== utf8/rune_count ==")
	case_utf8_rune_count()
	fmt.Println("== utf8/rune_count_in_string ==")
	case_utf8_rune_count_in_string()
	fmt.Println("== utf8/rune_len ==")
	case_utf8_rune_len()
	fmt.Println("== utf8/rune_start ==")
	case_utf8_rune_start()
	fmt.Println("== utf8/valid ==")
	case_utf8_valid()
	fmt.Println("== utf8/valid_rune ==")
	case_utf8_valid_rune()
	fmt.Println("== utf8/valid_string ==")
	case_utf8_valid_string()
}

func case_utf8_append_rune() {
	out := utf8.AppendRune([]byte("go:"), 'λ')
	fmt.Println(string(out))
}

func case_utf8_constants() {
	fmt.Println(utf8.MaxRune, utf8.RuneError, utf8.RuneSelf, utf8.UTFMax)
}

func case_utf8_decode_last_rune() {
	r, size := utf8.DecodeLastRune([]byte("goλ"))
	fmt.Println(r, size)
	invalid := []byte{0xff}
	r, size = utf8.DecodeLastRune(invalid)
	fmt.Println(r, size)
}

func case_utf8_decode_last_rune_in_string() {
	r, size := utf8.DecodeLastRuneInString("goλ")
	fmt.Println(r, size)
}

func case_utf8_decode_rune() {
	r, size := utf8.DecodeRune([]byte("λgo"))
	fmt.Println(r, size)
	invalid := []byte{0xff}
	r, size = utf8.DecodeRune(invalid)
	fmt.Println(r, size)
}

func case_utf8_decode_rune_in_string() {
	r, size := utf8.DecodeRuneInString("λgo")
	fmt.Println(r, size)
}

func case_utf8_encode_rune() {
	buf := []byte{0, 0, 0, 0}
	n := utf8.EncodeRune(buf, 'λ')
	fmt.Println(n, buf)
	n = utf8.EncodeRune(buf, utf8.MaxRune+1)
	fmt.Println(n, buf)
}

func case_utf8_full_rune() {
	fmt.Println(utf8.FullRune([]byte("λ")))
	incomplete := []byte{0xce}
	fmt.Println(utf8.FullRune(incomplete))
}

func case_utf8_full_rune_in_string() {
	fmt.Println(utf8.FullRuneInString("λ"))
	incomplete := string([]byte{0xce})
	fmt.Println(utf8.FullRuneInString(incomplete))
}

func case_utf8_rune_count() {
	fmt.Println(utf8.RuneCount([]byte("goλ")))
}

func case_utf8_rune_count_in_string() {
	fmt.Println(utf8.RuneCountInString("goλ"))
}

func case_utf8_rune_len() {
	fmt.Println(utf8.RuneLen('λ'))
	fmt.Println(utf8.RuneLen(-1))
}

func case_utf8_rune_start() {
	fmt.Println(utf8.RuneStart(0xce))
	fmt.Println(utf8.RuneStart(0xbb))
}

func case_utf8_valid() {
	fmt.Println(utf8.Valid([]byte("goλ")))
	invalid := []byte{0xff}
	fmt.Println(utf8.Valid(invalid))
}

func case_utf8_valid_rune() {
	fmt.Println(utf8.ValidRune('λ'))
	fmt.Println(utf8.ValidRune(utf8.RuneError))
}

func case_utf8_valid_string() {
	fmt.Println(utf8.ValidString("goλ"))
}
