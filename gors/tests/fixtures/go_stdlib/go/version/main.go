package main

import (
	"fmt"
	"go/version"
)

func main() {
	fmt.Println("== go/version/basic ==")
	fmt.Println(version.IsValid("go1.26"))
	fmt.Println(version.IsValid("1.26"))
	fmt.Println(version.Compare("go1.25", "go1.26"))
	fmt.Println(version.Lang("go1.26.3"))
}
