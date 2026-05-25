package main

import (
	"fmt"
	"strconv"
)

func main() {
	fmt.Println("== strconv/append ==")
	case_strconv_append()
	fmt.Println("== strconv/format ==")
	case_strconv_format()
}

func case_strconv_append() {
	out := []byte("v:")
	out = strconv.AppendBool(out, true)
	out = strconv.AppendInt(out, -42, 10)
	out = strconv.AppendUint(out, 255, 16)
	fmt.Println(string(out))
}

func case_strconv_format() {
	fmt.Println(strconv.FormatBool(true))
	fmt.Println(strconv.FormatInt(255, 16))
	fmt.Println(strconv.FormatUint(255, 2))
	fmt.Println(strconv.Itoa(42))
}
