package main

func main() {
	values := []int{10, 20}
	order := []int{}
	mark := func(value int) int {
		order = append(order, value)
		return value
	}

	index := 0
	index, values[mark(index)] = mark(1), mark(90)

	left, right := "left", "right"
	left, right = right, left

	if index != 1 || values[0] != 90 || values[1] != 20 {
		panic("assignment values changed")
	}
	if len(order) != 3 || order[0] != 0 || order[1] != 1 || order[2] != 90 {
		panic("assignment evaluation order changed")
	}
	if left != "right" || right != "left" {
		panic("parallel assignment did not exchange values")
	}
	println("assignment-evaluation-order: ok")
}
