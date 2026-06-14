package main

import (
	"fmt"
	"go/scanner"
)

func main() {
	fmt.Println(scanner.ScanComments == scanner.ScanComments)
}
