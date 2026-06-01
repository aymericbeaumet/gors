package main

import (
	"fmt"
	"unsafe"
)

func main() {
	var x int
	size := unsafe.Sizeof(x)
	fmt.Println(size > 0)

	align := unsafe.Alignof(x)
	fmt.Println(align > 0)

	type struct1 struct {
		a byte
		b int
	}
	var s struct1
	offset := unsafe.Offsetof(s.b)
	fmt.Println(offset > 0)

	arr := [2]int{10, 20}
	ptr := unsafe.Pointer(&arr[0])
	val := *(*int)(ptr)
	fmt.Println(val == 10)
}
