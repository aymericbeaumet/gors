package main

func main() {
	values := map[string]int{"zero": 0, "answer": 42}
	zero, zeroOK := values["zero"]
	missing, missingOK := values["missing"]

	delete(values, "missing")
	clear(values)

	var nilMap map[string]int
	nilValue, nilOK := nilMap["missing"]
	delete(nilMap, "missing")
	clear(nilMap)
	iterations := 0
	for range nilMap {
		iterations++
	}

	if zero != 0 || !zeroOK || missing != 0 || missingOK {
		panic("map comma-ok lookup changed")
	}
	if len(values) != 0 {
		panic("clear did not remove map entries")
	}
	if nilMap != nil || len(nilMap) != 0 || nilValue != 0 || nilOK || iterations != 0 {
		panic("nil map read behavior changed")
	}
	println("map-lookup-and-nil: ok")
}
