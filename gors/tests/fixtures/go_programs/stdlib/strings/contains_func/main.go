package main

import (
	"fmt"
	"strings"
)

func isDash(r rune) bool {
	return r == '-'
}

func main() {
	fmt.Println(strings.ContainsFunc("alpha-beta", isDash))
}
