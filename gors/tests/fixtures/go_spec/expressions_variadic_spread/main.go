package main

func f(args ...int) int {
	return len(args)
}

func main() {
	s := []int{1, 2, 3}
	if f(s...) != 3 {
		panic("variadic spread failed")
	}
}
