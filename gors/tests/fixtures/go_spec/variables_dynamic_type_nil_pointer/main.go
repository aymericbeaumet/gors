package main



type T struct{ v int }

func typed() *T { return nil }

func main() {
	var x interface{}
	println(x == nil)
	var v *T
	x = 42
	_, isInt := x.(int)
	x = v
	println(x == nil, isInt)
	if x == nil {
		panic("interface holding nil *T compares equal to nil")
	}
	pt, ok := x.(*T)
	if !ok || pt != nil {
		panic("dynamic type is not *T with nil value")
	}
	switch x.(type) {
	case *T:
		println("dynamic type *T")
	default:
		panic("type switch missed *T")
	}
	var y interface{} = typed()
	println(y != nil, x == y)
}
