package main

import (
	"fmt"
	"unsafe"
)

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

	fmt.Println(
		lettersAndDigits,
		tokenBoundary,
		zero.Value(),
		converted > 10,
		float64(temperature),
		chosenInt,
		chosenString,
		holder.Value,
		valuer.Value(),
		pointerCounter.Value(),
		received,
		closedValue,
		okBeforeDrain,
		okAfterDrain,
		len(fullSlice),
		cap(fullSlice),
		len(mapping),
		*pointer > 0,
		asserted,
		assertionOK,
		failed == "",
		failedOK,
		arithmetic,
		logical,
		classify(-1),
		classify(0),
		emptyStatementValue(),
		packageNumber,
	)
}
