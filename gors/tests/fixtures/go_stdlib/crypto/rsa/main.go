package main

import (
	"crypto/rsa"
	"fmt"
)

func main() {
	fmt.Println(rsa.PSSSaltLengthAuto, rsa.PSSSaltLengthEqualsHash)
}
