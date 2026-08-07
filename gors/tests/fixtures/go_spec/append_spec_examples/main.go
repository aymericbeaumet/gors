package main

func equal(got []int, want ...int) bool {
	if len(got) != len(want) {
		return false
	}
	for i := range got {
		if got[i] != want[i] {
			return false
		}
	}
	return true
}

func main() {
	s0 := []int{0, 0}
	s1 := append(s0, 2)
	s2 := append(s1, 3, 5, 7)
	s3 := append(s2, s0...)
	if !equal(s0, 0, 0) || !equal(s1, 0, 0, 2) || !equal(s2, 0, 0, 2, 3, 5, 7) {
		panic("append of elements changed")
	}
	if !equal(s3, 0, 0, 2, 3, 5, 7, 0, 0) {
		panic("append of a slice changed")
	}

	s4 := append(s3[3:6], s3[2:]...)
	if !equal(s4, 3, 5, 7, 2, 3, 5, 7, 0, 0) {
		panic("append of an overlapping slice changed")
	}

	var t []int
	t = append(t, 42)
	if !equal(t, 42) || cap(t) < 1 {
		panic("append to nil slice changed")
	}

	var mixed []any
	mixed = append(mixed, 42, 3.1415, "foo")
	if len(mixed) != 3 || mixed[0] != 42 || mixed[1] != 3.1415 || mixed[2] != "foo" {
		panic("append to []any changed")
	}

	base := []int{1}
	sameLen := append(base, nil...)
	sameAgain := append(base)
	if !equal(sameLen, 1) || !equal(sameAgain, 1) {
		panic("append with empty expansions changed")
	}

	println("append-spec-examples: ok")
}
