package main

import (
	"crypto/rc4"
	"fmt"
)

func main() {
	fmt.Println("== rc4/basic ==")

	// gors:stdlib-cover crypto/rc4::Cipher
	var cipher *rc4.Cipher
	fmt.Println(cipher == nil)

	// gors:stdlib-cover crypto/rc4::KeySizeError crypto/rc4::KeySizeError.Error
	fmt.Println(rc4.KeySizeError.Error(rc4.KeySizeError(2)))
}
