package main

import (
	"fmt"
	"strings"
)

func main() {
	parts := []string{"alpha", "beta", "gamma"}
	fmt.Println(strings.Join(parts, ":"))
}
