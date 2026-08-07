package main

type counter struct{ n int }

func (c counter) value() int { return c.n }

func main() {
	sum := 1 +
		2 +
		3
	logical := true &&
		true ||
		false
	c := counter{n: 40}
	viaDot := c.
		value()
	list := []int{
		1,
		2,
	}
	total := 0
	for index := 0; index < len(list); index++ {
		total += list[index]
	}
	if x := viaDot; x == 40 {
		total += x
	}
	if sum != 6 {
		panic("a line ending in an operator must not receive an inserted semicolon")
	}
	if !logical {
		panic("multi-line boolean expression changed")
	}
	if viaDot != 40 {
		panic("a line ending in a period must continue the selector on the next line")
	}
	if total != 43 {
		panic("single-line statements before a closing brace must omit the semicolon")
	}
	println(sum, viaDot, total)
}
