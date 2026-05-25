package main

import (
	"fmt"
	"regexp/syntax"
)

func main() {
	fmt.Println(syntax.IsWordChar('A'), syntax.IsWordChar('_'), syntax.IsWordChar('-'))
}
