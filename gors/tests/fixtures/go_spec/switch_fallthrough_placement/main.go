package main

func main() {
	trace := ""
	switch 5 {
	case 1:
		trace += "one,"
	default:
		trace += "default,"
		fallthrough
	case 2:
		trace += "two,"
	}
	if trace != "default,two," {
		panic("fallthrough from a middle default must run the next source-order clause")
	}

	out := ""
	switch 1 {
	case 1:
		out += "one,"
		fallthrough
	default:
		out += "default,"
	}
	if out != "one,default," {
		panic("fallthrough into a trailing default changed")
	}

	tail := ""
	switch 1 {
	case 1:
		tail += "a"
		fallthrough
		;
	case 2:
		tail += "b"
	}
	if tail != "ab" {
		panic("an empty statement after fallthrough changed placement")
	}

	labeled := ""
	switch 1 {
	case 1:
		labeled += "a"
		goto Next
	Next:
		fallthrough
	case 2:
		labeled += "b"
	}
	if labeled != "ab" {
		panic("labeled fallthrough changed")
	}

	println(trace, out, tail, labeled)
}
