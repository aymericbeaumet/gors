package main

func note(s string) {
	print(s)
}

func inner() {
	defer func() { note("[inner-defer]") }()
	note("[inner-start]")
	panic("stop")
}

func middle() {
	defer func() { note("[middle-defer]") }()
	inner()
	note("[middle-after-call]") // discarded: must never run
}

func g() (result string) {
	defer func() { note("[g-early-defer]") }() // deferred by G before D: runs after D
	defer func() {                             // this is D
		if r := recover(); r != nil {
			note("[D-recovered]")
			result = "recovered:" + r.(string)
		}
	}()
	middle()
	note("[g-after-call]") // discarded: must never run
	return "normal"
}

func main() {
	// The emitted trace itself locks the panic/defer order, including the
	// absence of the two post-call markers.
	print("handling-panics-sequence: ")
	got := g()
	if got != "recovered:stop" {
		panic("g did not return the named result set by the recovering defer: " + got)
	}
	note("[main-after-g]")
	println()
}
