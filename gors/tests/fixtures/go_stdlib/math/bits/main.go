package main

import (
	"fmt"
	"math/bits"
)

func main() {
	fmt.Println(bits.OnesCount8(0xf0), bits.OnesCount16(0x0f0f), bits.OnesCount32(0xf0f0f0f0))
	fmt.Println(bits.Len(1024), bits.Len64(1<<63), bits.LeadingZeros16(0x0100))
	fmt.Println(bits.RotateLeft8(0b10000001, 1), bits.TrailingZeros16(0x0100))
}
