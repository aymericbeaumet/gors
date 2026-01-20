package main

import (
	"bufio"
	"bytes"
	"encoding/json"
	"fmt"
	"go/parser"
	"go/scanner"
	"go/token"
	"io/ioutil"
	"os"
	"os/exec"
)

// unescapeJSONUnicode replaces JSON Unicode escapes (like \u2028) with raw UTF-8,
// but only when they are actual JSON escapes (preceded by odd number of backslashes).
// This correctly handles \\u2028 (escaped backslash + u2028) by not replacing it.
func unescapeJSONUnicode(data, escape, replacement []byte) []byte {
	result := make([]byte, 0, len(data))
	i := 0
	for i < len(data) {
		idx := bytes.Index(data[i:], escape)
		if idx == -1 {
			result = append(result, data[i:]...)
			break
		}
		
		// Count backslashes before this escape sequence
		backslashes := 0
		for j := i + idx - 1; j >= 0 && data[j] == '\\'; j-- {
			backslashes++
		}
		
		// If odd number of backslashes before \u, this is a JSON Unicode escape
		// If even number (including 0), the backslash is escaped, so \u is literal
		if backslashes%2 == 0 {
			// This is a real JSON Unicode escape - replace it
			result = append(result, data[i:i+idx]...)
			result = append(result, replacement...)
			i = i + idx + len(escape)
		} else {
			// The backslash is escaped, keep the escape sequence as-is
			result = append(result, data[i:i+idx+len(escape)]...)
			i = i + idx + len(escape)
		}
	}
	return result
}

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
			file, err := parser.ParseFile(fset, filename, src, parser.AllErrors|parser.SkipObjectResolution|parser.ParseComments)
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
			src, err := ioutil.ReadFile(filename)
			if err != nil {
				panic(err)
			}

			fset := token.NewFileSet()
			file := fset.AddFile(filename, fset.Base(), len(src))

			var s scanner.Scanner
			s.Init(file, src, nil, scanner.ScanComments)

			// Use a buffer so we can post-process the JSON output
			var buf bytes.Buffer
			enc := json.NewEncoder(&buf)
			enc.SetEscapeHTML(false)

			for {
				pos, tok, lit := s.Scan()

				buf.Reset()
				if err := enc.Encode([]interface{}{file.Position(pos), tok.String(), lit}); err != nil {
					panic(err)
				}

				// Unescape \u2028 and \u2029 to match serde_json's default behavior
				// Go's json encoder escapes these for HTML safety, but we want raw UTF-8
				// Note: Only unescape JSON Unicode escapes, not escaped backslashes
				// In JSON: \u2028 means U+2028, but \\u2028 means literal \u2028
				output := buf.Bytes()
				output = unescapeJSONUnicode(output, []byte(`\u2028`), []byte("\u2028"))
				output = unescapeJSONUnicode(output, []byte(`\u2029`), []byte("\u2029"))
				w.Write(output)

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
