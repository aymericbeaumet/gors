package main

func triggerOutOfBounds(s []int, idx int) {
	_ = s[idx]
}

func testOutOfBounds() {
	defer func() {
		if r := recover(); r == nil {
			panic("out-of-bounds panic was not recovered")
		}
	}()
	s := []int{1}
	triggerOutOfBounds(s, 2)
	panic("out-of-bounds panic continued after recovery")
}

func triggerNilPointer(p *int) {
	_ = *p
}

func testNilPointer() {
	defer func() {
		if r := recover(); r == nil {
			panic("nil pointer panic was not recovered")
		}
	}()
	triggerNilPointer(nil)
	panic("nil pointer panic continued after recovery")
}

func triggerDivideByZero(a, b int) {
	_ = a / b
}

func testDivideByZero() {
	defer func() {
		if r := recover(); r == nil {
			panic("divide-by-zero panic was not recovered")
		}
	}()
	triggerDivideByZero(1, 0)
	panic("divide-by-zero panic continued after recovery")
}

func main() {
	testOutOfBounds()
	testNilPointer()
	testDivideByZero()
}
