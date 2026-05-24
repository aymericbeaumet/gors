package main

import (
	"fmt"
	"strings"
)

func mapRune(r rune) rune {
	if r == 'a' {
		return 'A'
	}
	return r
}

func main() {
	fmt.Println(strings.Map(mapRune, "banana"))
}
