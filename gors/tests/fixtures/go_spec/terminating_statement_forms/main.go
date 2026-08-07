package main

func infiniteFor(v int) int {
	for {
		if v > 0 {
			return v + 1
		}
		v++
	}
}

func switchTerm(v int) string {
	switch v {
	case 1:
		fallthrough
	default:
		return "reached"
	}
}

func ifElseTerm(b bool) int {
	if b {
		return 1
	} else {
		return 2
	}
}

func panicTerm() int {
	panic("panic is terminating")
}

func selectTerm(ch chan int) int {
	select {
	case v := <-ch:
		return v * 2
	}
}

func blockTerm() int {
	{
		return 6
	}
}

func labeledTerm(v int) int {
	if v > 0 {
		goto Done
	}
	v = -v
Done:
	return v + 10
}

func emptyAfterReturn() int {
	return 7
	;
}

func recoveredPanic() (r int) {
	defer func() {
		if rec := recover(); rec != nil {
			r = 42
		}
	}()
	return panicTerm()
}

func main() {
	if infiniteFor(0) != 2 {
		panic("condition-less for was not terminating")
	}
	if switchTerm(1) != "reached" || switchTerm(9) != "reached" {
		panic("terminating switch changed")
	}
	if ifElseTerm(true) != 1 || ifElseTerm(false) != 2 {
		panic("terminating if/else changed")
	}
	if recoveredPanic() != 42 {
		panic("terminating panic changed")
	}
	ch := make(chan int, 1)
	ch <- 21
	if selectTerm(ch) != 42 {
		panic("terminating select changed")
	}
	if blockTerm() != 6 {
		panic("terminating block changed")
	}
	if labeledTerm(3) != 13 || labeledTerm(-4) != 14 {
		panic("terminating labeled statement changed")
	}
	if emptyAfterReturn() != 7 {
		panic("empty statement after return changed")
	}
	println("terminating-statement-forms: ok")
}
