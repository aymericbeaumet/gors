package main

import "fmt"

type Counter struct {
	Value int
}

func (c Counter) Add(delta int) int {
	return c.Value + delta
}

func (c *Counter) Inc(delta int) int {
	c.Value += delta
	return c.Value
}

func main() {
	valueReceiver := Counter.Add
	parenthesized := (Counter).Add
	pointerReceiver := (*Counter).Inc
	pointerToValueReceiver := (*Counter).Add
	counter := Counter{Value: 3}
	pointer := &Counter{Value: 10}

	fmt.Println(
		valueReceiver(counter, 4),
		parenthesized(counter, 5),
		pointerReceiver(pointer, 2),
		pointer.Value,
		pointerToValueReceiver(pointer, 6),
		pointer.Value,
	)
}
