package main

import "go_spec_declarations_and_scope_exported/lib"

func main() {
	if lib.Exported != "exported" {
		panic("exported package value changed")
	}
}
