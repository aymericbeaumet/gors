package main

import (
	"go/ast"
	"go/parser"
	"go/token"
	"os"
)

func main() {
	fset := token.NewFileSet()

	file, err := parser.ParseFile(fset, os.Args[1], nil, parser.AllErrors)
	if err != nil {
		panic(err)
	}

	if err := ast.Print(fset, file); err != nil {
		panic(err)
	}
}
