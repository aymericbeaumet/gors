package main

import (
	"fmt"
	"image/gif"
)

func main() {
	fmt.Println("== gif/basic ==")
	fmt.Println(gif.DisposalNone)
	fmt.Println(gif.DisposalBackground)
	fmt.Println(gif.DisposalPrevious)
}
