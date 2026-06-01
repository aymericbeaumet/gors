package main

import (
	"encoding/base64"
	"fmt"
)

func main() {
	fmt.Println("== encoding/base64/lengths ==")
	fmt.Println(base64.StdPadding, base64.NoPadding)
	fmt.Println(base64.StdEncoding.EncodedLen(4), base64.StdEncoding.DecodedLen(8))
	fmt.Println(base64.RawStdEncoding.EncodedLen(4), base64.RawStdEncoding.DecodedLen(6))
}
