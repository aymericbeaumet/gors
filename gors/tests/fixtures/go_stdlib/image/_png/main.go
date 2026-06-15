package main

import (
	"fmt"
	"image/png"
)

func main() {
	fmt.Println("== png/basic ==")
	fmt.Println(int(png.DefaultCompression))
	fmt.Println(int(png.NoCompression))
	fmt.Println(int(png.BestSpeed))
	fmt.Println(int(png.BestCompression))
}
