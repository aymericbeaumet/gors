package main

import (
	"fmt"
	"go/parser"
)

func main() {
	fmt.Println(parser.ParseComments == parser.ParseComments, parser.AllErrors == parser.AllErrors)
}
