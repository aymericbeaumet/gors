package main

import "unsafe"

type node struct {
	next *node
}

func (n *node) different() bool {
	return n.next != n
}

func (n *node) initialize() {
	if n.next == nil {
		n.next = (*node)(unsafe.Pointer(n))
	}
}

func main() {
	var n node
	println(n.different())
	n.initialize()
	println(n.different())
}
