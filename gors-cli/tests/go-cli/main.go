package main

import (
	"bufio"
	"encoding/json"
	"fmt"
	"go/ast"
	"go/parser"
	"go/scanner"
	"go/token"
	"io/ioutil"
	"os"
)

// ./go ast|tokens <filename>
func main() {
	subcommand := os.Args[1]
	filename := os.Args[2]

	w := bufio.NewWriterSize(os.Stdout, 8192)
	defer w.Flush()

	switch subcommand {
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

			// TODO: implement objects/scopes
			ast.Inspect(file, func(node ast.Node) bool {
				switch n := node.(type) {
				case *ast.File:
					n.Scope = nil
				case *ast.Ident:
					n.Obj = nil
				}

				return true
			})

			// TODO: implement ident resolution
			file.Unresolved = nil

			// Using our forked version that prints maps with their keys sorted
			if err := Fprint(w, fset, file, nil); err != nil {
				panic(err)
			}
		}

	case "tokens":
		{
			enc := json.NewEncoder(w)
			enc.SetEscapeHTML(false)

			src, err := ioutil.ReadFile(filename)
			if err != nil {
				panic(err)
			}

			fset := token.NewFileSet()
			file := fset.AddFile(filename, fset.Base(), len(src))

			var s scanner.Scanner
			s.Init(file, src, nil, scanner.ScanComments)

			for {
				pos, tok, lit := s.Scan()

				if err := enc.Encode([]interface{}{file.Position(pos), tok.String(), lit}); err != nil {
					panic(err)
				}

				if tok == token.EOF {
					break
				}
			}

			if s.ErrorCount > 0 {
				panic(fmt.Errorf("%d error(s) occured while scanning", s.ErrorCount))
			}
		}
	}
}