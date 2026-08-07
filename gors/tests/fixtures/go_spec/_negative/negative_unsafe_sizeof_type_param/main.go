package main

import "unsafe"

func constSize[T any](x T) uintptr {
	const size = unsafe.Sizeof(x)
	return size
}

func main() {
	_ = constSize(0)
}
