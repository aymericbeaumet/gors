package main

import (
	"fmt"
	"strings"
)

func main() {
	after, found := strings.CutPrefix("prefix:value", "prefix:")
	fmt.Println(after, found)
}
