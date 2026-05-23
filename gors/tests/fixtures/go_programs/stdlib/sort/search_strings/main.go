package main

import (
	"fmt"
	"sort"
)

func main() {
	values := []string{"alpha", "delta", "omega"}
	fmt.Println(sort.SearchStrings(values, "charlie"))
}
