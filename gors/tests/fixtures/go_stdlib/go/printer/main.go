package main

import (
	"fmt"
	"go/printer"
)

func main() {
	fmt.Println(printer.RawFormat == printer.RawFormat, printer.UseSpaces == printer.TabIndent)
}
