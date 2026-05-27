package main

import (
	"fmt"
	"io"
)

func main() {
	fmt.Println(io.SeekStart, io.SeekCurrent, io.SeekEnd)
}
