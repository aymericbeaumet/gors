package main

import (
	"fmt"
	alias "go_spec_packages_import_aliases/helper"
	. "go_spec_packages_import_aliases/dotpkg"
	_ "go_spec_packages_import_aliases/blankpkg"
)

func main() {
	fmt.Println(alias.Value(), DotValue())
}
