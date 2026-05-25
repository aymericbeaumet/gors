package main

import (
	"fmt"
	"strings"
)

func isBang(r rune) bool {
	return r == '!'
}

func main() {
	fmt.Println(strings.TrimFunc("!!gopher!!", isBang))
}
