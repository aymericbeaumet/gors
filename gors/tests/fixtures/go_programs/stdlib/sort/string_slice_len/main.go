package main

import (
	"fmt"
	"sort"
)

func main() {
	values := []string{"gamma", "alpha", "beta"}
	fmt.Println(sort.StringSlice(values).Len())
}
