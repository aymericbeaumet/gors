package main

func main() {
	withCR := `a
b
c`
	if withCR != "a\nb\nc" {
		panic("carriage returns inside raw string literals must be discarded")
	}
	if len(withCR) != 5 {
		panic("raw string length must not count discarded carriage returns")
	}
	multi := `\n
\n`
	if multi != "\\n\n\\n" {
		panic("raw string must keep backslashes uninterpreted and newlines intact")
	}
	println(len(withCR), len(multi))
	println(withCR == "a\nb\nc", multi == "\\n\n\\n")
}
