package main

import (
	"fmt"
	"strings"
)

func main() {
	before, after, found := strings.Cut("key=value", "=")
	fmt.Println(before, after, found)
}
