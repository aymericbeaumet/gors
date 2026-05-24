package main

import "fmt"

type bits [2]uint32

func main() {
	var b bits
	b[0] |= 1 << 3
	b[1] = b[0] >> 1
	fmt.Println(b[0], b[1])
}
