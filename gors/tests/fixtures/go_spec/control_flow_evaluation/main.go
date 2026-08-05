package main

func main() {
	tagCalls := 0
	caseOrder := 0
	tag := func() int {
		tagCalls++
		return 2
	}
	probe := func(value int) int {
		caseOrder = caseOrder*10 + value
		return value
	}

	selected := ""
	switch tag() {
	default:
		selected = "default"
	case probe(1), probe(2):
		selected = "matched"
	case probe(3):
		selected = "late"
	}

	postCount := 0
	sum := 0
	for index := 0; index < 4; index, postCount = index+1, postCount+1 {
		if index%2 == 0 {
			continue
		}
		sum += index
	}

	rangeCalls := 0
	values := func() []int {
		rangeCalls++
		return []int{2, 3, 4}
	}
	rangeTotal := 0
	for _, value := range values() {
		rangeTotal += value
	}

	if tagCalls != 1 || caseOrder != 12 || selected != "matched" {
		panic("switch evaluation order changed")
	}
	if postCount != 4 || sum != 4 {
		panic("continue did not execute the for post statement")
	}
	if rangeCalls != 1 || rangeTotal != 9 {
		panic("range expression was not evaluated exactly once")
	}
	println("control-flow-evaluation: ok")
}
