package main

import (
	"fmt"
	"sort"
)

func main() {
	values := []string{"gamma", "alpha", "beta"}
	sort.Stable(sort.StringSlice(values))
	fmt.Println(values)
}
