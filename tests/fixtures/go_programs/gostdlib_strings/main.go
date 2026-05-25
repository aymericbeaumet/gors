package main

import (
	"fmt"
	"strings"
)

func main() {
	fmt.Println("== strings/clone ==")
	case_strings_clone()
	fmt.Println("== strings/compare ==")
	case_strings_compare()
	fmt.Println("== strings/contains ==")
	case_strings_contains()
	fmt.Println("== strings/contains_any ==")
	case_strings_contains_any()
	fmt.Println("== strings/contains_func ==")
	case_strings_contains_func()
	fmt.Println("== strings/contains_rune ==")
	case_strings_contains_rune()
	fmt.Println("== strings/count ==")
	case_strings_count()
	fmt.Println("== strings/cut ==")
	case_strings_cut()
	fmt.Println("== strings/cut_prefix ==")
	case_strings_cut_prefix()
	fmt.Println("== strings/cut_suffix ==")
	case_strings_cut_suffix()
	fmt.Println("== strings/equal_fold ==")
	case_strings_equal_fold()
	fmt.Println("== strings/fields ==")
	case_strings_fields()
	fmt.Println("== strings/fields_func ==")
	case_strings_fields_func()
	fmt.Println("== strings/has_prefix ==")
	case_strings_has_prefix()
	fmt.Println("== strings/has_suffix ==")
	case_strings_has_suffix()
	fmt.Println("== strings/index ==")
	case_strings_index()
	fmt.Println("== strings/index_any ==")
	case_strings_index_any()
	fmt.Println("== strings/index_byte ==")
	case_strings_index_byte()
	fmt.Println("== strings/index_func ==")
	case_strings_index_func()
	fmt.Println("== strings/index_rune ==")
	case_strings_index_rune()
	fmt.Println("== strings/join ==")
	case_strings_join()
	fmt.Println("== strings/last_index ==")
	case_strings_last_index()
	fmt.Println("== strings/last_index_any ==")
	case_strings_last_index_any()
	fmt.Println("== strings/last_index_byte ==")
	case_strings_last_index_byte()
	fmt.Println("== strings/last_index_func ==")
	case_strings_last_index_func()
	fmt.Println("== strings/map ==")
	case_strings_map()
	fmt.Println("== strings/repeat ==")
	case_strings_repeat()
	fmt.Println("== strings/replace ==")
	case_strings_replace()
	fmt.Println("== strings/replace_all ==")
	case_strings_replace_all()
	fmt.Println("== strings/split ==")
	case_strings_split()
	fmt.Println("== strings/split_after ==")
	case_strings_split_after()
	fmt.Println("== strings/split_after_n ==")
	case_strings_split_after_n()
	fmt.Println("== strings/split_n ==")
	case_strings_split_n()
	fmt.Println("== strings/title ==")
	case_strings_title()
	fmt.Println("== strings/to_lower ==")
	case_strings_to_lower()
	fmt.Println("== strings/to_title ==")
	case_strings_to_title()
	fmt.Println("== strings/to_upper ==")
	case_strings_to_upper()
	fmt.Println("== strings/to_valid_utf8 ==")
	case_strings_to_valid_utf8()
	fmt.Println("== strings/trim ==")
	case_strings_trim()
	fmt.Println("== strings/trim_func ==")
	case_strings_trim_func()
	fmt.Println("== strings/trim_left ==")
	case_strings_trim_left()
	fmt.Println("== strings/trim_left_func ==")
	case_strings_trim_left_func()
	fmt.Println("== strings/trim_prefix ==")
	case_strings_trim_prefix()
	fmt.Println("== strings/trim_right ==")
	case_strings_trim_right()
	fmt.Println("== strings/trim_right_func ==")
	case_strings_trim_right_func()
	fmt.Println("== strings/trim_space ==")
	case_strings_trim_space()
	fmt.Println("== strings/trim_suffix ==")
	case_strings_trim_suffix()
	fmt.Println("== strings/basic ==")
	case_strings_basic()
}

func case_strings_clone() {
	fmt.Println(strings.Clone("alpha"))
}

func case_strings_compare() {
	fmt.Println(strings.Compare("alpha", "beta"))
}

func case_strings_contains() {
	fmt.Println(strings.Contains("alphabet", "pha"))
}

func case_strings_contains_any() {
	fmt.Println(strings.ContainsAny("team", "xyzam"))
}

func case_strings_contains_func_isDash(r rune) bool {
	return r == '-'
}

func case_strings_contains_func() {
	fmt.Println(strings.ContainsFunc("alpha-beta", case_strings_contains_func_isDash))
}

func case_strings_contains_rune() {
	fmt.Println(strings.ContainsRune("alpha", 'p'))
}

func case_strings_count() {
	fmt.Println(strings.Count("banana", "ana"))
}

func case_strings_cut() {
	before, after, found := strings.Cut("key=value", "=")
	fmt.Println(before, after, found)
}

func case_strings_cut_prefix() {
	after, found := strings.CutPrefix("prefix:value", "prefix:")
	fmt.Println(after, found)
}

func case_strings_cut_suffix() {
	before, found := strings.CutSuffix("value.suffix", ".suffix")
	fmt.Println(before, found)
}

func case_strings_equal_fold() {
	fmt.Println(strings.EqualFold("GoLang", "golang"))
}

func case_strings_fields() {
	fmt.Println(strings.Fields(" alpha\tbeta  gamma "))
}

func case_strings_fields_func_isSep(r rune) bool {
	return r == ',' || r == ';'
}

func case_strings_fields_func() {
	fmt.Println(strings.FieldsFunc("alpha,beta;gamma", case_strings_fields_func_isSep))
}

func case_strings_has_prefix() {
	fmt.Println(strings.HasPrefix("transpile", "trans"))
}

func case_strings_has_suffix() {
	fmt.Println(strings.HasSuffix("transpile", "pile"))
}

func case_strings_index() {
	fmt.Println(strings.Index("alphabet", "ha"))
}

func case_strings_index_any() {
	fmt.Println(strings.IndexAny("alphabet", "zxha"))
}

func case_strings_index_byte() {
	fmt.Println(strings.IndexByte("alphabet", 'h'))
}

func case_strings_index_func_isDash(r rune) bool {
	return r == '-'
}

func case_strings_index_func() {
	fmt.Println(strings.IndexFunc("alpha-beta", case_strings_index_func_isDash))
}

func case_strings_index_rune() {
	fmt.Println(strings.IndexRune("alphabet", 'p'))
}

func case_strings_join() {
	parts := []string{"alpha", "beta", "gamma"}
	fmt.Println(strings.Join(parts, ":"))
}

func case_strings_last_index() {
	fmt.Println(strings.LastIndex("go gopher go", "go"))
}

func case_strings_last_index_any() {
	fmt.Println(strings.LastIndexAny("alpha-beta", "-x"))
}

func case_strings_last_index_byte() {
	fmt.Println(strings.LastIndexByte("banana", 'a'))
}

func case_strings_last_index_func_isDash(r rune) bool {
	return r == '-'
}

func case_strings_last_index_func() {
	fmt.Println(strings.LastIndexFunc("alpha-beta-gamma", case_strings_last_index_func_isDash))
}

func case_strings_map_mapRune(r rune) rune {
	if r == 'a' {
		return 'A'
	}
	return r
}

func case_strings_map() {
	fmt.Println(strings.Map(case_strings_map_mapRune, "banana"))
}

func case_strings_repeat() {
	fmt.Println(strings.Repeat("go", 3))
}

func case_strings_replace() {
	fmt.Println(strings.Replace("a-b-c-b", "b", "B", 1))
}

func case_strings_replace_all() {
	fmt.Println(strings.ReplaceAll("a-b-c-b", "b", "B"))
}

func case_strings_split() {
	fmt.Println(strings.Split("alpha,beta,gamma", ","))
}

func case_strings_split_after() {
	fmt.Println(strings.SplitAfter("alpha,beta,gamma", ","))
}

func case_strings_split_after_n() {
	fmt.Println(strings.SplitAfterN("alpha,beta,gamma", ",", 2))
}

func case_strings_split_n() {
	fmt.Println(strings.SplitN("alpha,beta,gamma", ",", 2))
}

func case_strings_title() {
	fmt.Println(strings.Title("hello world"))
}

func case_strings_to_lower() {
	fmt.Println(strings.ToLower("GoLANG"))
}

func case_strings_to_title() {
	fmt.Println(strings.ToTitle("gopher"))
}

func case_strings_to_upper() {
	fmt.Println(strings.ToUpper("gopher"))
}

func case_strings_to_valid_utf8() {
	fmt.Println(strings.ToValidUTF8("gopher", "?"))
}

func case_strings_trim() {
	fmt.Println(strings.Trim("!!gopher!!", "!"))
}

func case_strings_trim_func_isBang(r rune) bool {
	return r == '!'
}

func case_strings_trim_func() {
	fmt.Println(strings.TrimFunc("!!gopher!!", case_strings_trim_func_isBang))
}

func case_strings_trim_left() {
	fmt.Println(strings.TrimLeft("!!gopher!!", "!"))
}

func case_strings_trim_left_func_isBang(r rune) bool {
	return r == '!'
}

func case_strings_trim_left_func() {
	fmt.Println(strings.TrimLeftFunc("!!gopher!!", case_strings_trim_left_func_isBang))
}

func case_strings_trim_prefix() {
	fmt.Println(strings.TrimPrefix("prefix:gopher", "prefix:"))
}

func case_strings_trim_right() {
	fmt.Println(strings.TrimRight("!!gopher!!", "!"))
}

func case_strings_trim_right_func_isBang(r rune) bool {
	return r == '!'
}

func case_strings_trim_right_func() {
	fmt.Println(strings.TrimRightFunc("!!gopher!!", case_strings_trim_right_func_isBang))
}

func case_strings_trim_space() {
	fmt.Println(strings.TrimSpace(" \t gopher \n "))
}

func case_strings_trim_suffix() {
	fmt.Println(strings.TrimSuffix("gopher:suffix", ":suffix"))
}

func case_strings_basic() {
	fmt.Println(strings.Contains("Hello, World", "World"))
	fmt.Println(strings.HasPrefix("Hello", "He"))
	fmt.Println(strings.HasSuffix("Hello", "lo"))
	fmt.Println(strings.ToUpper("hello"))
	fmt.Println(strings.ToLower("HELLO"))
	fmt.Println(strings.TrimSpace("  hello  "))
	fmt.Println(strings.Repeat("ab", 3))
	fmt.Println(strings.Count("hello", "l"))
	fmt.Println(strings.ReplaceAll("hello world", "world", "Go"))
	fmt.Println(strings.Index("hello", "ll"))
}
