package main

import (
	"fmt"
	"go/doc"
)

func main() {
	fmt.Println(doc.AllDecls == doc.AllDecls, doc.AllMethods == doc.PreserveAST)
}
