package main

import (
	"container/ring"
	"fmt"
)

func fill(r *ring.Ring, values []string) {
	p := r
	for _, value := range values {
		p.Value = value
		p = p.Next()
	}
}

func dump(label string, r *ring.Ring) {
	fmt.Print(label, " ", r.Len(), ":")
	r.Do(func(value any) {
		fmt.Print(" ", value)
	})
	fmt.Println()
}

func main() {
	fmt.Println("== container/ring/basic ==")
	// gors:stdlib-cover container/ring::Ring
	var zero ring.Ring
	zero.Value = "zero"
	// gors:stdlib-cover container/ring::Ring.Next
	// gors:stdlib-cover container/ring::Ring.Prev
	// gors:stdlib-cover container/ring::Ring.Move
	fmt.Println("zero", zero.Len(), zero.Next().Value, zero.Prev().Value, zero.Move(1).Value)
	// gors:stdlib-cover container/ring::New
	fmt.Println("new-zero", ring.New(0) == nil)
	head := ring.New(3)
	fill(head, []string{"a", "b", "c"})
	// gors:stdlib-cover container/ring::Ring.Len
	// gors:stdlib-cover container/ring::Ring.Do
	dump("new", head)
	fmt.Println("neighbors", head.Value, head.Next().Value, head.Prev().Value, head.Move(2).Value, head.Move(-1).Value)
	extra := ring.New(2)
	fill(extra, []string{"x", "y"})
	// gors:stdlib-cover container/ring::Ring.Link
	next := head.Link(extra)
	fmt.Println("link-return", next.Value)
	dump("linked", head)
	// gors:stdlib-cover container/ring::Ring.Unlink
	removed := head.Unlink(2)
	dump("unlinked-head", head)
	dump("unlinked-removed", removed)
	subring := head.Link(head.Move(2))
	dump("same-link-head", head)
	dump("same-link-result", subring)
}
