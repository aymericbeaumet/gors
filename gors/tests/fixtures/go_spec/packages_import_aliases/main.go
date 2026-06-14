package main

import (
	alias "go_spec_packages_import_aliases/helper"
	. "go_spec_packages_import_aliases/dotpkg"
	_ "go_spec_packages_import_aliases/blankpkg"
)

func main() {
	if alias.Value() != "alias" {
		panic("aliased import value changed")
	}
	if DotValue() != "dot" {
		panic("dot import value changed")
	}
}
