package main

import (
	"fmt"
	"text/scanner"
)

func main() {
	fmt.Println(scanner.ScanIdents, scanner.ScanInts, scanner.ScanFloats)
}
