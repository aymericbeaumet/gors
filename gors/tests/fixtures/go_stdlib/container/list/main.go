package main

import (
	"container/list"
	"fmt"
)

func dump(label string, l *list.List) {
	fmt.Print(label, " ", l.Len(), ":")
	for e := l.Front(); e != nil; e = e.Next() {
		fmt.Print(" ", e.Value)
	}
	fmt.Println()
}

func main() {
	fmt.Println("== container/list/basic ==")
	// gors:stdlib-cover container/list::New
	// gors:stdlib-cover container/list::List
	l := list.New()
	// gors:stdlib-cover container/list::List.PushBack
	a := l.PushBack("a")
	b := l.PushBack("b")
	// gors:stdlib-cover container/list::List.PushFront
	c := l.PushFront("c")
	// gors:stdlib-cover container/list::List.Len
	// gors:stdlib-cover container/list::List.Front
	// gors:stdlib-cover container/list::Element.Next
	dump("push", l)
	// gors:stdlib-cover container/list::List.Back
	fmt.Println("front-back", l.Front().Value, l.Back().Value)
	// gors:stdlib-cover container/list::List.InsertAfter
	l.InsertAfter("after-a", a)
	// gors:stdlib-cover container/list::List.InsertBefore
	l.InsertBefore("before-b", b)
	dump("insert", l)
	// gors:stdlib-cover container/list::List.MoveToFront
	l.MoveToFront(b)
	// gors:stdlib-cover container/list::List.MoveToBack
	l.MoveToBack(c)
	dump("move-ends", l)
	// gors:stdlib-cover container/list::List.MoveAfter
	l.MoveAfter(a, b)
	// gors:stdlib-cover container/list::List.MoveBefore
	l.MoveBefore(c, a)
	dump("move-near", l)
	// gors:stdlib-cover container/list::Element
	// gors:stdlib-cover container/list::Element.Prev
	fmt.Println("around-a", a.Prev().Value, a.Value, a.Next().Value)
	// gors:stdlib-cover container/list::List.Remove
	fmt.Println("remove", l.Remove(a))
	dump("removed", l)
	other := list.New()
	other.PushBack("x")
	other.PushBack("y")
	// gors:stdlib-cover container/list::List.PushBackList
	l.PushBackList(other)
	// gors:stdlib-cover container/list::List.PushFrontList
	l.PushFrontList(other)
	dump("lists", l)
	// gors:stdlib-cover container/list::List.Init
	l.Init()
	dump("init", l)
}
