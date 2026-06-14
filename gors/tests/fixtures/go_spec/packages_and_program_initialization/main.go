package main

import (
	"go_spec_packages_and_program_initialization/helper"
)

var order = []string{"var"}

func init() {
	order = append(order, "init")
}

func main() {
	if helper.Value() != "helper" {
		panic("imported package init did not run")
	}
	if len(order) != 2 || order[0] != "var" || order[1] != "init" {
		panic("main package initialization order mismatch")
	}
}
