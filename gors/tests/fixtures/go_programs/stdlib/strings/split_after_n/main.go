package main

import (
	"fmt"
	"strings"
)

func main() {
	fmt.Println(strings.SplitAfterN("alpha,beta,gamma", ",", 2))
}
