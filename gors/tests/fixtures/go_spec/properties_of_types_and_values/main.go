package main

type Score struct {
	Value int
}

func (s Score) Double() int {
	return s.Value * 2
}

func main() {
	var number int = 12
	alias := number
	left := Score{Value: 3}
	right := Score{Value: 3}
	method := left.Double
	if alias != 12 {
		panic("value alias changed")
	}
	if left != right {
		panic("comparable struct equality changed")
	}
	if method() != 6 {
		panic("method value result changed")
	}
}
