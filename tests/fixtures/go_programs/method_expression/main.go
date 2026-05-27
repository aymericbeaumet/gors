package main

type Counter struct {
	Count int
}

func (c Counter) Get() int {
	return c.Count
}

func (c Counter) Add(n int) Counter {
	return Counter{Count: c.Count + n}
}

func (c *Counter) Bump(n int) {
	c.Count += n
}

func main() {
	value := Counter{Count: 3}
	if Counter.Get(value) != 3 {
		zero := len("")
		_ = 1 / zero
	}

	value = Counter.Add(value, 4)
	if Counter.Get(value) != 7 {
		zero := len("")
		_ = 1 / zero
	}

	target := Counter{Count: 7}
	ptr := &target
	(*Counter).Bump(ptr, 5)
	if Counter.Get(target) != 12 {
		zero := len("")
		_ = 1 / zero
	}
}
