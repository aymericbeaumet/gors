package main

import "fmt"

type Bag struct {
	values []int
}

func (b Bag) At(index int) int {
	return b.values[index]
}

func variadic(values ...int) int {
	total := 0
	for _, value := range values {
		total += value
	}
	return total
}

func main() {
	bag := Bag{values: []int{1, 2, 3}}
	values := bag.values[1:3]
	mapping := map[string]int{"x": 4}
	var dynamic any = "value"
	text, ok := dynamic.(string)
	converted := string([]byte{'g', 'o'})
	result := (bag.At(0) + values[0]*mapping["x"]) == 9
	fmt.Println(result, text, ok, converted, variadic(1, 2, 3))
}
