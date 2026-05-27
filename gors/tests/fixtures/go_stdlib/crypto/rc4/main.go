package main

import (
	"crypto/rc4"
	"fmt"
)

func main() {
	fmt.Println("== rc4/basic ==")
	var cipher *rc4.Cipher
	fmt.Println(cipher == nil)
	fmt.Println(rc4.KeySizeError(2).Error())
}
