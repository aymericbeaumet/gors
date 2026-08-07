package main

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
	single := dynamic.(string)
	missing, missingOK := dynamic.(int)
	converted := string([]byte{'g', 'o'})
	double := func(value int) int { return value * 2 }
	shifted := 1 << 3
	bitCleared := shifted &^ 2
	order := []int{}
	next := func(value int) int {
		order = append(order, value)
		return value
	}
	ordered := []int{next(1), next(2), next(3)}
	result := (bag.At(0) + values[0]*mapping["x"]) == 9
	if !result || text != "value" || !ok || converted != "go" {
		panic("basic expression result changed")
	}
	if single != "value" || missing != 0 || missingOK {
		panic("basic expression result changed")
	}
	if variadic(1, 2, 3) != 6 || double(values[1]) != 6 || bitCleared != 8 {
		panic("call or arithmetic expression changed")
	}
	if ordered[0] != 1 || ordered[1] != 2 || ordered[2] != 3 {
		panic("composite literal evaluation order changed")
	}
	if order[0] != 1 || order[1] != 2 || order[2] != 3 {
		panic("call evaluation order changed")
	}

	// Index expressions: array read+write, string byte read, slice write, map write.
	array := [3]int{10, 20, 30}
	array[1] = array[0] + array[2]
	if array[1] != 40 {
		panic("array index write changed")
	}
	if "go"[1] != 'o' || converted[0] != 'g' {
		panic("string index read changed")
	}
	values[0] = 7
	if bag.values[1] != 7 {
		panic("slice index write changed")
	}
	mapping["y"] = mapping["x"] + 1
	if mapping["y"] != 5 || mapping["missing"] != 0 {
		panic("map index write changed")
	}
}
