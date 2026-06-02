package main

import (
	"container/heap"
	"fmt"
)

type IntHeap []int

func (h IntHeap) Len() int {
	return len(h)
}

func (h IntHeap) Less(i, j int) bool {
	return h[i] < h[j]
}

func (h IntHeap) Swap(i, j int) {
	h[i], h[j] = h[j], h[i]
}

func (h *IntHeap) Push(x any) {
	*h = IntHeap(append(*h, x.(int)))
}

func (h *IntHeap) Pop() any {
	old := *h
	n := len(old)
	x := old[n-1]
	*h = IntHeap(old[0 : n-1])
	return x
}

func main() {
	fmt.Println("== container/heap/intheap ==")
	// gors:stdlib-cover container/heap::Interface
	h := &IntHeap{2, 1, 5}
	// gors:stdlib-cover container/heap::Init
	heap.Init(h)
	// gors:stdlib-cover container/heap::Push
	heap.Push(h, 3)
	fmt.Println("min", (*h)[0])
	(*h)[2] = 0
	// gors:stdlib-cover container/heap::Fix
	heap.Fix(h, 2)
	fmt.Println("fix", (*h)[0])
	// gors:stdlib-cover container/heap::Remove
	fmt.Println("remove", heap.Remove(h, 1).(int))
	for h.Len() > 0 {
		// gors:stdlib-cover container/heap::Pop
		fmt.Print(heap.Pop(h).(int), " ")
	}
	fmt.Println()
}
