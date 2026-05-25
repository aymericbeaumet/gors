package main

import (
	"fmt"
)

func main() {
	out := []byte("start:")
	out = fmt.Append(out, "value", 7)
	fmt.Println(string(out))
}
