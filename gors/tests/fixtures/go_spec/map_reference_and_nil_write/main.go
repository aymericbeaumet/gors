package main

func nilMapWritePanics() (panicked bool) {
	defer func() {
		panicked = recover() != nil
	}()

	var values map[string]int
	values["missing"] = 1
	return false
}

func main() {
	original := map[string]int{"value": 1}
	alias := original
	alias["value"] = 2

	if original["value"] != 2 {
		panic("map assignment did not preserve shared map identity")
	}
	if !nilMapWritePanics() {
		panic("assignment to a nil map did not panic")
	}
	println("map-reference-and-nil-write: ok")
}
