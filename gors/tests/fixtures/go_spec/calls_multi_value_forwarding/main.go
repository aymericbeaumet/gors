package main



func split(s string, pos int) (string, string) {
	return s[0:pos], s[pos:]
}

func join(s, t string) string {
	return s + t
}

func triple() (int, int, int) {
	return 1, 2, 3
}

func headAndRest(head int, rest ...int) (int, int, int) {
	total := 0
	for _, value := range rest {
		total += value
	}
	return head, total, cap(rest)
}

func spreadAll(values ...int) int {
	return len(values)
}

func main() {
	if join(split("value", 2)) != "value" {
		panic("f(g()) forwarding of two results changed")
	}

	head, total, restCap := headAndRest(triple())
	if head != 1 || total != 5 {
		panic("f(g()) forwarding into a variadic tail changed")
	}
	if restCap != 2 {
		panic("variadic tail slice capacity is not the number of bound arguments")
	}

	if spreadAll(triple()) != 3 {
		panic("f(g()) forwarding of all results into ... changed")
	}
	println("calls-multi-value-forwarding: ok")
}
