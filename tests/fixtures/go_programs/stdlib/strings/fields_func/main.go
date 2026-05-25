package main

import (
	"fmt"
	"strings"
)

func isSep(r rune) bool {
	return r == ',' || r == ';'
}

func main() {
	fmt.Println(strings.FieldsFunc("alpha,beta;gamma", isSep))
}
