package main

import (
	"crypto/hmac"
	"fmt"
)

func main() {
	fmt.Println("== hmac/basic ==")
	// gors:stdlib-cover crypto/hmac::Equal
	left := []byte{1, 2, 3, 4}
	right := []byte{1, 2, 3, 4}
	other := []byte{1, 2, 3, 5}
	fmt.Println(hmac.Equal(left, right))
	fmt.Println(hmac.Equal(left, other))
}
