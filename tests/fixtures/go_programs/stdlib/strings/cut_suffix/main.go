package main

import (
	"fmt"
	"strings"
)

func main() {
	before, found := strings.CutSuffix("value.suffix", ".suffix")
	fmt.Println(before, found)
}
