package main

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

func fail() {
	zero := len("")
	_ = 1 / zero
}

func check(got int, want int) {
	if got != want {
		fail()
	}
}

func main() {
	valueReceiver := Counter.Add
	parenthesized := (Counter).Add
	pointerReceiver := (*Counter).Inc
	pointerToValueReceiver := (*Counter).Add
	counter := Counter{Value: 3}
	pointer := &Counter{Value: 10}

	check(valueReceiver(counter, 4), 7)
	check(parenthesized(counter, 5), 8)
	check(pointerReceiver(pointer, 2), 12)
	check(pointer.Value, 12)
	check(pointerToValueReceiver(pointer, 6), 18)
	check(pointer.Value, 12)
}
