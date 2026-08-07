package main

func main() {
	var a struct {
		X int "one"
	}
	var b struct {
		X int "two"
	}
	// illegal: tags take part in type identity for structs, so these two
	// struct types are different and b is not assignable to a.
	a = b
	_ = a
}
