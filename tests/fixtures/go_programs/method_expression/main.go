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

func (c Counter) Sum(nums ...int) int {
	total := c.Count
	for _, n := range nums {
		total += n
	}
	return total
}

func (c *Counter) AddAll(nums ...int) {
	for _, n := range nums {
		c.Count += n
	}
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
	values := []int{2, 3}
	if Counter.Sum(value, 1, 2) != 10 {
		zero := len("")
		_ = 1 / zero
	}
	if Counter.Sum(value, values...) != 12 {
		zero := len("")
		_ = 1 / zero
	}
	get := value.Get
	if get() != 7 {
		zero := len("")
		_ = 1 / zero
	}
	add := value.Add
	if add(5).Count != 12 {
		zero := len("")
		_ = 1 / zero
	}

	target := Counter{Count: 7}
	ptr := &target
	(*Counter).Bump(ptr, 5)
	bump := ptr.Bump
	bump(5)
	(*Counter).AddAll(ptr, 1, 2)
	(*Counter).AddAll(ptr, values...)
	if Counter.Get(target) != 25 {
		zero := len("")
		_ = 1 / zero
	}
}
