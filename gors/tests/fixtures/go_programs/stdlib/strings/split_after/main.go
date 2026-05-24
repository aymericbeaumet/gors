package main

import (
	"fmt"
	"strings"
)

func main() {
	fmt.Println(strings.SplitAfter("alpha,beta,gamma", ","))
}
