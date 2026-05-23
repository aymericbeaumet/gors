package main

import (
	"fmt"
	"strings"
)

func main() {
	fmt.Println(strings.TrimSuffix("gopher:suffix", ":suffix"))
}
