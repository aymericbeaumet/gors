package main

import (
	"crypto/rsa"
	"fmt"
)

func main() {
	// gors:stdlib-cover crypto/rsa::PSSSaltLengthAuto crypto/rsa::PSSSaltLengthEqualsHash
	fmt.Println(rsa.PSSSaltLengthAuto, rsa.PSSSaltLengthEqualsHash)
}
