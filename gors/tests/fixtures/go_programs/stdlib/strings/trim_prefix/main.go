package main

import (
	"fmt"
	"strings"
)

func main() {
	fmt.Println(strings.TrimPrefix("prefix:gopher", "prefix:"))
}
