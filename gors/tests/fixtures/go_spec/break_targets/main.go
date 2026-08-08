package main

func main() {
	// Plain break terminates only the innermost for.
	trail := []int{}
	for i := 0; i < 3; i++ {
		for j := 0; j < 10; j++ {
			if j == 2 {
				break
			}
			trail = append(trail, i*10+j)
		}
	}
	if len(trail) != 6 || trail[0] != 0 || trail[1] != 1 ||
		trail[2] != 10 || trail[3] != 11 || trail[4] != 20 || trail[5] != 21 {
		panic("plain break did not target the innermost for")
	}

	// break inside a switch terminates the switch, not the enclosing for.
	afterSwitch := 0
	iterations := 0
	for i := 0; i < 3; i++ {
		iterations++
		switch i {
		case 1:
			break
		default:
		}
		afterSwitch++
	}
	if iterations != 3 || afterSwitch != 3 {
		panic("break in switch terminated the enclosing for")
	}

	// break inside a select terminates the select, not the enclosing for.
	ch := make(chan int, 1)
	selectLoops := 0
	afterSelect := 0
	for i := 0; i < 2; i++ {
		selectLoops++
		ch <- i
		select {
		case <-ch:
			break
		}
		afterSelect++
	}
	if selectLoops != 2 || afterSelect != 2 {
		panic("break in select terminated the enclosing for")
	}

	// Labeled break from inside a switch terminates the labeled for.
	steps := 0
Loop:
	for i := 0; i < 10; i++ {
		switch {
		case i == 2:
			break Loop
		default:
			steps++
		}
	}
	if steps != 2 {
		panic("labeled break did not terminate the labeled for")
	}

	// While-style condition-only for, terminated by its condition.
	n := 1
	for n < 100 {
		n *= 2
	}
	if n != 128 {
		panic("condition-only for changed")
	}

	// Infinite for terminated by break.
	count := 0
	for {
		count++
		if count == 5 {
			break
		}
	}
	if count != 5 {
		panic("break out of infinite for changed")
	}

	println("break-targets: ok")
}
