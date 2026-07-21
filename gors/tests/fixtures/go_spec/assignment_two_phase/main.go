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

	overlap := []int{0, 1}
	overlap[0], overlap[overlap[0]] = 1, 2

	mapping := map[string]int{"old": 1}
	key := "old"
	key, mapping[key] = "new", mark(3)

	var boxed any = 1
	number := 0
	boxed, number = "changed", 2

	pair := func() (int, int) {
		return 7, 8
	}
	var tupleBox any
	tupleNumber := 0
	tupleBox, tupleNumber = pair()

	commaMap := map[string]int{"value": 11}
	var commaBox any
	commaOK := false
	commaBox, commaOK = commaMap["value"]

	left, right := "left", "right"
	left, right = right, left

	if index != 1 || values[0] != 90 || values[1] != 20 {
		panic("assignment values changed")
	}
	if len(order) != 4 || order[0] != 0 || order[1] != 1 || order[2] != 90 || order[3] != 3 {
		panic("assignment evaluation order changed")
	}
	if overlap[0] != 2 || overlap[1] != 1 {
		panic("assignment targets were not evaluated before writes")
	}
	if key != "new" || mapping["old"] != 3 {
		panic("map assignment key was not evaluated before writes")
	}
	slot := 0
	flags := []bool{false}
	slot, flags[slot] = mapping["old"]
	if slot != 3 || !flags[0] {
		panic("comma-ok assignment target was not evaluated before the lookup")
	}
	if boxed.(string) != "changed" || number != 2 {
		panic("multi-assignment values did not use their destination types")
	}
	if tupleBox.(int) != 7 || tupleNumber != 8 {
		panic("multi-return assignment values did not use their destination types")
	}
	if commaBox.(int) != 11 || !commaOK {
		panic("comma-ok assignment values did not use their destination types")
	}
	first, second := 4, 5
	pointer, replacement := &first, &second
	pointerValues := func() (*int, int) {
		return replacement, 8
	}
	pointer, *pointer = pointerValues()
	if first != 8 || second != 5 || *pointer != 5 {
		panic("pointer assignment target was not evaluated before writes")
	}

	type holder struct {
		value int
	}
	oldHolder, newHolder := holder{value: 4}, holder{value: 5}
	holderPointer, holderReplacement := &oldHolder, &newHolder
	holderPointer, holderPointer.value = holderReplacement, 9
	if oldHolder.value != 9 || newHolder.value != 5 || holderPointer.value != 5 {
		panic("implicit pointer assignment target was not evaluated before writes")
	}

	if left != "right" || right != "left" {
		panic("parallel assignment did not exchange values")
	}
	println("assignment-two-phase: ok")
}
