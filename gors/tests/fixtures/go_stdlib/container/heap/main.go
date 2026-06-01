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
	h := &IntHeap{2, 1, 5}
	heap.Init(h)
	heap.Push(h, 3)
	fmt.Println("min", (*h)[0])
	fmt.Println("remove", heap.Remove(h, 1).(int))
	for h.Len() > 0 {
		fmt.Print(heap.Pop(h).(int), " ")
	}
	fmt.Println()
}
