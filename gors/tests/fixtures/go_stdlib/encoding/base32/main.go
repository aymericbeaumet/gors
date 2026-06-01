package main

import (
	"encoding/base32"
	"fmt"
)

func main() {
	fmt.Println("== encoding/base32/constants ==")
	fmt.Println(base32.StdPadding, base32.NoPadding)
	fmt.Println("== encoding/base32/lengths ==")
	fmt.Println(base32.StdEncoding.EncodedLen(5), base32.StdEncoding.DecodedLen(8))
	fmt.Println(base32.HexEncoding.EncodedLen(5), base32.HexEncoding.DecodedLen(8))
	fmt.Println(base32.StdEncoding.WithPadding(base32.NoPadding).EncodedLen(5), base32.StdEncoding.WithPadding(base32.NoPadding).DecodedLen(8))
}
