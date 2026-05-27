package main

import (
	"crypto/subtle"
	"fmt"
)

func main() {
	fmt.Println("== subtle/basic ==")
	left := []byte{1, 2, 3}
	dst := make([]byte, 3)
	fmt.Println(subtle.ConstantTimeCompare(left, left))
	fmt.Println(subtle.ConstantTimeByteEq(7, 7))
	fmt.Println(subtle.ConstantTimeEq(4, 5))
	fmt.Println(subtle.ConstantTimeLessOrEq(4, 5))
	fmt.Println(subtle.ConstantTimeSelect(1, 10, 20))
	subtle.ConstantTimeCopy(1, dst, left)
	fmt.Println(dst[0])
	subtle.WithDataIndependentTiming(func() {
		fmt.Println("timing")
	})
}
