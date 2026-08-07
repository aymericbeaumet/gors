package main

type Score struct {
	Value int
}

func (s Score) Double() int {
	return s.Value * 2
}

type Doubler interface {
	Double() int
}

func main() {
	var number int = 12
	alias := number
	number = 99
	left := Score{Value: 3}
	right := Score{Value: 3}
	method := left.Double
	if alias != 12 {
		panic("value alias changed")
	}
	if alias != 12 || number != 99 {
		panic("value copy must not track later writes to the source")
	}
	if left != right {
		panic("comparable struct equality changed")
	}
	if method() != 6 {
		panic("method value result changed")
	}

	// Assignability: predeclared nil assigns to pointer, map, slice types.
	var p *int = nil
	var m map[string]int = nil
	var ns []int = nil
	if p != nil || m != nil || ns != nil {
		panic("nil assignment changed")
	}

	// Assignability: T is an interface type and x implements T.
	var d Doubler = left
	if d.Double() != 6 {
		panic("interface assignment changed dynamic dispatch")
	}

	// Copy semantics: arrays copy their elements on assignment.
	arr := [3]int{1, 2, 3}
	arrCopy := arr
	arr[0] = 100
	if arrCopy[0] != 1 || arr[0] != 100 {
		panic("array copy must not share backing storage")
	}

	// Copy semantics: structs copy their fields on assignment.
	shadow := left
	left.Value = 42
	if shadow.Value != 3 {
		panic("struct copy must not track later writes to the source")
	}

	// A method value with a value receiver captures a copy of the receiver
	// at binding time.
	shadowMethod := shadow.Double
	shadow.Value = 50
	if shadowMethod() != 6 {
		panic("method value must capture the receiver copy at binding time")
	}
}
