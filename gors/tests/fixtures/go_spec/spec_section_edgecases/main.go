package main

import "unsafe"

type Celsius float64
type Fahrenheit float64

type Holder[T any] struct {
	Value T
}

type Counter struct {
	value int
}

func (c Counter) Value() int {
	return c.value
}

func (c *Counter) Add(value int) {
	c.value += value
}

type Valuer interface {
	Value() int
}

func Choose[T int | string](value T) T {
	return value
}

func semicolonAfterBlockComment() int {
	value := 1 /*
		a block comment containing a newline acts like a newline
	*/
	value++
	return value
}

func classify(value int) (label string) {
Exit:
	switch {
	case value < 0:
		label = "negative"
		break Exit
	case value == 0:
		label = "zero"
	default:
		label = "positive"
	}
	return
}

func emptyStatementValue() int {
	;
	return (1 + 2)
}

func main() {
	lettersAndDigits := "AZaz09_"
	tokenBoundary := semicolonAfterBlockComment()

	var zero Counter
	named := Celsius(10.5)
	converted := float64(named)
	temperature := Fahrenheit(32)
	chosenInt := Choose(4)
	chosenString := Choose("type")

	holder := Holder[string]{Value: "holder"}
	var valuer Valuer = Counter{value: 5}
	pointerCounter := &Counter{value: 7}
	pointerCounter.Add(3)

	channel := make(chan int, 1)
	channel <- 11
	received := <-channel

	closed := make(chan int, 1)
	closed <- 13
	close(closed)
	closedValue, okBeforeDrain := <-closed
	_, okAfterDrain := <-closed

	values := []int{0, 1, 2, 3, 4}
	fullSlice := values[1:3:4]

	mapping := map[string]int{"a": 1, "b": 2}
	delete(mapping, "a")

	pointer := new(int)
	*pointer = int(unsafe.Alignof(*pointer))

	var dynamic any = 14
	asserted, assertionOK := dynamic.(int)
	failed, failedOK := dynamic.(string)

	arithmetic := (((5+3)*2/4)%3 + (6 & 3) + (1 | 2) + (7 ^ 3) + (8 >> 1) + (1 << 2) + (7 &^ 2))
	logical := true || false && false

	if lettersAndDigits != "AZaz09_" || tokenBoundary != 2 {
		panic("lexical edgecase changed")
	}
	if zero.Value() != 0 || converted <= 10 || float64(temperature) != 32 {
		panic("basic type edgecase changed")
	}
	if chosenInt != 4 || chosenString != "type" || holder.Value != "holder" {
		panic("generic edgecase changed")
	}
	if valuer.Value() != 5 || pointerCounter.Value() != 10 {
		panic("interface or pointer receiver edgecase changed")
	}
	if received != 11 || closedValue != 13 || !okBeforeDrain || okAfterDrain {
		panic("channel edgecase changed")
	}
	if len(fullSlice) != 2 || cap(fullSlice) != 3 || len(mapping) != 1 {
		panic("slice or map edgecase changed")
	}
	if *pointer <= 0 {
		panic("unsafe align edgecase changed")
	}
	if asserted != 14 || !assertionOK || failed != "" || failedOK {
		panic("type assertion edgecase changed")
	}
	if arithmetic != 23 || !logical {
		panic("operator edgecase changed")
	}
	if classify(-1) != "negative" || classify(0) != "zero" || emptyStatementValue() != 3 {
		panic("control-flow edgecase changed")
	}
	if packageNumber != 20 {
		panic("package init edgecase changed")
	}
}
