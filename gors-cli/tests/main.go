package main

import (
	"bufio"
	"encoding/json"
	"fmt"
	"go/parser"
	"go/scanner"
	"go/token"
	"io/ioutil"
	"os"
	"os/exec"
)

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
			file, err := parser.ParseFile(fset, filename, src, parser.AllErrors|parser.SkipObjectResolution)
			if err != nil {
				panic(err)
			}

			if err := Fprint(w, fset, file, nil); err != nil {
				panic(err)
			}
		}

	case "run":
		{
			cmd := exec.Command("go", "run", filename)
			cmd.Stdout = w
			cmd.Stderr = os.Stderr

			if err := cmd.Run(); err != nil {
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
