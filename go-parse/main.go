package main

import (
	"encoding/json"
	"go/ast"
	"go/parser"
	"go/scanner"
	"go/token"
	"io/ioutil"
	"os"
)

type Token struct {
	Position token.Position
	Token    string
	Literal  string
}

// ./go-parse ast|tokens <filename>
func main() {
	filename := os.Args[2]

	switch os.Args[1] {
	case "ast":
		{
			src, err := ioutil.ReadFile(filename)
			if err != nil {
				panic(err)
			}

			fset := token.NewFileSet()
			file, err := parser.ParseFile(fset, filename, src, parser.AllErrors)
			if err != nil {
				panic(err)
			}

			if err := ast.Print(fset, file); err != nil {
				panic(err)
			}
		}

	case "tokens":
		{
			src, err := ioutil.ReadFile(filename)
			if err != nil {
				panic(err)
			}

			var s scanner.Scanner
			fset := token.NewFileSet()
			file := fset.AddFile(filename, fset.Base(), len(src))
			s.Init(file, src, nil, scanner.ScanComments)

			var tokens []*Token
			for {
				pos, tok, lit := s.Scan()
				tokens = append(tokens, &Token{file.Position(pos), tok.String(), lit})
				if tok == token.EOF {
					break
				}
			}

			if err := json.NewEncoder(os.Stdout).Encode(tokens); err != nil {
				panic(err)
			}
		}
	}
}
