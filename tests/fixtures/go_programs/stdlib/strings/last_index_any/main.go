package main

import (
	"fmt"
	"strings"
)

func main() {
	fmt.Println(strings.LastIndexAny("alpha-beta", "-x"))
}
