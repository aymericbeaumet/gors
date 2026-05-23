package main

import (
	"fmt"
	"sort"
)

func main() {
	values := []string{"gamma", "alpha"}
	fmt.Println(sort.StringSlice(values).Less(1, 0))
}
