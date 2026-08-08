package main

func main() {
	var a any = int64(3)
	got := ""
	switch a {
	case 3:
		got = "int"
	case int64(3):
		got = "int64"
	}
	if got != "int64" {
		panic("untyped case constant against interface switch value must convert to its default type")
	}

	var b any = 3
	got2 := ""
	switch b {
	case int64(3):
		got2 = "int64"
	case 3:
		got2 = "int"
	}
	if got2 != "int" {
		panic("interface switch dynamic type comparison changed")
	}

	var c any
	got3 := ""
	switch c {
	case nil:
		got3 = "nil"
	default:
		got3 = "non-nil"
	}
	if got3 != "nil" {
		panic("nil interface must equal nil case in expression switch")
	}

	var p *int
	var d any = p
	got4 := ""
	switch d {
	case nil:
		got4 = "nil"
	default:
		got4 = "typed-nil"
	}
	if got4 != "typed-nil" {
		panic("interface holding typed nil pointer must not equal untyped nil case")
	}

	got5 := ""
	switch 1e2 {
	case 100:
		got5 = "hundred"
	default:
		got5 = "other"
	}
	if got5 != "hundred" {
		panic("untyped float switch expression default-type conversion changed")
	}

	println(got, got2, got3, got4, got5)
}
