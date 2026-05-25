package main

import (
	"fmt"
	"strings"
)

func main() {
	fmt.Println(strings.SplitN("alpha,beta,gamma", ",", 2))
}
