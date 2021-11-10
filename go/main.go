package main

import (
	"encoding/json"
	"fmt"
	"go/ast"
	"go/parser"
	"go/scanner"
	"go/token"
	"io/ioutil"
	"os"
	"reflect"
	"strings"
)

// ./go ast|tokens <filename>
func main() {
	filename := os.Args[2]

	switch os.Args[1] {
	case "tokens":
		{
			enc := json.NewEncoder(os.Stdout)
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

	case "ast":
		{
			enc := json.NewEncoder(os.Stdout)
			enc.SetEscapeHTML(false)
			enc.SetIndent("", "  ")

			src, err := ioutil.ReadFile(filename)
			if err != nil {
				panic(err)
			}

			fset := token.NewFileSet()
			file, err := parser.ParseFile(fset, filename, src, parser.AllErrors)
			if err != nil {
				panic(err)
			}

			if err := enc.Encode(walk(fset, file)); err != nil {
				panic(err)
			}
		}
	}
}

// https://github.com/DanSnow/astexplorer-go/blob/master/parse.go#L22-L89
func walk(fst *token.FileSet, node interface{}) map[string]interface{} {
	if node == nil {
		return nil
	}

	m := make(map[string]interface{})

	if _, ok := node.(*ast.Scope); ok {
		return nil
	}

	if _, ok := node.(*ast.Object); ok {
		return nil
	}

	val := reflect.ValueOf(node)
	if val.IsNil() {
		return nil
	}
	if val.Kind() == reflect.Ptr {
		val = val.Elem()
	}
	ty := val.Type()
	m["_type"] = ty.Name()
	for i := 0; i < ty.NumField(); i++ {
		field := ty.Field(i)
		val := val.Field(i)
		if strings.HasSuffix(field.Name, "Pos") {
			continue
		}
		switch field.Type.Kind() {
		case reflect.Array, reflect.Slice:
			list := make([]interface{}, 0, val.Len())
			for i := 0; i < val.Len(); i++ {
				if item := walk(fst, val.Index(i).Interface()); item != nil {
					list = append(list, item)
				}
			}
			m[field.Name] = list
		case reflect.Ptr:
			if child := walk(fst, val.Interface()); child != nil {
				m[field.Name] = child
			}
		case reflect.Interface:
			if child := walk(fst, val.Interface()); child != nil {
				m[field.Name] = child
			}
		case reflect.String:
			m[field.Name] = val.String()
		case reflect.Int:
			if field.Type.Name() == "Token" {
				m[field.Name] = token.Token(val.Int()).String()
			} else {
				m[field.Name] = val.Int()
			}
		case reflect.Bool:
			m[field.Name] = val.Bool()
		default:
			fmt.Fprintln(os.Stderr, field)
		}
	}
	if n, ok := node.(ast.Node); ok {
		start := fst.Position(n.Pos())
		end := fst.Position(n.End())
		m["Loc"] = map[string]interface{}{"Start": start, "End": end}
	}
	return m
}
