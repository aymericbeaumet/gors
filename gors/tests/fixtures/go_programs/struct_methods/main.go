package main

import "fmt"

type Counter struct {
	Count int
}

func NewCounter() Counter {
	return Counter{Count: 0}
}

func (c Counter) Get() int {
	return c.Count
}

func (c Counter) Add(n int) Counter {
	return Counter{Count: c.Count + n}
}

func main() {
	c := NewCounter()
	fmt.Println(c.Get())
	c = c.Add(1)
	fmt.Println(c.Get())
	c = c.Add(5)
	fmt.Println(c.Get())
}
