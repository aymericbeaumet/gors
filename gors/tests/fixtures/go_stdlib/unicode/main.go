package main

import (
	"fmt"
	"unicode"
)

func main() {
	fmt.Println("== unicode/classes ==")
	case_unicode_classes()
	fmt.Println("== unicode/digit_graphic ==")
	case_unicode_digit_graphic()
	fmt.Println("== unicode/range_table ==")
	case_unicode_range_table()
	fmt.Println("== unicode/simple_fold ==")
	case_unicode_simple_fold()
	fmt.Println("== unicode/special_case ==")
	case_unicode_special_case()
}

func case_unicode_classes() {
	fmt.Println(unicode.IsLetter('λ'))
	fmt.Println(unicode.IsLower('g'))
	fmt.Println(unicode.IsMark('́'))
	fmt.Println(unicode.IsNumber('9'))
	fmt.Println(unicode.IsPunct('!'))
	fmt.Println(unicode.IsSpace('\t'))
	fmt.Println(unicode.IsSymbol('€'))
	fmt.Println(unicode.IsUpper('G'))
}

func case_unicode_digit_graphic() {
	fmt.Println(unicode.IsDigit('7'))
	fmt.Println(unicode.IsGraphic('λ'))
	fmt.Println(unicode.IsOneOf([]*unicode.RangeTable{unicode.Latin, unicode.Greek}, 'λ'))
	fmt.Println(unicode.IsPrint('\n'))
}

func case_unicode_range_table() {
	fmt.Println(unicode.In('λ', unicode.Greek))
	fmt.Println(unicode.Is(unicode.Latin, 'G'))
}

func case_unicode_simple_fold() {
	fmt.Println(unicode.SimpleFold('A'))
	fmt.Println(unicode.SimpleFold('a'))
}

func case_unicode_special_case() {
	fmt.Println(unicode.To(unicode.UpperCase, 'g'))
	fmt.Println(unicode.ToLower('G'))
	fmt.Println(unicode.ToTitle('g'))
	fmt.Println(unicode.ToUpper('g'))
}
