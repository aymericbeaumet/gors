package main

import "unsafe"

func main() {
	var x int
	size := unsafe.Sizeof(x)
	if size == 0 {
		panic("unsafe.Sizeof returned zero")
	}

	align := unsafe.Alignof(x)
	if align == 0 {
		panic("unsafe.Alignof returned zero")
	}

	type struct1 struct {
		a byte
		b int
	}
	var s struct1
	offset := unsafe.Offsetof(s.b)
	if offset == 0 {
		panic("unsafe.Offsetof returned zero")
	}

	arr := [2]int{10, 20}
	ptr := unsafe.Pointer(&arr[0])
	val := *(*int)(ptr)
	if val != 10 {
		panic("unsafe.Pointer address round trip mismatch")
	}
}
