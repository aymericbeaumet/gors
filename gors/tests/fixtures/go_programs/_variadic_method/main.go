package main

type Accumulator struct {
	base int
}

func (a Accumulator) Sum(label string, nums ...int) int {
	total := a.base + len(label)
	for _, n := range nums {
		total += n
	}
	return total
}

func (a *Accumulator) Add(nums ...int) {
	for _, n := range nums {
		a.base += n
	}
}

func main() {
	acc := Accumulator{base: 2}
	values := []int{4, 5}

	if acc.Sum("go", 1, 2, 3) != 10 {
		zero := len("")
		_ = 1 / zero
	}
	if acc.Sum("spread", values...) != 17 {
		zero := len("")
		_ = 1 / zero
	}
	if acc.Sum("empty") != 7 {
		zero := len("")
		_ = 1 / zero
	}

	ptr := &acc
	ptr.Add(1, 2)
	ptr.Add(values...)
	if acc.base != 14 {
		zero := len("")
		_ = 1 / zero
	}
}
