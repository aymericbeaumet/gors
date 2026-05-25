package main

import (
	"fmt"
	"example/foo"
	"example/bar"
)

func main() {
	fmt.Println(foo.Value())
	fmt.Println(bar.Value())
	fmt.Println(foo.Value() + bar.Value())
}
