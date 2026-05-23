package main

import "fmt"

func main() {
	out := []byte("start:")
	out = fmt.Appendln(out, "value", 7)
	fmt.Print(string(out))
}
