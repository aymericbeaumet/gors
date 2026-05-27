package main

import "fmt"

type Counter struct {
	Embedded
	Value int
}

func (c Counter) Label() string {
	return "counter"
}

type Embedded struct {
	Name string
}

type Namer interface {
	Label() string
}

func Name(n Namer) string {
	return n.Label()
}

func main() {
	booleans := true && !false
	integer := 4 + 2
	float := 2.5 + 0.5
	complexValue := complex(1, 2)
	text := "gors"
	array := [2]int{1, 2}
	slice := []int{1, 2, 3}
	slice = append(slice, 4)
	structValue := Counter{Embedded: Embedded{Name: "score"}, Value: array[1]}
	var pointer *Counter = &structValue
	mapping := map[string]int{"answer": 42}
	channel := make(chan int, 1)
	channel <- mapping["answer"]
	function := func(value int) int { return value + pointer.Value }
	fmt.Println(booleans, integer, float, real(complexValue), imag(complexValue), text[0], len(slice), structValue.Name, Name(structValue), <-channel, function(3))
}
