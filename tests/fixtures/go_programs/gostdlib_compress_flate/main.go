package main

import (
	"compress/flate"
	"fmt"
)

func main() {
	fmt.Println(flate.NoCompression, flate.BestSpeed, flate.BestCompression, flate.DefaultCompression, flate.HuffmanOnly)
}
