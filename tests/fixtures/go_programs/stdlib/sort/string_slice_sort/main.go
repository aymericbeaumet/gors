package main

import (
	"fmt"
	"sort"
)

func main() {
	values := []string{"gamma", "alpha", "beta"}
	sort.StringSlice(values).Sort()
	fmt.Println(values)
}
