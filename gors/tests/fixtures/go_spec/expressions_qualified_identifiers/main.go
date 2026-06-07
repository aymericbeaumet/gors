package main

import "go_spec_expressions_qualified_identifiers/lib"

func main() {
	if lib.Value != "qualified" {
		panic("qualified package value changed")
	}
}
