package main

func double(v int) int {
	result := 0
	{
		if v == 0 {
			goto Skip
		}
		result = v * 2
	Skip:
	}
	return result
}

func bump(n *int) {
	if *n > 10 {
		goto Done
	}
	*n = *n + 5
Done:
}

func main() {

	a := double(3)

	b := double(0)
	n := 4
	bump(&n)
	bump(&n)
	bump(&n)
	switch a {
	case 6:

	}
	for i := 0; i < 1; i++ {

	}
	println(a, b, n)
}
