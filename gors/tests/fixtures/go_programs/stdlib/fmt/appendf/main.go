package main

import "fmt"

func main() {
	out := []byte("start:")
	out = fmt.Appendf(out, "%s=%d", "value", 7)
	fmt.Println(string(out))
}
