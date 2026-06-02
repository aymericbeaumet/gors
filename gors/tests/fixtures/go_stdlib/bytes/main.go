package main

import (
	"bytes"
	"fmt"
)

func main() {
	fmt.Println("== bytes/clone ==")
	case_bytes_clone()
	fmt.Println("== bytes/compare ==")
	case_bytes_compare()
	fmt.Println("== bytes/contains ==")
	case_bytes_contains()
	fmt.Println("== bytes/contains_any ==")
	case_bytes_contains_any()
	fmt.Println("== bytes/contains_rune ==")
	case_bytes_contains_rune()
	fmt.Println("== bytes/count ==")
	case_bytes_count()
	fmt.Println("== bytes/cut ==")
	case_bytes_cut()
	fmt.Println("== bytes/cut_prefix ==")
	case_bytes_cut_prefix()
	fmt.Println("== bytes/cut_suffix ==")
	case_bytes_cut_suffix()
	fmt.Println("== bytes/equal ==")
	case_bytes_equal()
	fmt.Println("== bytes/equal_fold ==")
	case_bytes_equal_fold()
	fmt.Println("== bytes/has_prefix ==")
	case_bytes_has_prefix()
	fmt.Println("== bytes/has_suffix ==")
	case_bytes_has_suffix()
	fmt.Println("== bytes/index ==")
	case_bytes_index()
	fmt.Println("== bytes/index_any ==")
	case_bytes_index_any()
	fmt.Println("== bytes/index_byte ==")
	case_bytes_index_byte()
	fmt.Println("== bytes/index_rune ==")
	case_bytes_index_rune()
	fmt.Println("== bytes/last_index ==")
	case_bytes_last_index()
	fmt.Println("== bytes/last_index_any ==")
	case_bytes_last_index_any()
	fmt.Println("== bytes/last_index_byte ==")
	case_bytes_last_index_byte()
	fmt.Println("== bytes/runes ==")
	case_bytes_runes()
	fmt.Println("== bytes/split ==")
	case_bytes_split()
	fmt.Println("== bytes/split_after ==")
	case_bytes_split_after()
	fmt.Println("== bytes/split_after_n ==")
	case_bytes_split_after_n()
	fmt.Println("== bytes/split_n ==")
	case_bytes_split_n()
	fmt.Println("== bytes/to_valid_utf8 ==")
	case_bytes_to_valid_utf8()
	fmt.Println("== bytes/trim_prefix ==")
	case_bytes_trim_prefix()
	fmt.Println("== bytes/trim_suffix ==")
	case_bytes_trim_suffix()
}

func printBytes(b []byte) {
	fmt.Println(string(b))
}

func printByteSlices(parts [][]byte) {
	for i, part := range parts {
		fmt.Println(i, string(part))
	}
}

func case_bytes_clone() {
	// gors:stdlib-cover bytes::Clone
	printBytes(bytes.Clone([]byte("alpha")))
}

func case_bytes_compare() {
	// gors:stdlib-cover bytes::Compare
	fmt.Println(bytes.Compare([]byte("alpha"), []byte("beta")))
}

func case_bytes_contains() {
	// gors:stdlib-cover bytes::Contains
	fmt.Println(bytes.Contains([]byte("alphabet"), []byte("pha")))
}

func case_bytes_contains_any() {
	// gors:stdlib-cover bytes::ContainsAny
	fmt.Println(bytes.ContainsAny([]byte("team"), "xyzam"))
}

func case_bytes_contains_func_isDash(r rune) bool {
	return r == '-'
}

func case_bytes_contains_func() {
	fmt.Println(bytes.ContainsFunc([]byte("alpha-beta"), case_bytes_contains_func_isDash))
}

func case_bytes_contains_rune() {
	// gors:stdlib-cover bytes::ContainsRune
	fmt.Println(bytes.ContainsRune([]byte("alpha"), 'p'))
}

func case_bytes_count() {
	// gors:stdlib-cover bytes::Count
	fmt.Println(bytes.Count([]byte("banana"), []byte("ana")))
}

func case_bytes_cut() {
	// gors:stdlib-cover bytes::Cut
	before, after, found := bytes.Cut([]byte("key=value"), []byte("="))
	fmt.Println(string(before), string(after), found)
}

func case_bytes_cut_prefix() {
	// gors:stdlib-cover bytes::CutPrefix
	after, found := bytes.CutPrefix([]byte("prefix:value"), []byte("prefix:"))
	fmt.Println(string(after), found)
}

func case_bytes_cut_suffix() {
	// gors:stdlib-cover bytes::CutSuffix
	before, found := bytes.CutSuffix([]byte("value.suffix"), []byte(".suffix"))
	fmt.Println(string(before), found)
}

func case_bytes_equal() {
	// gors:stdlib-cover bytes::Equal
	fmt.Println(bytes.Equal([]byte("alpha"), []byte("alpha")))
}

func case_bytes_equal_fold() {
	// gors:stdlib-cover bytes::EqualFold
	fmt.Println(bytes.EqualFold([]byte("GoLang"), []byte("golang")))
}

func case_bytes_fields() {
	printByteSlices(bytes.Fields([]byte(" alpha\tbeta  gamma ")))
}

func case_bytes_fields_func_isSep(r rune) bool {
	return r == ',' || r == ';'
}

func case_bytes_fields_func() {
	printByteSlices(bytes.FieldsFunc([]byte("alpha,beta;gamma"), case_bytes_fields_func_isSep))
}

func case_bytes_has_prefix() {
	// gors:stdlib-cover bytes::HasPrefix
	fmt.Println(bytes.HasPrefix([]byte("transpile"), []byte("trans")))
}

func case_bytes_has_suffix() {
	// gors:stdlib-cover bytes::HasSuffix
	fmt.Println(bytes.HasSuffix([]byte("transpile"), []byte("pile")))
}

func case_bytes_index() {
	// gors:stdlib-cover bytes::Index
	fmt.Println(bytes.Index([]byte("alphabet"), []byte("ha")))
}

func case_bytes_index_any() {
	// gors:stdlib-cover bytes::IndexAny
	fmt.Println(bytes.IndexAny([]byte("alphabet"), "zxha"))
}

func case_bytes_index_byte() {
	// gors:stdlib-cover bytes::IndexByte
	fmt.Println(bytes.IndexByte([]byte("alphabet"), 'h'))
}

func case_bytes_index_func_isDash(r rune) bool {
	return r == '-'
}

func case_bytes_index_func() {
	fmt.Println(bytes.IndexFunc([]byte("alpha-beta"), case_bytes_index_func_isDash))
}

func case_bytes_index_rune() {
	// gors:stdlib-cover bytes::IndexRune
	fmt.Println(bytes.IndexRune([]byte("alphabet"), 'p'))
}

func case_bytes_last_index() {
	// gors:stdlib-cover bytes::LastIndex
	fmt.Println(bytes.LastIndex([]byte("go gopher go"), []byte("go")))
}

func case_bytes_last_index_any() {
	// gors:stdlib-cover bytes::LastIndexAny
	fmt.Println(bytes.LastIndexAny([]byte("alpha-beta"), "-x"))
}

func case_bytes_last_index_byte() {
	// gors:stdlib-cover bytes::LastIndexByte
	fmt.Println(bytes.LastIndexByte([]byte("banana"), 'a'))
}

func case_bytes_last_index_func_isDash(r rune) bool {
	return r == '-'
}

func case_bytes_last_index_func() {
	fmt.Println(bytes.LastIndexFunc([]byte("alpha-beta"), case_bytes_last_index_func_isDash))
}

func case_bytes_map_rot(r rune) rune {
	if r >= 'a' && r <= 'z' {
		return r + 1
	}
	return r
}

func case_bytes_map() {
	printBytes(bytes.Map(case_bytes_map_rot, []byte("abc!")))
}

func case_bytes_runes() {
	// gors:stdlib-cover bytes::Runes
	fmt.Println(bytes.Runes([]byte("goλ")))
}

func case_bytes_split() {
	// gors:stdlib-cover bytes::Split
	printByteSlices(bytes.Split([]byte("a,b,c"), []byte(",")))
}

func case_bytes_split_after() {
	// gors:stdlib-cover bytes::SplitAfter
	printByteSlices(bytes.SplitAfter([]byte("a,b,c"), []byte(",")))
}

func case_bytes_split_after_n() {
	// gors:stdlib-cover bytes::SplitAfterN
	printByteSlices(bytes.SplitAfterN([]byte("a,b,c"), []byte(","), 2))
}

func case_bytes_split_n() {
	// gors:stdlib-cover bytes::SplitN
	printByteSlices(bytes.SplitN([]byte("a,b,c"), []byte(","), 2))
}

func case_bytes_title() {
	printBytes(bytes.Title([]byte("go gopher")))
}

func case_bytes_to_lower() {
	printBytes(bytes.ToLower([]byte("GoLang")))
}

func case_bytes_to_title() {
	printBytes(bytes.ToTitle([]byte("GoLang")))
}

func case_bytes_to_upper() {
	printBytes(bytes.ToUpper([]byte("GoLang")))
}

func case_bytes_to_valid_utf8() {
	// gors:stdlib-cover bytes::ToValidUTF8
	printBytes(bytes.ToValidUTF8([]byte{'g', 'o', 0xff}, []byte("?")))
}

func case_bytes_trim() {
	printBytes(bytes.Trim([]byte("!!go!!"), "!"))
}

func case_bytes_trim_func_isBang(r rune) bool {
	return r == '!'
}

func case_bytes_trim_func() {
	printBytes(bytes.TrimFunc([]byte("!!go!!"), case_bytes_trim_func_isBang))
}

func case_bytes_trim_left() {
	printBytes(bytes.TrimLeft([]byte("!!go!!"), "!"))
}

func case_bytes_trim_left_func() {
	printBytes(bytes.TrimLeftFunc([]byte("!!go!!"), case_bytes_trim_func_isBang))
}

func case_bytes_trim_prefix() {
	// gors:stdlib-cover bytes::TrimPrefix
	printBytes(bytes.TrimPrefix([]byte("prefix:value"), []byte("prefix:")))
}

func case_bytes_trim_right() {
	printBytes(bytes.TrimRight([]byte("!!go!!"), "!"))
}

func case_bytes_trim_right_func() {
	printBytes(bytes.TrimRightFunc([]byte("!!go!!"), case_bytes_trim_func_isBang))
}

func case_bytes_trim_suffix() {
	// gors:stdlib-cover bytes::TrimSuffix
	printBytes(bytes.TrimSuffix([]byte("value.suffix"), []byte(".suffix")))
}
