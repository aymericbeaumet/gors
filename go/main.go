package main

import (
	"go/ast"
	"go/parser"
	"go/token"
	"os"
)

func main() {
	filename := os.Args[1]
	fset := token.NewFileSet()

	file, err := parser.ParseFile(fset, filename, nil, parser.AllErrors)
	if err != nil {
		panic(err)
	}

	if err := ast.Print(fset, file); err != nil {
		panic(err)
	}
}
