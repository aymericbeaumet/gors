package main

import (
	"encoding/ascii85"
	"fmt"
)

func main() {
	fmt.Println(ascii85.MaxEncodedLen(0), ascii85.MaxEncodedLen(4), ascii85.MaxEncodedLen(5))
	fmt.Println(ascii85.CorruptInputError.Error(ascii85.CorruptInputError(3)))
}
