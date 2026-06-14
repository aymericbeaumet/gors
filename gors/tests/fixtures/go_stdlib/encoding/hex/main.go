package main

import (
	"encoding/hex"
	"fmt"
)

func main() {
	fmt.Println("== hex/basic ==")
	encoded := hex.EncodeToString([]byte("gors"))
	fmt.Println(encoded)
	dst := make([]byte, hex.EncodedLen(2))
	hex.Encode(dst, []byte{0xab, 0xcd})
	fmt.Println(string(dst))
	fmt.Println(hex.DecodedLen(8))
}
