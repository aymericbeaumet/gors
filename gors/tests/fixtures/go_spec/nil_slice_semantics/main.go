package main

func main() {
	var pending []int
	if pending != nil {
		panic("uninitialized slice is not nil")
	}
	if len(pending) != 0 || cap(pending) != 0 {
		panic("nil slice len/cap changed")
	}
	for range pending {
		panic("range over nil slice iterated")
	}
	if pending[:0] != nil {
		panic("slicing a nil slice produced a non-nil slice")
	}
	grown := append(pending, 4)
	if pending != nil || grown == nil || len(grown) != 1 || grown[0] != 4 {
		panic("append to nil slice changed")
	}
	emptied := grown[:0]
	if emptied == nil || len(emptied) != 0 {
		panic("reslicing a non-nil slice produced nil")
	}
	println(pending == nil, grown[0], len(grown), cap(grown) >= 1, emptied == nil)
}
