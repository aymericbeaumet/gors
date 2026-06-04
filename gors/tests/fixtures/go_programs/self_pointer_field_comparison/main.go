package main

type node struct {
	next *node
}

func (n *node) different() bool {
	return n.next != n
}

func main() {
	var n node
	println(n.different())
}
