package main

import "go_spec_negative_unexported_access/lib"

func main() {
	_ = lib.unexported
}

---- lib/lib.go ----
package lib

var unexported = "hidden"

func Exported() string { return unexported }
