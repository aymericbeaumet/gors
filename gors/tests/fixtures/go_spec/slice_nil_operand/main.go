package main

func main() {
	var s []int

	simple := s[:0]
	explicit := s[0:0]
	whole := s[:]
	full := s[0:0:0]

	if simple != nil || explicit != nil || whole != nil || full != nil {
		panic("slicing a nil slice did not yield a nil slice")
	}
	if len(whole) != 0 || cap(whole) != 0 || len(full) != 0 || cap(full) != 0 {
		panic("slicing a nil slice changed length or capacity")
	}

	// Bounds of a nil slice are checked against len 0 / cap 0: appending to
	// the nil result still works and produces an independent slice.
	grown := append(simple, 5)
	if grown[0] != 5 || len(grown) != 1 || s != nil {
		panic("append after slicing a nil slice changed")
	}

	// An empty string can be sliced at its boundaries; s[len(s):] is valid
	// even though the index expression s[len(s)] would panic.
	empty := ""
	text := "go"
	if empty[0:0] != "" || text[2:] != "" || text[:0] != "" || text[1:2] != "o" {
		panic("string slice boundary behavior changed")
	}
	println("slice-nil-operand: ok")
}
