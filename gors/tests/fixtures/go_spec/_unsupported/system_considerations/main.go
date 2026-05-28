package main

import (
	"fmt"
	"unsafe"
)

func main() {
	var x int
	// test Sizeof
	size := unsafe.Sizeof(x)
	fmt.Println(size > 0)

	// test Alignof
	align := unsafe.Alignof(x)
	fmt.Println(align > 0)

	// test Offsetof
	type struct1 struct {
		a byte
		b int
	}
	var s struct1
	offset := unsafe.Offsetof(s.b)
	fmt.Println(offset > 0)

	// test Pointer
	arr := [2]int{10, 20}
	ptr := unsafe.Pointer(&arr[0])
	val := *(*int)(ptr)
	fmt.Println(val == 10)
}
