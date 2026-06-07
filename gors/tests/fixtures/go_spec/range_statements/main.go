package main

func ints(yield func(int) bool) {
	for i := 0; i < 5; i++ {
		if !yield(i) {
			return
		}
	}
}

func pairs(yield func(string, int) bool) {
	if !yield("left", 2) {
		return
	}
	yield("right", 4)
}

func ticks(yield func() bool) {
	for i := 0; i < 5; i++ {
		if !yield() {
			return
		}
	}
}

func main() {
	array := [3]int{2, 4, 6}
	arrayTotal := 0
	for index, value := range array {
		arrayTotal += index + value
	}

	slice := []int{1, 3, 5}
	sliceTotal := 0
	for index := range slice {
		sliceTotal += index
	}

	mapping := map[string]int{"a": 10, "b": 20}
	mapTotal := 0
	for key, value := range mapping {
		mapTotal += len(key) + value
	}

	stringIndexTotal := 0
	stringRuneTotal := 0
	for index, r := range "a¢日" {
		stringIndexTotal += index
		stringRuneTotal += int(r)
	}

	channel := make(chan int, 3)
	channel <- 7
	channel <- 8
	close(channel)
	channelTotal := 0
	for value := range channel {
		channelTotal += value
	}

	intTotal := 0
	for i := range 4 {
		intTotal += i
	}
	intCount := 0
	for range 3 {
		intCount++
	}
	typedIntTotal := 0
	var smallLimit uint8 = 4
	var lastSmall uint8
	for lastSmall = range smallLimit {
		typedIntTotal += int(lastSmall)
	}
	typedUntypedTotal := 0
	var fromUntyped uint8
	for fromUntyped = range 4 {
		typedUntypedTotal += int(fromUntyped)
	}
	negativeCount := 0
	for range -2 {
		negativeCount++
	}

	funcTotal := 0
	for value := range ints {
		if value == 3 {
			break
		}
		funcTotal += value
	}

	pairTotal := 0
	for key, value := range pairs {
		pairTotal += len(key) + value
	}

	tickCount := 0
	for range ticks {
		tickCount++
		if tickCount == 3 {
			break
		}
	}

	if arrayTotal != 15 || sliceTotal != 3 || mapTotal != 32 {
		panic("array, slice, or map range changed")
	}
	if stringIndexTotal != 4 || stringRuneTotal != 26344 {
		panic("string range changed")
	}
	if channelTotal != 15 {
		panic("channel range changed")
	}
	if intTotal != 6 || intCount != 3 {
		panic("integer range changed")
	}
	if typedIntTotal != 6 || lastSmall != 3 {
		panic("typed integer range changed")
	}
	if typedUntypedTotal != 6 || fromUntyped != 3 {
		panic("untyped integer range assignment changed")
	}
	if negativeCount != 0 {
		panic("negative integer range changed")
	}
	if funcTotal != 3 || pairTotal != 15 || tickCount != 3 {
		panic("function range changed")
	}
}
