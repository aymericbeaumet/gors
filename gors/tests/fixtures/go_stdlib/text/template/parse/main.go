package main

import (
	"fmt"
	"text/template/parse"
)

func main() {
	fmt.Println(parse.NodeText == parse.NodeText, parse.NodeAction == parse.NodeBool)
}
