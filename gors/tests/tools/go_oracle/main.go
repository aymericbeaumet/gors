package main

import (
	"bufio"
	"bytes"
	"encoding/json"
	"fmt"
	"go/parser"
	"go/scanner"
	"go/token"
	"io"
	"os"
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

type fileResult struct {
	Path   string `json:"path"`
	Ok     bool   `json:"ok"`
	Stdout string `json:"stdout,omitempty"`
	Stderr string `json:"stderr,omitempty"`
}

func astOutput(filename string) ([]byte, error) {
	src, err := os.ReadFile(filename)
	if err != nil {
		return nil, err
	}

	fset := token.NewFileSet()
	file, err := parser.ParseFile(fset, filename, src, parser.AllErrors|parser.SkipObjectResolution|parser.ParseComments)
	if err != nil {
		return nil, err
	}

	var out bytes.Buffer
	if err := Fprint(&out, fset, file, nil); err != nil {
		return nil, err
	}
	return out.Bytes(), nil
}

func tokensOutput(filename string) ([]byte, error) {
	src, err := os.ReadFile(filename)
	if err != nil {
		return nil, err
	}

	fset := token.NewFileSet()
	file := fset.AddFile(filename, fset.Base(), len(src))

	var s scanner.Scanner
	s.Init(file, src, nil, scanner.ScanComments)

	var out bytes.Buffer
	var tokenBuf bytes.Buffer
	enc := json.NewEncoder(&tokenBuf)
	enc.SetEscapeHTML(false)

	for {
		pos, tok, lit := s.Scan()

		tokenBuf.Reset()
		if err := enc.Encode([]interface{}{file.Position(pos), tok.String(), lit}); err != nil {
			return nil, err
		}

		output := tokenBuf.Bytes()
		output = unescapeJSONUnicode(output, []byte(`\u2028`), []byte("\u2028"))
		output = unescapeJSONUnicode(output, []byte(`\u2029`), []byte("\u2029"))
		out.Write(output)

		if tok == token.EOF {
			break
		}
	}

	if s.ErrorCount > 0 {
		return nil, fmt.Errorf("%d error(s) occured while scanning", s.ErrorCount)
	}
	return out.Bytes(), nil
}

func commandOutput(command, filename string) ([]byte, error) {
	switch command {
	case "ast":
		return astOutput(filename)
	case "tokens":
		return tokensOutput(filename)
	default:
		return nil, fmt.Errorf("unknown command %q", command)
	}
}

func runSingle(command, filename string, w io.Writer) error {
	output, err := commandOutput(command, filename)
	if err != nil {
		return err
	}
	_, err = w.Write(output)
	return err
}

func runFiles(command string, files []string, w io.Writer) error {
	if len(files) == 1 {
		return runSingle(command, files[0], w)
	}

	enc := json.NewEncoder(w)
	enc.SetEscapeHTML(false)
	for _, filename := range files {
		output, err := commandOutput(command, filename)
		result := fileResult{Path: filename, Ok: err == nil}
		if err == nil {
			result.Stdout = string(output)
		} else {
			result.Stderr = err.Error()
		}
		if err := enc.Encode(result); err != nil {
			return err
		}
	}
	return nil
}

func main() {
	if len(os.Args) < 3 {
		panic("usage: go-oracle <ast|tokens> <file> [file...]")
	}

	subcommand := os.Args[1]

	w := bufio.NewWriterSize(os.Stdout, 8192)
	defer w.Flush()

	switch subcommand {
	case "ast", "tokens":
		if err := runFiles(subcommand, os.Args[2:], w); err != nil {
			panic(err)
		}
	default:
		panic(fmt.Errorf("unknown command %q", subcommand))
	}
}
