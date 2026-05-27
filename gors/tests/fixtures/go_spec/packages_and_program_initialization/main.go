package main

import (
	"fmt"

	"go_spec_packages_and_program_initialization/helper"
)

var order = []string{"var"}

func init() {
	order = append(order, "init")
}

func main() {
	fmt.Println(helper.Value(), order[0], order[1])
}
