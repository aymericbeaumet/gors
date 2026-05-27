package main

import "fmt"

func main() {
	fmt.Println("== string/conversions ==")
	case_string_conversions()
	fmt.Println("== string/escapes ==")
	case_string_escapes()
}

func case_string_conversions() {
	s := "hello"
	b := []byte(s)
	fmt.Println(len(b))
	fmt.Println(b[0])

	s2 := string(b)
	fmt.Println(s2)

	r := []rune("world")
	fmt.Println(len(r))

	n := string(65)
	fmt.Println(n)
}

func case_string_escapes() {
	fmt.Println("tab:\there")
	fmt.Println("newline:\nhere")
	fmt.Println("quote:\"here\"")
	fmt.Println("backslash:\\here")
	fmt.Println("null:\x00end")
	fmt.Println("unicode:ABC")
}
