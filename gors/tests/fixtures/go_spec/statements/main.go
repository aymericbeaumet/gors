package main

func main() {
	total := 0
	deferredValue := total == 6
	defer func(value bool) {
		if value {
			panic("defer argument evaluation changed")
		}
	}(deferredValue)
	for i := 0; i < 4; i++ {
		if i == 1 {
			continue
		}
		total += i
	}
	switch total {
	case 5:
		total++
		fallthrough
	case 6:
		total++
	default:
		total = 0
	}
	labeled := 0
Outer:
	for x := 0; x < 3; x++ {
		for y := 0; y < 3; y++ {
			if y == 1 {
				continue Outer
			}
			labeled += x + y
		}
	}
	var dynamic any = 2
	switch value := dynamic.(type) {
	case int:
		total += value
	default:
		total = -1
	}
	nilMatched := 0
	var nilDynamic any
	switch nilDynamic.(type) {
	case nil:
		nilMatched = 1
	default:
		nilMatched = -1
	}
	values := []int{1, 2, 3}
	for _, value := range values {
		total += value
	}
	channel := make(chan int, 1)
	channel <- total
	select {
	case received := <-channel:
		total = received
	default:
		total = -1
	}
Label:
	total++
	if total < 13 {
		goto Label
	}
	go func() {}()
	if total != 16 {
		panic("statement total changed")
	}
	if labeled != 3 {
		panic("labeled continue result changed")
	}
	if nilMatched != 1 {
		panic("nil type switch result changed")
	}
}
